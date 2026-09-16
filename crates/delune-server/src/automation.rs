//! Automation: following artists and upgrading lossy albums. Both off by default.
//!
//! - **Followed artists.** Once a day each followed artist's releases are fetched
//!   from Deezer's public API. Albums and EPs released since the follow go onto
//!   the wishlist, which finds them on Soulseek and (if allowed) downloads them for
//!   review.
//! - **Quality upgrades.** A slow walk through the Navidrome library, a few dozen
//!   albums an hour, looking for albums whose tracks are lossy. Each gets a wishlist
//!   search for a better copy. The walk resumes where it left off.
//!
//! Nothing here imports anything: everything still stops at review.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{
    AlbumFollow, ApiError, AutomationSettings, Follow, LibraryState, MinQuality, RadarRelease, WishlistItem,
};
use delune_core::{Codec, Quality};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

const DEEZER: &str = "https://api.deezer.com";
const CHECK_FOLLOWS_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const UPGRADE_BATCH: u32 = 40;
const UPGRADE_EVERY: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Default)]
pub struct Automation {
    store: Option<Arc<Database>>,
    state: Mutex<Stored>,
    http: reqwest::Client,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    settings: AutomationSettings,
    #[serde(default)]
    follows: Vec<Follow>,
    #[serde(default)]
    albums: Vec<AlbumFollow>,
    /// Where the library walk continues.
    #[serde(default)]
    upgrade_offset: u32,
    /// Albums already queued for an upgrade, by Navidrome id.
    #[serde(default)]
    upgrades_queued: HashSet<String>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Automation {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let state = db.load("automation").unwrap_or_default();
        Self { store: Some(db.clone()), state: Mutex::new(state), http: crate::music::http_client() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Stored> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, state: &Stored) {
        if let Some(db) = &self.store {
            db.save("automation", state);
        }
    }

    /// Whether automation-made wishlist items download on their own.
    pub(crate) fn auto_download(&self) -> bool {
        self.lock().settings.auto_download
    }

    async fn deezer(&self, path: &str) -> Option<Value> {
        let response = self.http.get(format!("{DEEZER}{path}")).timeout(Duration::from_secs(15)).send().await.ok()?;
        let value: Value = response.json().await.ok()?;
        value.get("error").is_none().then_some(value)
    }
}

/// Where quality-upgrade wishlist items say they came from.
pub(crate) const UPGRADE_SOURCE: &str = "Quality upgrades";

/// Add an automation-made item to the wishlist unless the same search is there.
pub(crate) fn queue(
    app: &AppState,
    query: String,
    track: Option<String>,
    added_by: &str,
    min_quality: MinQuality,
    auto_download: bool,
    source: &str,
) {
    app.wishlist.with_items(|items| {
        // Someone else wishing for the same thing doesn't cover this person.
        if items.iter().any(|i| i.query.eq_ignore_ascii_case(&query) && i.track == track && i.added_by == added_by)
            || crate::wishlist::active(items) >= crate::wishlist::MAX_ITEMS
        {
            return;
        }
        let item = WishlistItem {
            id: crate::store::new_id("a"),
            query,
            track,
            playlist: Some(source.to_owned()),
            added_by: added_by.to_owned(),
            added_at: now(),
            auto_download,
            min_quality,
            paused: false,
            last_searched: None,
            last_matches: 0,
            best: None,
            download_id: None,
        };
        items.push(item);
    });
}

/// Run both jobs for as long as the server runs. Call once at startup.
pub fn start(app: &AppState) {
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(120)).await;
            loop {
                // Following an artist is the opt-in; there's nothing else to switch on.
                check_follows(&app).await;
                check_albums(&app).await;
                tokio::time::sleep(Duration::from_secs(60 * 60)).await;
            }
        });
    }
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        loop {
            if app.automation.lock().settings.quality_upgrades {
                look_for_upgrades(&app).await;
            }
            tokio::time::sleep(UPGRADE_EVERY).await;
        }
    });
}

async fn check_follows(app: &AppState) {
    let due: Vec<Follow> = app
        .automation
        .lock()
        .follows
        .iter()
        .filter(|f| f.last_checked.is_none_or(|t| now().saturating_sub(t) >= CHECK_FOLLOWS_EVERY.as_secs()))
        .cloned()
        .collect();
    let settings = app.automation.lock().settings.clone();
    for follow in due {
        let Some(albums) = app.automation.deezer(&format!("/artist/{}/albums?limit=50", follow.deezer_id)).await else {
            continue;
        };
        let mut seen: Vec<u64> = follow.seen.clone();
        for album in albums.get("data").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), Some(title)) =
                (album.get("id").and_then(Value::as_u64), album.get("title").and_then(Value::as_str))
            else {
                continue;
            };
            let kind = album.get("record_type").and_then(Value::as_str).unwrap_or("album");
            let released = album
                .get("release_date")
                .and_then(Value::as_str)
                .and_then(|d| d.get(..10))
                .and_then(release_timestamp)
                .unwrap_or(0);
            if seen.contains(&id) || released < follow.since || !matches!(kind, "album" | "ep") {
                continue;
            }
            tracing::info!(artist = %follow.artist, %title, "new release from a followed artist");
            queue(
                app,
                format!("{} {title}", follow.artist),
                None,
                &follow.added_by,
                MinQuality::Lossless,
                settings.auto_download,
                &format!("{} (followed)", follow.artist),
            );
            let kind_name = if kind == "ep" { "EP" } else { "album" };
            app.notifications.notify(
                &follow.added_by,
                delune_core::api::NotificationKind::NewRelease,
                format!("New {kind_name} from {}: {title}", follow.artist),
                Some(if settings.auto_download {
                    "It's on your wishlist, and will download for review when a good copy turns up.".into()
                } else {
                    "It's on your wishlist.".into()
                }),
                "/radar",
            );
            seen.push(id);
        }
        let mut state = app.automation.lock();
        if let Some(f) =
            state.follows.iter_mut().find(|f| f.deezer_id == follow.deezer_id && f.added_by == follow.added_by)
        {
            f.seen = seen;
            f.last_checked = Some(now());
        }
        app.automation.save(&state);
    }
}

/// How many tracks one check may put on the wishlist.
const TRACKS_PER_CHECK: usize = 50;
/// What `queued` holds once the whole album has been wished for.
const WHOLE_ALBUM: &str = "*album*";

async fn check_albums(app: &AppState) {
    let due: Vec<AlbumFollow> = app
        .automation
        .lock()
        .albums
        .iter()
        .filter(|f| f.last_checked.is_none_or(|t| now().saturating_sub(t) >= CHECK_FOLLOWS_EVERY.as_secs()))
        .cloned()
        .collect();
    for follow in due {
        check_album(app, follow).await;
    }
}

/// Tracks on the album that the library doesn't have and haven't been asked for, with
/// their comparison keys.
fn missing_tracks<'a>(
    tracklist: &'a [delune_library::merge::ListedTrack],
    library: &[&str],
    queued: &[String],
) -> Vec<(&'a delune_library::merge::ListedTrack, String)> {
    use delune_library::merge::{same_song, title_key};
    let have: Vec<String> = library.iter().map(|t| title_key(t)).collect();
    tracklist
        .iter()
        .map(|t| (t, title_key(&t.title)))
        .filter(|(_, key)| !key.is_empty() && !have.iter().any(|h| same_song(h, key)))
        .filter(|(_, key)| !queued.contains(key))
        .take(TRACKS_PER_CHECK)
        .collect()
}

/// Put what's missing from a followed album on the wishlist.
async fn check_album(app: &AppState, follow: AlbumFollow) {
    let Some((tracklist, _)) = crate::music::tracklist_of(app, follow.id).await else { return };
    let library = crate::library::lookup(app, Some(&follow.artist), &follow.title, None).await;
    let auto_download = app.automation.lock().settings.auto_download;
    let source = format!("{} (followed album)", follow.title);
    let mut queued = follow.queued.clone();
    match library.state {
        // Can't tell what's there: try again tomorrow rather than fetch it all.
        LibraryState::Unknown => {}
        LibraryState::NotInLibrary => {
            if !queued.iter().any(|q| q == WHOLE_ALBUM) {
                tracing::info!(album = %follow.title, "followed album isn't in the library; wishing for it");
                queue(
                    app,
                    format!("{} {}", follow.artist, follow.title),
                    None,
                    &follow.added_by,
                    MinQuality::Lossless,
                    auto_download,
                    &source,
                );
                queued.push(WHOLE_ALBUM.to_owned());
            }
        }
        LibraryState::InLibrary => {
            let have: Vec<&str> = library.tracks.iter().map(|t| t.title.as_str()).collect();
            for (track, key) in missing_tracks(&tracklist, &have, &queued) {
                tracing::info!(album = %follow.title, track = %track.title, "followed album is missing a track");
                // Searching for the album finds folders of it; the wishlist takes just this
                // song from one, and the import files it with the rest.
                queue(
                    app,
                    format!("{} {}", follow.artist, follow.title),
                    Some(track.title.clone()),
                    &follow.added_by,
                    MinQuality::Lossless,
                    auto_download,
                    &source,
                );
                queued.push(key);
            }
        }
    }
    let mut state = app.automation.lock();
    if let Some(f) = state.albums.iter_mut().find(|f| f.id == follow.id && f.added_by == follow.added_by) {
        f.queued = queued;
        f.tracks = u32::try_from(tracklist.len()).unwrap_or(u32::MAX);
        f.last_checked = Some(now());
    }
    app.automation.save(&state);
    drop(state);
    crate::events::changed(app, crate::events::Topic::Follows);
}

/// "2026-09-01" → Unix seconds at midnight UTC.
fn release_timestamp(date: &str) -> Option<u64> {
    let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Days from civil, Howard Hinnant's algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400).ok()
}

fn song_quality(suffix: Option<&str>, bit_rate: Option<u32>) -> Option<Quality> {
    let codec = Codec::from_extension(suffix?)?;
    Some(Quality {
        codec,
        bit_depth: None,
        sample_rate: None,
        bitrate_kbps: if codec.is_lossless() { None } else { bit_rate },
        vbr: false,
    })
}

async fn look_for_upgrades(app: &AppState) {
    let Some(navidrome) = &app.navidrome else { return };
    let (offset, settings) = {
        let state = app.automation.lock();
        (state.upgrade_offset, state.settings.clone())
    };
    let albums = match navidrome.albums(offset, UPGRADE_BATCH).await {
        Ok(albums) => albums,
        Err(error) => {
            tracing::debug!(%error, "couldn't list the library for upgrades");
            return;
        }
    };
    let next_offset = if albums.len() < UPGRADE_BATCH as usize { 0 } else { offset + UPGRADE_BATCH };
    for album in albums {
        if app.automation.lock().upgrades_queued.contains(&album.id) {
            continue;
        }
        let Ok(full) = navidrome.album(&album.id).await else { continue };
        let lossy = full
            .song
            .iter()
            .filter_map(|s| song_quality(s.suffix.as_deref(), s.bit_rate))
            .any(|q| !q.codec.is_lossless());
        let wants_hires = settings.upgrade_to == MinQuality::HiRes
            && full.song.iter().all(|s| s.bit_depth.unwrap_or(16) <= 16 && s.sampling_rate.unwrap_or(44_100) <= 48_000);
        if !(lossy || wants_hires) {
            continue;
        }
        let artist = full.artist.clone().unwrap_or_default();
        tracing::info!(%artist, album = %full.name, "looking for a better copy");
        queue(
            app,
            format!("{artist} {}", full.name).trim().to_owned(),
            None,
            "delune",
            settings.upgrade_to,
            settings.auto_download,
            UPGRADE_SOURCE,
        );
        let mut state = app.automation.lock();
        state.upgrades_queued.insert(album.id.clone());
        app.automation.save(&state);
    }
    let mut state = app.automation.lock();
    state.upgrade_offset = next_offset;
    app.automation.save(&state);
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// `GET /api/v1/automation`
#[utoipa::path(
    get,
    operation_id = "automation_settings",
    path = "/api/v1/automation",
    tag = "automation",
    responses(
        (status = 200, description = "OK", body = delune_core::api::AutomationSettings),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn settings(State(app): State<AppState>, _user: CurrentUser) -> Json<AutomationSettings> {
    Json(app.automation.lock().settings.clone())
}

/// `PUT /api/v1/automation`
#[utoipa::path(
    put,
    operation_id = "automation_update",
    path = "/api/v1/automation",
    tag = "automation",
    request_body = delune_core::api::AutomationSettings,
    responses(
        (status = 200, description = "OK", body = delune_core::api::AutomationSettings),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(settings): Json<AutomationSettings>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "change automation") {
        return denied;
    }
    let mut state = app.automation.lock();
    state.settings = settings.clone();
    app.automation.save(&state);
    drop(state);
    crate::events::changed(&app, crate::events::Topic::Automation);
    Json(settings).into_response()
}

/// `GET /api/v1/follows`: yours, or everyone's if you manage delune.
#[utoipa::path(
    get,
    operation_id = "automation_follows",
    path = "/api/v1/follows",
    tag = "automation",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::Follow>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn follows(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<Follow>> {
    Json(app.automation.lock().follows.iter().filter(|f| user.can_see(Some(&f.added_by))).cloned().collect())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FollowRequest {
    artist: String,
}

/// `POST /api/v1/follows`: find the artist on Deezer and follow them.
#[utoipa::path(
    post,
    operation_id = "automation_follow",
    path = "/api/v1/follows",
    tag = "automation",
    request_body = FollowRequest,
    responses(
        (status = 201, description = "Following", body = delune_core::api::Follow),
        (status = 200, description = "Already following", body = delune_core::api::Follow),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn follow(State(app): State<AppState>, user: CurrentUser, Json(request): Json<FollowRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.search && p.download, "follow artists") {
        return denied;
    }
    let name = request.artist.trim();
    if name.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no-artist", "Which artist?");
    }
    let query = url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>();
    let Some(found) = app.automation.deezer(&format!("/search/artist?q={query}&limit=5")).await else {
        return error(StatusCode::BAD_GATEWAY, "lookup-failed", "Couldn't look the artist up right now.");
    };
    let wanted = crate::artwork::normalize(name);
    let candidates = found.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let Some(artist) = candidates
        .iter()
        .find(|a| a.get("name").and_then(Value::as_str).is_some_and(|n| crate::artwork::normalize(n) == wanted))
        .or_else(|| candidates.first())
    else {
        return error(StatusCode::NOT_FOUND, "no-such-artist", "Couldn't find that artist.");
    };
    let (Some(id), Some(artist_name)) =
        (artist.get("id").and_then(Value::as_u64), artist.get("name").and_then(Value::as_str))
    else {
        return error(StatusCode::NOT_FOUND, "no-such-artist", "Couldn't find that artist.");
    };
    let mut state = app.automation.lock();
    if let Some(existing) = state.follows.iter().find(|f| f.deezer_id == id && f.added_by == user.username) {
        return Json(existing.clone()).into_response();
    }
    let follow = Follow {
        artist: artist_name.to_owned(),
        deezer_id: id,
        picture: artist.get("picture_medium").and_then(Value::as_str).map(str::to_owned),
        added_by: user.username.clone(),
        since: now(),
        last_checked: None,
        seen: Vec::new(),
    };
    state.follows.push(follow.clone());
    app.automation.save(&state);
    crate::events::changed(&app, crate::events::Topic::Follows);
    (StatusCode::CREATED, Json(follow)).into_response()
}

/// `DELETE /api/v1/follows/{deezer_id}`
#[utoipa::path(
    delete,
    operation_id = "automation_unfollow",
    path = "/api/v1/follows/{id}",
    tag = "automation",
    params(
        ("id" = u64, Path, description = "The artist's Deezer id"),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn unfollow(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<u64>) -> StatusCode {
    let mut state = app.automation.lock();
    state.follows.retain(|f| !(f.deezer_id == id && user.can_see(Some(&f.added_by))));
    app.automation.save(&state);
    drop(state);
    crate::events::changed(&app, crate::events::Topic::Follows);
    StatusCode::NO_CONTENT
}

/// Releases per followed artist, kept for a few hours.
/// An artist's releases, and when they were fetched.
type Releases = HashMap<u64, (u64, Vec<Value>)>;
static RADAR: std::sync::LazyLock<Mutex<Releases>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
const RADAR_KEEP: u64 = 6 * 60 * 60;
/// How far back the radar looks.
const RADAR_DAYS: u64 = 120;

async fn artist_releases(app: &AppState, id: u64) -> Vec<Value> {
    if let Some((at, albums)) = RADAR.lock().unwrap_or_else(PoisonError::into_inner).get(&id)
        && now().saturating_sub(*at) < RADAR_KEEP
    {
        return albums.clone();
    }
    let albums = app
        .automation
        .deezer(&format!("/artist/{id}/albums?limit=100"))
        .await
        .and_then(|v| v.get("data").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    RADAR.lock().unwrap_or_else(PoisonError::into_inner).insert(id, (now(), albums.clone()));
    albums
}

/// `GET /api/v1/radar`: new and upcoming releases from the artists you follow.
#[utoipa::path(
    get,
    operation_id = "automation_radar",
    path = "/api/v1/radar",
    tag = "automation",
    responses(
        (status = 200, description = "Newest first; upcoming ones first of all", body = Vec<delune_core::api::RadarRelease>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn radar(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<RadarRelease>> {
    let follows: Vec<Follow> = {
        let state = app.automation.lock();
        let mut seen = HashSet::new();
        state
            .follows
            .iter()
            .filter(|f| f.added_by == user.username || user.permissions.manage)
            .filter(|f| seen.insert(f.deezer_id))
            .cloned()
            .collect()
    };
    let cutoff = now().saturating_sub(RADAR_DAYS * 86_400);
    let today = now();
    let wished: Vec<String> = app
        .wishlist
        .snapshot()
        .into_iter()
        .filter(|i| i.track.is_none())
        .map(|i| crate::artwork::normalize(&i.query))
        .collect();
    let jobs = app.downloads.list();

    let mut found = Vec::new();
    for follow in follows.iter().take(200) {
        for album in artist_releases(&app, follow.deezer_id).await {
            let (Some(id), Some(title), Some(date)) = (
                album.get("id").and_then(Value::as_u64),
                album.get("title").and_then(Value::as_str),
                album.get("release_date").and_then(Value::as_str),
            ) else {
                continue;
            };
            let Some(released) = release_timestamp(date.get(..10).unwrap_or(date)) else { continue };
            if released < cutoff {
                continue;
            }
            let key = crate::artwork::normalize(&format!("{} {title}", follow.artist));
            let title_key = crate::artwork::normalize(title);
            let downloading = jobs.iter().any(|j| {
                crate::artwork::normalize(&j.title).contains(&title_key)
                    && matches!(
                        j.status,
                        delune_core::api::JobStatus::Queued
                            | delune_core::api::JobStatus::Downloading
                            | delune_core::api::JobStatus::Ready
                    )
            });
            found.push(RadarRelease {
                id,
                artist: follow.artist.clone(),
                title: title.to_owned(),
                kind: album.get("record_type").and_then(Value::as_str).unwrap_or("album").to_owned(),
                release_date: date.get(..10).unwrap_or(date).to_owned(),
                upcoming: released > today,
                cover: album
                    .get("cover_xl")
                    .or_else(|| album.get("cover_medium"))
                    .and_then(Value::as_str)
                    .map(crate::artwork::proxy),
                wished: wished.contains(&key),
                downloading,
                in_library: false,
            });
        }
    }
    let mut releases = found;
    releases.sort_by(|a, b| b.upcoming.cmp(&a.upcoming).then(b.release_date.cmp(&a.release_date)));
    releases.truncate(120);

    // Whether each is already in the library, a few at a time.
    let wanted: Vec<(String, String, bool)> =
        releases.iter().map(|r| (r.artist.clone(), r.title.clone(), r.upcoming)).collect();
    let checks = wanted.into_iter().map(|(artist, title, upcoming)| {
        let app = app.clone();
        async move {
            if upcoming {
                return false;
            }
            crate::library::lookup(&app, Some(&artist), &title, None).await.state == LibraryState::InLibrary
        }
    });
    let owned: Vec<bool> = futures_util::stream::iter(checks).buffered(6).collect().await;
    for (release, owned) in releases.iter_mut().zip(owned) {
        release.in_library = owned;
    }
    Json(releases)
}

/// `GET /api/v1/follows/albums`
#[utoipa::path(
    get,
    operation_id = "automation_album_follows",
    path = "/api/v1/follows/albums",
    tag = "automation",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::AlbumFollow>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn album_follows(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<AlbumFollow>> {
    Json(app.automation.lock().albums.iter().filter(|f| user.can_see(Some(&f.added_by))).cloned().collect())
}

/// `POST /api/v1/follows/albums`: keep an album complete, now and as it grows.
#[utoipa::path(
    post,
    operation_id = "automation_follow_album",
    path = "/api/v1/follows/albums",
    tag = "automation",
    request_body = delune_core::api::FollowAlbumRequest,
    responses(
        (status = 201, description = "Following", body = delune_core::api::AlbumFollow),
        (status = 200, description = "Already following", body = delune_core::api::AlbumFollow),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn follow_album(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(request): Json<delune_core::api::FollowAlbumRequest>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.search && p.download, "follow albums") {
        return denied;
    }
    match follow_album_for(&app, &user.username, Some(request.artist.trim()), request.album.trim()).await {
        Ok((follow, true)) => (StatusCode::CREATED, Json(follow)).into_response(),
        Ok((follow, false)) => Json(follow).into_response(),
        Err(message) => error(StatusCode::NOT_FOUND, "no-such-album", &message),
    }
}

/// Follow an album for `username`: find it, remember it, and put what's missing on the
/// wishlist now. Returns the follow and whether it's new.
pub(crate) async fn follow_album_for(
    app: &AppState,
    username: &str,
    artist: Option<&str>,
    album: &str,
) -> Result<(AlbumFollow, bool), String> {
    if album.is_empty() {
        return Err("Which album?".into());
    }
    let artist = artist.filter(|a| !a.is_empty());
    let Some((found, _, _)) = crate::music::find_album(app, artist, album).await else {
        return Err("Couldn't find that album to follow.".into());
    };
    let Some(id) = found.get("id").and_then(Value::as_u64) else {
        return Err("Couldn't find that album to follow.".into());
    };
    let text = |pointer: &str| found.pointer(pointer).and_then(Value::as_str).map(str::to_owned);
    let follow = {
        let mut state = app.automation.lock();
        if let Some(existing) = state.albums.iter().find(|f| f.id == id && f.added_by == username) {
            return Ok((existing.clone(), false));
        }
        let follow = AlbumFollow {
            id,
            artist: text("/artist/name").or_else(|| artist.map(str::to_owned)).unwrap_or_default(),
            title: text("/title").unwrap_or_else(|| album.to_owned()),
            cover: text("/cover_medium"),
            added_by: username.to_owned(),
            since: now(),
            last_checked: None,
            tracks: found.get("nb_tracks").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
            queued: Vec::new(),
        };
        state.albums.push(follow.clone());
        app.automation.save(&state);
        follow
    };
    crate::events::changed(app, crate::events::Topic::Follows);
    // What's missing today goes on the wishlist straight away.
    {
        let (app, follow) = (app.clone(), follow.clone());
        tokio::spawn(async move { check_album(&app, follow).await });
    }
    Ok((follow, true))
}

/// `DELETE /api/v1/follows/albums/{id}`
#[utoipa::path(
    delete,
    operation_id = "automation_unfollow_album",
    path = "/api/v1/follows/albums/{id}",
    tag = "automation",
    params(("id" = u64, Path, description = "The album's Deezer id")),
    responses(
        (status = 204, description = "Done"),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn unfollow_album(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<u64>) -> StatusCode {
    let mut state = app.automation.lock();
    state.albums.retain(|f| !(f.id == id && user.can_see(Some(&f.added_by))));
    app.automation.save(&state);
    drop(state);
    crate::events::changed(&app, crate::events::Topic::Follows);
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_followed_album_asks_for_each_missing_track_once() {
        use delune_library::merge::ListedTrack;
        let list: Vec<ListedTrack> = ["Lights Burn Dimmer", "solo", "solo (KETTAMA remix)", "Jungle"]
            .iter()
            .enumerate()
            .map(|(i, t)| ListedTrack { position: u32::try_from(i + 1).unwrap(), title: (*t).into() })
            .collect();
        let library = ["Solo", "Jungle (feat. Elley Duhé)"];
        let missing: Vec<&str> = missing_tracks(&list, &library, &[]).iter().map(|(t, _)| t.title.as_str()).collect();
        assert_eq!(missing, ["Lights Burn Dimmer", "solo (KETTAMA remix)"], "the remix isn't the song you have");

        let queued = vec!["lightsburndimmer".to_owned()];
        let missing: Vec<&str> =
            missing_tracks(&list, &library, &queued).iter().map(|(t, _)| t.title.as_str()).collect();
        assert_eq!(missing, ["solo (KETTAMA remix)"], "asked for once");
    }

    #[test]
    fn release_dates_become_timestamps() {
        assert_eq!(release_timestamp("1970-01-01"), Some(0));
        assert_eq!(release_timestamp("2026-09-15"), Some(1_789_430_400));
        assert_eq!(release_timestamp("not a date"), None);
    }

    #[test]
    fn spots_lossy_tracks() {
        assert!(!song_quality(Some("mp3"), Some(320)).unwrap().codec.is_lossless());
        assert!(song_quality(Some("flac"), None).unwrap().codec.is_lossless());
        assert!(song_quality(None, None).is_none());
    }

    #[test]
    fn settings_saved_with_the_old_follow_switch_still_load() {
        let stored: Stored = serde_json::from_str(
            r#"{"settings":{"follow_artists":true,"quality_upgrades":true,"upgrade_to":"lossless","auto_download":false}}"#,
        )
        .unwrap();
        assert!(stored.settings.quality_upgrades);
        assert!(!stored.settings.auto_download);
    }
}
