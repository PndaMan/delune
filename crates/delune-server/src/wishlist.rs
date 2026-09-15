//! The wishlist: searches that keep running until something good turns up.
//!
//! Soulseek gives every client a wishlist allowance separate from normal searches:
//! one saved search per interval (the server says how long, usually twelve
//! minutes). delune works through the list oldest-first, one item per interval,
//! keeps the best copy each search finds, and, when the item asks for it, starts a
//! download of that copy for review. Nothing skips review, and albums already
//! complete in the library aren't downloaded again.
//!
//! Items live in `<data dir>/wishlist.json`.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{
    ApiError, Candidate, DownloadJobRequest, LibraryState, QualityTier, RequestedFile, WishlistItem, WishlistRequest,
    WishlistUpdate,
};
use delune_soulseek::SessionState;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

const MAX_ITEMS: usize = 500;
const FIRST_RUN_AFTER: Duration = Duration::from_secs(45);

#[derive(Debug, Default)]
pub struct Wishlist {
    store: Option<Arc<Database>>,
    items: Mutex<Vec<WishlistItem>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Wishlist {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let items = db.load("wishlist").unwrap_or_default();
        Self { store: Some(db.clone()), items: Mutex::new(items) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<WishlistItem>> {
        self.items.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, items: &[WishlistItem]) {
        if let Some(db) = &self.store {
            db.save("wishlist", items);
        }
    }

    /// The item to search for next: active, not yet downloaded, searched longest ago.
    fn next(&self) -> Option<WishlistItem> {
        self.lock()
            .iter()
            .filter(|i| !i.paused && i.download_id.is_none())
            .min_by_key(|i| i.last_searched.unwrap_or(0))
            .cloned()
    }

    /// Change the list directly, saving afterwards.
    pub(crate) fn with_items<R>(&self, change: impl FnOnce(&mut Vec<WishlistItem>) -> R) -> R {
        let mut items = self.lock();
        let result = change(&mut items);
        self.save(&items);
        result
    }

    /// Ids of the items that started download `job_id`.
    pub(crate) fn started(&self, job_id: &str) -> Vec<String> {
        self.lock().iter().filter(|i| i.download_id.as_deref() == Some(job_id)).map(|i| i.id.clone()).collect()
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut WishlistItem)) {
        let mut items = self.lock();
        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            change(item);
        }
        self.save(&items);
    }
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The shared file that is this song, if the folder has it.
fn track_file<'a>(candidate: &'a Candidate, title: &str) -> Option<&'a delune_core::api::CandidateFile> {
    let wanted = squash(title);
    if wanted.len() < 2 {
        return None;
    }
    candidate.files.iter().find(|f| f.audio && squash(&f.name).contains(&wanted))
}

/// The best copy among `candidates` that `item` would accept.
fn best_match(item: &WishlistItem, mut candidates: Vec<Candidate>) -> (u32, Option<Candidate>) {
    candidates.retain(|c| c.audio_files > 0 && item.min_quality.accepts(QualityTier::of(c.quality)));
    if let Some(title) = &item.track {
        candidates.retain(|c| track_file(c, title).is_some());
    }
    Candidate::rank(&mut candidates);
    (u32::try_from(candidates.len()).unwrap_or(u32::MAX), candidates.into_iter().next())
}

/// Work through the wishlist for as long as the server runs. Call once at startup.
pub fn start(app: &AppState) {
    let Some(client) = app.soulseek.clone() else { return };
    // Newly imported playlist items get a first, normal search soon, paced by the
    // search limiter, instead of waiting a wishlist interval each.
    {
        let (app, client) = (app.clone(), client.clone());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let online = matches!(*client.state().borrow(), SessionState::Online { .. });
                let fresh = app
                    .wishlist
                    .lock()
                    .iter()
                    .find(|i| i.playlist.is_some() && i.last_searched.is_none() && !i.paused)
                    .cloned();
                if online && let Some(item) = fresh {
                    match client.search(&item.query).await {
                        Ok(search) => finish_run(&app, item, search).await,
                        Err(error) => tracing::debug!(%error, "playlist search didn't start"),
                    }
                }
            }
        });
    }
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_RUN_AFTER).await;
        loop {
            let online = matches!(*client.state().borrow(), SessionState::Online { .. });
            if online && let Some(item) = app.wishlist.next() {
                run_one(&app, &client, item).await;
            }
            tokio::time::sleep(client.wishlist_interval()).await;
        }
    });
}

async fn run_one(app: &AppState, client: &delune_soulseek::Client, item: WishlistItem) {
    match client.wishlist_search(&item.query) {
        Ok(search) => finish_run(app, item, search).await,
        Err(error) => tracing::debug!(%error, query = %item.query, "wishlist search didn't start"),
    }
}

async fn finish_run(app: &AppState, item: WishlistItem, mut search: delune_soulseek::Search) {
    let mut candidates = Vec::new();
    while let Some(response) = search.next().await {
        candidates.extend(crate::search::candidates(&response));
    }
    let (matches, best) = best_match(&item, candidates);
    tracing::info!(query = %item.query, matches, "wishlist searched");

    let mut download_id = None;
    if item.auto_download
        && let Some(best) = &best
    {
        let owned = crate::library::lookup(app, best.parent.as_deref(), &best.title, Some(&item.query)).await;
        let complete = item.track.is_none()
            && owned.state == LibraryState::InLibrary
            && owned.tracks.len() >= usize::try_from(best.audio_files).unwrap_or(usize::MAX);
        if complete {
            tracing::info!(query = %item.query, "already in the library; not downloading again");
        } else {
            let request = DownloadJobRequest {
                username: best.username.clone(),
                folder: best.folder.clone(),
                title: best.title.clone(),
                parent: best.parent.clone(),
                // A single song downloads just that file, with any artwork beside it.
                files: best
                    .files
                    .iter()
                    .filter(|f| match &item.track {
                        Some(title) => track_file(best, title).is_some_and(|t| t.path == f.path) || is_image(&f.name),
                        None => true,
                    })
                    .map(|f| RequestedFile { path: f.path.clone(), size: f.size })
                    .collect(),
            };
            match crate::downloads::begin(app, request, &item.added_by) {
                Ok(job) => download_id = Some(job.id),
                Err((_, _, message)) => tracing::warn!(query = %item.query, %message, "wishlist download didn't start"),
            }
        }
    }

    app.wishlist.update(&item.id, |i| {
        i.last_searched = Some(now());
        i.last_matches = matches;
        if best.is_some() {
            i.best = best;
        }
        if download_id.is_some() {
            i.download_id = download_id;
        }
    });
}

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".webp"].iter().any(|ext| lower.ends_with(ext))
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// `GET /api/v1/wishlist`: yours, or everyone's if you manage delune.
pub async fn list(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<WishlistItem>> {
    let mut items: Vec<WishlistItem> =
        app.wishlist.lock().iter().filter(|i| user.can_see(Some(&i.added_by))).cloned().collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.added_at));
    Json(items)
}

/// `POST /api/v1/wishlist`
pub async fn add(State(app): State<AppState>, user: CurrentUser, Json(request): Json<WishlistRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.search && p.download, "use the wishlist") {
        return denied;
    }
    let mut items = app.wishlist.lock();
    let result = insert(&mut items, &user.username, request);
    app.wishlist.save(&items);
    match result {
        Ok((true, item)) => (StatusCode::CREATED, Json(item)).into_response(),
        Ok((false, item)) => Json(item).into_response(),
        Err((status, code, message)) => error(status, code, message),
    }
}

/// `POST /api/v1/wishlist/batch`: many items at once, such as a playlist.
pub async fn add_many(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(requests): Json<Vec<WishlistRequest>>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.search && p.download, "use the wishlist") {
        return denied;
    }
    let mut items = app.wishlist.lock();
    let mut added = 0u32;
    for request in requests {
        match insert(&mut items, &user.username, request) {
            Ok((true, _)) => added += 1,
            Ok((false, _)) | Err((_, "query-too-short", _)) => {}
            Err((status, code, message)) => {
                app.wishlist.save(&items);
                return error(status, code, message);
            }
        }
    }
    app.wishlist.save(&items);
    Json(serde_json::json!({ "added": added })).into_response()
}

/// Add an item on someone's behalf, or return theirs that's already there.
///
/// # Errors
///
/// The status, code and message when it can't be added.
pub(crate) fn insert_for(
    app: &AppState,
    username: &str,
    request: WishlistRequest,
) -> Result<WishlistItem, (StatusCode, &'static str, &'static str)> {
    let mut items = app.wishlist.lock();
    let result = insert(&mut items, username, request);
    app.wishlist.save(&items);
    result.map(|(_, item)| item)
}

/// Add one item, or return the matching one already there (`false`).
fn insert(
    items: &mut Vec<WishlistItem>,
    username: &str,
    request: WishlistRequest,
) -> Result<(bool, WishlistItem), (StatusCode, &'static str, &'static str)> {
    let query = request.query.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.chars().count() < 3 {
        return Err((StatusCode::BAD_REQUEST, "query-too-short", "Wishlist searches need a few more letters."));
    }
    if let Some(existing) = items.iter().find(|i| i.query.eq_ignore_ascii_case(&query) && i.added_by == username) {
        return Ok((false, existing.clone()));
    }
    if items.len() >= MAX_ITEMS {
        return Err((StatusCode::CONFLICT, "wishlist-full", "The wishlist is full. Remove something first."));
    }
    let item = WishlistItem {
        id: format!("w{:x}{:04x}", now(), items.len()),
        query,
        track: request.track.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()),
        playlist: request.playlist,
        added_by: username.to_owned(),
        added_at: now(),
        auto_download: request.auto_download,
        min_quality: request.min_quality,
        paused: false,
        last_searched: None,
        last_matches: 0,
        best: None,
        download_id: None,
    };
    items.push(item.clone());
    Ok((true, item))
}

/// `PATCH /api/v1/wishlist/{id}`
pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(id): UrlPath<String>,
    Json(update): Json<WishlistUpdate>,
) -> Response {
    let mut items = app.wishlist.lock();
    let Some(item) = items.iter_mut().find(|i| i.id == id && user.can_see(Some(&i.added_by))) else {
        return error(StatusCode::NOT_FOUND, "no-such-item", "That isn't on the wishlist.");
    };
    if let Some(auto_download) = update.auto_download {
        item.auto_download = auto_download;
    }
    if let Some(min_quality) = update.min_quality {
        item.min_quality = min_quality;
    }
    if let Some(paused) = update.paused {
        item.paused = paused;
    }
    let item = item.clone();
    app.wishlist.save(&items);
    Json(item).into_response()
}

/// `DELETE /api/v1/wishlist/{id}`
pub async fn remove(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let mut items = app.wishlist.lock();
    let before = items.len();
    items.retain(|i| !(i.id == id && user.can_see(Some(&i.added_by))));
    if items.len() == before {
        return error(StatusCode::NOT_FOUND, "no-such-item", "That isn't on the wishlist.");
    }
    app.wishlist.save(&items);
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use delune_core::api::MinQuality;
    use delune_core::{Codec, Quality};

    fn candidate(id: &str, quality: Quality, files: u32) -> Candidate {
        Candidate {
            id: id.into(),
            username: "u".into(),
            folder: id.into(),
            title: id.into(),
            parent: None,
            files: vec![],
            audio_files: files,
            total_bytes: 1,
            duration_secs: None,
            quality_label: None,
            quality_rank: quality.rank(),
            quality: Some(quality),
            mixed_quality: false,
            has_cover: false,
            free_slot: true,
            avg_speed: 1,
            queue_length: 0,
            peer: None,
        }
    }

    fn item(min_quality: MinQuality) -> WishlistItem {
        WishlistItem {
            id: "w".into(),
            query: "q".into(),
            track: None,
            playlist: None,
            added_by: "sam".into(),
            added_at: 0,
            auto_download: true,
            min_quality,
            paused: false,
            last_searched: None,
            last_matches: 0,
            best: None,
            download_id: None,
        }
    }

    #[test]
    fn picks_the_best_copy_that_meets_the_bar() {
        let found = vec![
            candidate("mp3", Quality::lossy(Codec::Mp3, 320), 10),
            candidate("cd", Quality::lossless(Codec::Flac, 16, 44_100), 10),
            candidate("hires", Quality::lossless(Codec::Flac, 24, 96_000), 10),
        ];
        let (matches, best) = best_match(&item(MinQuality::Lossless), found.clone());
        assert_eq!((matches, best.unwrap().id.as_str()), (2, "hires"));

        let (matches, best) = best_match(&item(MinQuality::HiRes), found[..2].to_vec());
        assert_eq!((matches, best), (0, None));

        let (matches, _) = best_match(&item(MinQuality::Any), found);
        assert_eq!(matches, 3);
    }

    #[test]
    fn searches_the_longest_waiting_item_first() {
        let wishlist = Wishlist::default();
        let mut a = item(MinQuality::Any);
        a.id = "a".into();
        a.last_searched = Some(200);
        let mut b = item(MinQuality::Any);
        b.id = "b".into();
        b.last_searched = Some(100);
        let mut done = item(MinQuality::Any);
        done.id = "done".into();
        done.download_id = Some("job".into());
        *wishlist.lock() = vec![a, b, done];
        assert_eq!(wishlist.next().unwrap().id, "b");
    }
}
