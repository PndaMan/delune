//! Favourite Soulseek users: people someone starred, whose shares delune keeps.
//!
//! Browsing someone means fetching their whole share list, which can take a while
//! from a slow peer. For favourites, delune saves that list in its database and
//! shows the saved copy straight away, then fetches a fresh one behind it. Saved
//! lists are also refreshed every few hours, so they're rarely far behind.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, FavouriteUser};
use delune_soulseek::SharedFileList;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::events::{Topic, changed};
use crate::store::Database;

/// How old a saved list may be before opening it fetches a fresh one.
pub const STALE_AFTER: Duration = Duration::from_secs(10 * 60);
/// How often every favourite's list is refreshed in the background.
const REFRESH_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// People starred per account.
const MAX_PER_PERSON: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Starred {
    username: String,
    since: u64,
}

#[derive(Debug, Default)]
pub struct Favourites {
    store: Option<Arc<Database>>,
    /// Starred Soulseek users, by delune account.
    starred: Mutex<BTreeMap<String, Vec<Starred>>>,
    /// Folder and file counts of saved lists, so listing favourites doesn't decode them.
    sizes: Mutex<BTreeMap<String, (u32, u32)>>,
    /// Lists being fetched right now, so two requests don't fetch the same one.
    fetching: Mutex<HashSet<String>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Favourites {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let starred = db.load("favourites").unwrap_or_default();
        let sizes = db.load("favourite-sizes").unwrap_or_default();
        Self { store: Some(db.clone()), starred: Mutex::new(starred), sizes: Mutex::new(sizes), ..Self::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<Starred>>> {
        self.starred.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, starred: &BTreeMap<String, Vec<Starred>>) {
        if let Some(db) = &self.store {
            db.save("favourites", starred);
        }
    }

    /// Whether anyone has starred `username`.
    #[must_use]
    pub fn anyone_starred(&self, username: &str) -> bool {
        self.lock().values().flatten().any(|s| s.username == username)
    }

    /// Everyone starred by anyone, once each.
    fn everyone(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.lock().values().flatten().filter(|s| seen.insert(s.username.clone())).map(|s| s.username.clone()).collect()
    }

    /// Star `username` for `account`. False when they already were, or the list is full.
    fn star(&self, account: &str, username: &str) -> bool {
        let mut starred = self.lock();
        let mine = starred.entry(account.to_owned()).or_default();
        if mine.len() >= MAX_PER_PERSON || mine.iter().any(|s| s.username == username) {
            return false;
        }
        mine.insert(0, Starred { username: username.to_owned(), since: now() });
        self.save(&starred);
        true
    }

    /// Unstar `username` for `account`. Returns whether nobody has them starred now.
    fn unstar(&self, account: &str, username: &str) -> bool {
        let mut starred = self.lock();
        if let Some(mine) = starred.get_mut(account) {
            mine.retain(|s| s.username != username);
        }
        self.save(&starred);
        !starred.values().flatten().any(|s| s.username == username)
    }

    fn list(&self, account: &str, db: &Database) -> Vec<FavouriteUser> {
        let mine = self.lock().get(account).cloned().unwrap_or_default();
        let sizes = self.sizes.lock().unwrap_or_else(PoisonError::into_inner);
        mine.into_iter()
            .map(|s| {
                let saved_at = db.share_list_saved_at(&s.username);
                let (folders, files) = sizes.get(&s.username).copied().unwrap_or_default();
                FavouriteUser { username: s.username, since: s.since, saved_at, folders, files }
            })
            .collect()
    }

    fn note_size(&self, username: &str, list: &SharedFileList) {
        let mut sizes = self.sizes.lock().unwrap_or_else(PoisonError::into_inner);
        sizes.insert(username.to_owned(), (count(list.directories.len()), count(list.file_count())));
        if let Some(db) = &self.store {
            db.save("favourite-sizes", &*sizes);
        }
    }

    fn forget_size(&self, username: &str) {
        let mut sizes = self.sizes.lock().unwrap_or_else(PoisonError::into_inner);
        sizes.remove(username);
        if let Some(db) = &self.store {
            db.save("favourite-sizes", &*sizes);
        }
    }

    /// A freshly fetched list for `username`: keep it if they're someone's favourite.
    pub fn fetched(&self, db: &Database, username: &str, list: &SharedFileList) {
        if self.anyone_starred(username) {
            db.save_share_list(username, &list.compressed());
            self.note_size(username, list);
        }
    }

    /// The saved list for `username`, if they're a favourite and one was kept.
    #[must_use]
    pub fn saved(&self, db: &Database, username: &str) -> Option<(SharedFileList, u64)> {
        if !self.anyone_starred(username) {
            return None;
        }
        let (bytes, at) = db.share_list(username)?;
        match SharedFileList::decode(&bytes) {
            Ok(list) => Some((list, at)),
            Err(error) => {
                tracing::warn!(%username, %error, "a saved share list no longer reads; fetching afresh");
                db.forget_share_list(username);
                None
            }
        }
    }
}

/// Fetch a fresh list for `username` in the background, unless one is on its way.
/// Clients hear about it on the `favourites` topic when it lands.
pub fn refresh(app: &AppState, username: &str) {
    let Some(client) = app.soulseek.clone() else { return };
    let key = username.to_owned();
    if !app.favourites.fetching.lock().unwrap_or_else(PoisonError::into_inner).insert(key.clone()) {
        return;
    }
    let app = app.clone();
    let username = username.to_owned();
    tokio::spawn(async move {
        match client.browse(&username).await {
            Ok(list) => {
                crate::users::remember_shares(&app, &username, list.clone());
                app.favourites.fetched(&app.db, &username, &list);
                changed(&app, Topic::Favourites);
            }
            Err(error) => tracing::debug!(%username, %error, "couldn't refresh a favourite's shares"),
        }
        app.favourites.fetching.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
    });
}

/// Refresh every favourite's saved list now and then. Call once at startup.
pub fn start(app: &AppState) {
    if app.soulseek.is_none() {
        return;
    }
    let app = app.clone();
    tokio::spawn(async move {
        // Let the Soulseek connection settle first.
        tokio::time::sleep(Duration::from_secs(90)).await;
        loop {
            for username in app.favourites.everyone() {
                let stale = app.db.share_list_saved_at(&username).is_none_or(|at| {
                    now().saturating_sub(at) >= REFRESH_EVERY.as_secs().saturating_sub(STALE_AFTER.as_secs())
                });
                if stale {
                    refresh(&app, &username);
                    // One peer at a time, gently.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
            tokio::time::sleep(REFRESH_EVERY).await;
        }
    });
}

fn refuse(user: &CurrentUser) -> Option<Response> {
    user.refuse_unless(|p| p.search, "keep favourite Soulseek users")
}

/// `GET /api/v1/soulseek/favourites`
#[utoipa::path(
    get,
    operation_id = "favourites_list",
    path = "/api/v1/soulseek/favourites",
    tag = "soulseek",
    responses(
        (status = 200, description = "Your favourite Soulseek users, newest first", body = [delune_core::api::FavouriteUser]),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
    ),
)]
pub async fn list(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = refuse(&user) {
        return denied;
    }
    Json(app.favourites.list(&user.username, &app.db)).into_response()
}

/// `PUT /api/v1/soulseek/favourites/{username}`
#[utoipa::path(
    put,
    operation_id = "favourites_add",
    path = "/api/v1/soulseek/favourites/{username}",
    tag = "soulseek",
    params(("username" = String, Path, description = "Their Soulseek username")),
    responses(
        (status = 200, description = "Starred; their shares are being saved", body = [delune_core::api::FavouriteUser]),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 422, description = "Too many favourites", body = delune_core::api::ApiError),
    ),
)]
pub async fn add(State(app): State<AppState>, user: CurrentUser, UrlPath(username): UrlPath<String>) -> Response {
    if let Some(denied) = refuse(&user) {
        return denied;
    }
    let username = username.trim();
    if username.is_empty() || username.len() > 128 {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(ApiError::new("bad-username", "That isn't a username.")))
            .into_response();
    }
    if !app.favourites.star(&user.username, username) {
        let already = app.favourites.list(&user.username, &app.db).iter().any(|f| f.username == username);
        if !already {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiError::new("too-many-favourites", "That's as many favourites as delune keeps.")),
            )
                .into_response();
        }
    }
    // Keep whatever was just browsed, and fetch it if nothing was.
    match crate::users::remembered_shares(&app, username) {
        Some(list) => app.favourites.fetched(&app.db, username, &list),
        None => refresh(&app, username),
    }
    changed(&app, Topic::Favourites);
    Json(app.favourites.list(&user.username, &app.db)).into_response()
}

/// `DELETE /api/v1/soulseek/favourites/{username}`
#[utoipa::path(
    delete,
    operation_id = "favourites_remove",
    path = "/api/v1/soulseek/favourites/{username}",
    tag = "soulseek",
    params(("username" = String, Path, description = "Their Soulseek username")),
    responses(
        (status = 200, description = "Unstarred", body = [delune_core::api::FavouriteUser]),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
    ),
)]
pub async fn remove(State(app): State<AppState>, user: CurrentUser, UrlPath(username): UrlPath<String>) -> Response {
    if let Some(denied) = refuse(&user) {
        return denied;
    }
    let username = username.trim();
    if app.favourites.unstar(&user.username, username) {
        app.db.forget_share_list(username);
        app.favourites.forget_size(username);
    }
    changed(&app, Topic::Favourites);
    Json(app.favourites.list(&user.username, &app.db)).into_response()
}

#[cfg(test)]
mod tests {
    use delune_soulseek::SharedDirectory;

    use super::*;

    fn sample() -> SharedFileList {
        SharedFileList {
            directories: vec![SharedDirectory { path: "Music\\Boards of Canada\\Twoism".into(), files: vec![] }],
            private_directories: vec![],
        }
    }

    #[test]
    fn starring_keeps_lists_only_for_favourites() {
        let db = Arc::new(Database::in_memory());
        let favourites = Favourites::open(&db);

        favourites.fetched(&db, "stranger", &sample());
        assert!(db.share_list("stranger").is_none(), "nobody starred them");

        assert!(favourites.star("aidan", "Kindred"));
        assert!(!favourites.star("aidan", "Kindred"), "starring twice does nothing");
        assert!(favourites.star("aidan", "KINDRED"), "Soulseek names are case-sensitive");
        assert!(favourites.star("sam", "Kindred"));
        favourites.fetched(&db, "Kindred", &sample());

        let (list, _) = favourites.saved(&db, "Kindred").unwrap();
        assert_eq!(list, sample());
        assert!(favourites.saved(&db, "KINDRED").is_none(), "another person's name, nothing saved");
        let mine = favourites.list("aidan", &db);
        assert_eq!(mine.len(), 2);
        let kindred = mine.iter().find(|f| f.username == "Kindred").unwrap();
        assert_eq!((kindred.folders, kindred.files), (1, 0));
        assert!(kindred.saved_at.is_some());
        assert!(favourites.list("nobody", &db).is_empty());

        assert!(!favourites.unstar("aidan", "Kindred"), "sam still has them");
        assert!(favourites.unstar("sam", "Kindred"));
        assert!(favourites.saved(&db, "Kindred").is_none());
    }

    #[tokio::test]
    async fn favourites_api_stars_lists_and_unstars() {
        use axum::body::Body;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let db = Arc::new(Database::in_memory());
        let app = crate::router(AppState {
            favourites: Arc::new(Favourites::open(&db)),
            db: db.clone(),
            ..AppState::default()
        });
        let call = |method: &str, uri: &str| {
            let request = Request::builder().method(method).uri(uri).body(Body::empty()).unwrap();
            let app = app.clone();
            async move {
                let response = app.oneshot(request).await.unwrap();
                let status = response.status();
                let body = response.into_body().collect().await.unwrap().to_bytes();
                (status, serde_json::from_slice::<serde_json::Value>(&body).unwrap())
            }
        };

        let (status, listed) = call("PUT", "/api/v1/soulseek/favourites/kindred").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed[0]["username"], "kindred");
        let (_, again) = call("PUT", "/api/v1/soulseek/favourites/kindred").await;
        assert_eq!(again.as_array().unwrap().len(), 1, "starring twice keeps one");
        let (status, _) = call("PUT", "/api/v1/soulseek/favourites/%20").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        let (_, listed) = call("GET", "/api/v1/soulseek/favourites").await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        let (_, listed) = call("DELETE", "/api/v1/soulseek/favourites/kindred").await;
        assert!(listed.as_array().unwrap().is_empty());
    }

    #[test]
    fn favourites_survive_a_restart() {
        let db = Arc::new(Database::in_memory());
        Favourites::open(&db).star("aidan", "kindred");
        let reopened = Favourites::open(&db);
        assert!(reopened.anyone_starred("kindred"));
        assert!(!reopened.anyone_starred("KINDRED"));
        assert_eq!(reopened.everyone(), ["kindred"]);
    }
}
