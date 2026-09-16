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

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, AutomationSettings, Follow, MinQuality, WishlistItem};
use delune_core::{Codec, Quality};
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
        if items.iter().any(|i| i.query.eq_ignore_ascii_case(&query) && i.added_by == added_by)
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

#[cfg(test)]
mod tests {
    use super::*;

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
