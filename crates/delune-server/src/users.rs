//! Other Soulseek users: who they are and what they share.
//!
//! Browsing someone fetches their whole share list once (it can be hundreds of
//! thousands of files) and keeps it for ten minutes. Clients get the folder list
//! first, then one folder at a time as a [`Candidate`], which is exactly what search
//! results are made of, so a browsed folder opens and downloads like any result.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Path as UrlPath, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use delune_core::Quality;
use delune_core::api::{ApiError, Candidate, Presence, ShareFolder, ShareTree, SoulseekProfile, SoulseekUser};
use delune_soulseek::peer::SearchResponse;
use delune_soulseek::{PeerError, SharedFileList, UserInfo, UserStatus};
use serde::Deserialize;

use crate::AppState;
use crate::accounts::CurrentUser;

const KEEP_FOR: Duration = Duration::from_secs(10 * 60);
const MAX_CACHED_USERS: usize = 32;

#[derive(Debug, Default)]
pub struct BrowseCache {
    shares: Mutex<HashMap<String, (Instant, Arc<SharedFileList>)>>,
    profiles: Mutex<HashMap<String, (Instant, UserInfo)>>,
    /// Upload speeds from the server, for folders opened from someone's shares.
    speeds: Mutex<HashMap<String, (Instant, u32)>>,
    /// Lists above that came from a favourite's saved copy, and when it was fetched.
    saved_at: Mutex<HashMap<String, u64>>,
}

fn fresh<V: Clone>(map: &Mutex<HashMap<String, (Instant, V)>>, key: &str) -> Option<V> {
    let map = map.lock().unwrap_or_else(PoisonError::into_inner);
    map.get(key).filter(|(at, _)| at.elapsed() < KEEP_FOR).map(|(_, v)| v.clone())
}

fn remember<V>(map: &Mutex<HashMap<String, (Instant, V)>>, key: &str, value: V) {
    let mut map = map.lock().unwrap_or_else(PoisonError::into_inner);
    if map.len() >= MAX_CACHED_USERS {
        // Share lists are big; forget the oldest rather than growing.
        if let Some(oldest) = map.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    map.insert(key.to_owned(), (Instant::now(), value));
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn peer_error(e: &PeerError) -> Response {
    let status = match e {
        PeerError::Offline => StatusCode::SERVICE_UNAVAILABLE,
        PeerError::NoSuchUser(_) => StatusCode::NOT_FOUND,
        PeerError::Unreachable(_) | PeerError::TimedOut(_) => StatusCode::GATEWAY_TIMEOUT,
    };
    let mut message = e.to_string();
    if let Some(first) = message.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    error(status, "soulseek-peer", &format!("{message}."))
}

fn guard(app: &AppState, user: &CurrentUser) -> Result<delune_soulseek::Client, Box<Response>> {
    if let Some(denied) = user.refuse_unless(|p| p.search, "browse Soulseek users") {
        return Err(Box::new(denied));
    }
    app.soulseek.clone().ok_or_else(|| {
        Box::new(error(StatusCode::SERVICE_UNAVAILABLE, "soulseek-not-configured", "Soulseek isn't set up."))
    })
}

async fn profile(app: &AppState, client: &delune_soulseek::Client, username: &str) -> Result<UserInfo, PeerError> {
    if let Some(info) = fresh(&app.browse.profiles, username) {
        return Ok(info);
    }
    let info = client.user_info(username).await?;
    remember(&app.browse.profiles, username, info.clone());
    Ok(info)
}

/// `GET /api/v1/soulseek/users/{username}`
#[utoipa::path(
    get,
    operation_id = "users_user",
    path = "/api/v1/soulseek/users/{username}",
    tag = "soulseek",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::SoulseekUser),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn user(State(app): State<AppState>, user: CurrentUser, UrlPath(username): UrlPath<String>) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let presence = match client.user_presence(&username).await {
        Ok(presence) => presence,
        Err(e) => return peer_error(&e),
    };
    let reachable = presence.exists && presence.status != UserStatus::Offline;
    remember(&app.browse.speeds, &username, presence.avg_speed);
    // Only ask the user themselves if they might answer.
    let info = if reachable { profile(&app, &client, &username).await.ok() } else { None };
    Json(SoulseekUser {
        username: presence.username,
        exists: presence.exists,
        presence: match presence.status {
            UserStatus::Online => Presence::Online,
            UserStatus::Away => Presence::Away,
            UserStatus::Offline => Presence::Offline,
        },
        avg_speed: presence.avg_speed,
        files: presence.files,
        folders: presence.folders,
        country: presence.country,
        profile: info.map(|i| SoulseekProfile {
            description: i.description,
            has_picture: i.picture.is_some(),
            queue_size: i.queue_size,
            slots_free: i.slots_free,
            total_uploads: i.total_uploads,
        }),
    })
    .into_response()
}

/// `GET /api/v1/soulseek/users/{username}/picture`
#[utoipa::path(
    get,
    operation_id = "users_picture",
    path = "/api/v1/soulseek/users/{username}/picture",
    tag = "soulseek",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 200, description = "Their picture", content_type = "image/*"),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn picture(State(app): State<AppState>, user: CurrentUser, UrlPath(username): UrlPath<String>) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let Ok(info) = profile(&app, &client, &username).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(bytes) = info.picture else { return StatusCode::NOT_FOUND.into_response() };
    // Pictures are whatever people uploaded; only pass through recognisable images.
    let kind = match bytes.get(..4) {
        Some([0x89, b'P', b'N', b'G']) => "image/png",
        Some([0xFF, 0xD8, 0xFF, _]) => "image/jpeg",
        Some([b'G', b'I', b'F', b'8']) => "image/gif",
        Some([b'R', b'I', b'F', b'F']) => "image/webp",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "private, max-age=600"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

/// Keep a freshly fetched share list for the next ten minutes.
pub fn remember_shares(app: &AppState, username: &str, list: Arc<SharedFileList>) {
    app.browse.saved_at.lock().unwrap_or_else(PoisonError::into_inner).remove(username);
    remember(&app.browse.shares, username, list);
}

/// The share list browsed in the last ten minutes, if there is one.
#[must_use]
pub fn remembered_shares(app: &AppState, username: &str) -> Option<Arc<SharedFileList>> {
    fresh(&app.browse.shares, username)
}

/// When the list [`shares`] would give for `username` is a favourite's saved copy.
fn saved_at(app: &AppState, username: &str) -> Option<u64> {
    app.browse.saved_at.lock().unwrap_or_else(PoisonError::into_inner).get(username).copied()
}

async fn shares(
    app: &AppState,
    client: &delune_soulseek::Client,
    username: &str,
) -> Result<Arc<SharedFileList>, PeerError> {
    if let Some(list) = fresh(&app.browse.shares, username) {
        return Ok(list);
    }
    // A favourite: answer from the saved copy at once, and fetch a fresh one behind it.
    if let Some((list, at)) = app.favourites.saved(&app.db, username) {
        let list = Arc::new(list);
        remember(&app.browse.shares, username, list.clone());
        let age = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
            .saturating_sub(at);
        if age >= crate::favourites::STALE_AFTER.as_secs() {
            app.browse.saved_at.lock().unwrap_or_else(PoisonError::into_inner).insert(username.to_owned(), at);
            crate::favourites::refresh(app, username);
        }
        return Ok(list);
    }
    let list = client.browse(username).await?;
    remember_shares(app, username, list.clone());
    app.favourites.fetched(&app.db, username, &list);
    Ok(list)
}

/// `GET /api/v1/soulseek/users/{username}/shares`
#[utoipa::path(
    get,
    operation_id = "users_share_tree",
    path = "/api/v1/soulseek/users/{username}/shares",
    tag = "soulseek",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::ShareTree),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn share_tree(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(username): UrlPath<String>,
) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let list = match shares(&app, &client, &username).await {
        Ok(list) => list,
        Err(e) => return peer_error(&e),
    };
    let folders = list
        .directories
        .iter()
        .map(|d| {
            let qualities: Vec<Quality> =
                d.files.iter().filter_map(delune_soulseek::peer::SharedFile::quality).collect();
            let worst = qualities.iter().copied().min_by_key(Quality::rank);
            ShareFolder {
                path: d.path.clone(),
                files: u32::try_from(d.files.len()).unwrap_or(u32::MAX),
                audio_files: u32::try_from(qualities.len()).unwrap_or(u32::MAX),
                bytes: d.files.iter().map(|f| f.size).sum(),
                quality_label: worst.map(|q| q.to_string()),
                quality_rank: worst.map_or(0, |q| q.rank()),
            }
        })
        .collect();
    let saved_at = saved_at(&app, &username);
    Json(ShareTree {
        username,
        folders,
        private_folders: u32::try_from(list.private_directories.len()).unwrap_or(u32::MAX),
        saved_at,
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct FolderParams {
    path: String,
}

/// `GET /api/v1/soulseek/users/{username}/folder?path=…`: one folder, ready to open and download.
#[utoipa::path(
    get,
    operation_id = "users_folder",
    path = "/api/v1/soulseek/users/{username}/folder",
    tag = "soulseek",
    params(
        ("username" = String, Path),
        ("path" = String, Query, description = "The folder, as they share it"),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::Candidate),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn folder(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(username): UrlPath<String>,
    Query(params): Query<FolderParams>,
) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let list = match shares(&app, &client, &username).await {
        Ok(list) => list,
        Err(e) => return peer_error(&e),
    };
    let Some(directory) = list.directories.iter().find(|d| d.path == params.path) else {
        return error(StatusCode::NOT_FOUND, "no-such-folder", "They don't share that folder.");
    };
    let info = fresh(&app.browse.profiles, &username);
    let response = SearchResponse {
        username: username.clone(),
        token: 0,
        files: directory.files.clone(),
        free_slot: info.as_ref().is_none_or(|i| i.slots_free),
        avg_speed: fresh(&app.browse.speeds, &username).unwrap_or(0),
        queue_length: info.as_ref().map_or(0, |i| i.queue_size),
        private_files: vec![],
    };
    let candidates: Vec<Candidate> = crate::search::candidates(&response);
    match candidates.into_iter().next() {
        Some(mut candidate) => {
            candidate.peer = app.db.peers(&[username.as_str()]).remove(&username);
            Json(candidate).into_response()
        }
        None => error(StatusCode::NOT_FOUND, "no-audio", "That folder has no music in it."),
    }
}
