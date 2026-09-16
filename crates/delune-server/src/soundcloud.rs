//! SoundCloud: an artist's profile and newest tracks, following them so new
//! tracks go onto the wishlist, and the downloads artists choose to give away.
//!
//! Only public pages and the RSS feed SoundCloud publishes are read (see
//! `delune-soundcloud`). delune never pulls audio from SoundCloud itself: a free
//! download opens where the artist offers it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{
    ApiError, MinQuality, SoundcloudArtist, SoundcloudFollow, SoundcloudTrack, SoundcloudTrackDetail,
};
use delune_soundcloud::{Client, Error as SoundcloudError, FeedTrack, Profile};
use serde::Deserialize;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::artwork::{names_match, normalize};
use crate::events::{Topic, changed};
use crate::store::Database;

/// How long an artist lookup (found or not) is remembered.
const LOOKUP_FOR: Duration = Duration::from_secs(6 * 60 * 60);
/// How often followed artists' feeds are checked.
const CHECK_EVERY: Duration = Duration::from_secs(3 * 60 * 60);
/// Tracks shown on an artist's page.
const RECENT: usize = 8;
/// A guessed address must belong to an artist this well known, unless verified,
/// so a namesake's empty account isn't shown.
const MIN_FOLLOWERS: u64 = 500;

type Found = Option<(Profile, Vec<FeedTrack>)>;

#[derive(Debug)]
pub struct SoundCloud {
    store: Option<Arc<Database>>,
    client: Client,
    follows: Mutex<Vec<SoundcloudFollow>>,
    lookups: Mutex<HashMap<String, (Instant, Found)>>,
}

impl Default for SoundCloud {
    fn default() -> Self {
        Self::with_client(None, Client::default())
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn soundcloud_error(e: &SoundcloudError) -> Response {
    match e {
        SoundcloudError::NotFound => error(StatusCode::NOT_FOUND, "not-on-soundcloud", "That isn't on SoundCloud."),
        SoundcloudError::Http(_) | SoundcloudError::Unreadable => error(
            StatusCode::BAD_GATEWAY,
            "soundcloud-unreachable",
            "Couldn't reach SoundCloud just now. Try again soon.",
        ),
    }
}

fn track(t: FeedTrack) -> SoundcloudTrack {
    SoundcloudTrack {
        id: t.id,
        title: t.title,
        url: t.url,
        published_at: t.published_at,
        duration_secs: t.duration_secs,
        artwork: t.artwork,
    }
}

impl SoundCloud {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        Self::with_client(Some(db.clone()), Client::default())
    }

    /// For tests: talk to `client` (usually a local stand-in for SoundCloud).
    #[must_use]
    pub fn with_client(store: Option<Arc<Database>>, client: Client) -> Self {
        let follows = store.as_ref().and_then(|db| db.load("soundcloud-follows")).unwrap_or_default();
        Self { store, client, follows: Mutex::new(follows), lookups: Mutex::default() }
    }

    fn follows(&self) -> std::sync::MutexGuard<'_, Vec<SoundcloudFollow>> {
        self.follows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, follows: &[SoundcloudFollow]) {
        if let Some(db) = &self.store {
            db.save("soundcloud-follows", follows);
        }
    }

    fn remembered(&self, key: &str) -> Option<Found> {
        let lookups = self.lookups.lock().unwrap_or_else(PoisonError::into_inner);
        lookups.get(key).filter(|(at, _)| at.elapsed() < LOOKUP_FOR).map(|(_, found)| found.clone())
    }

    fn remember(&self, key: String, found: Found) {
        let mut lookups = self.lookups.lock().unwrap_or_else(PoisonError::into_inner);
        if lookups.len() > 2000 {
            lookups.retain(|_, (at, _)| at.elapsed() < LOOKUP_FOR);
        }
        lookups.insert(key, (Instant::now(), found));
    }

    async fn with_feed(&self, profile: Profile) -> Result<(Profile, Vec<FeedTrack>), SoundcloudError> {
        // An account with no public feed still has a profile worth showing.
        let feed = match self.client.feed(profile.id).await {
            Ok(feed) => feed,
            Err(SoundcloudError::NotFound) => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok((profile, feed))
    }

    /// The account at `soundcloud.com/{permalink}`.
    async fn by_permalink(&self, permalink: &str) -> Result<Found, SoundcloudError> {
        let key = format!("@{}", permalink.to_ascii_lowercase());
        if let Some(found) = self.remembered(&key) {
            return Ok(found);
        }
        let found = match self.client.profile(permalink).await {
            Ok(profile) => Some(self.with_feed(profile).await?),
            Err(SoundcloudError::NotFound) => None,
            Err(e) => return Err(e),
        };
        self.remember(key, found.clone());
        Ok(found)
    }

    /// An artist known only by name: try the addresses they'd likely have, and
    /// keep one only if it's really them.
    async fn by_name(&self, name: &str) -> Result<Found, SoundcloudError> {
        let key = normalize(name);
        if let Some(found) = self.remembered(&key) {
            return Ok(found);
        }
        let mut found = None;
        for guess in delune_soundcloud::permalink_guesses(name) {
            match self.client.profile(&guess).await {
                Ok(profile)
                    if names_match(&normalize(&profile.name), &key)
                        && (profile.verified || profile.followers >= MIN_FOLLOWERS) =>
                {
                    found = Some(self.with_feed(profile).await?);
                    break;
                }
                Ok(_) | Err(SoundcloudError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        self.remember(key, found.clone());
        Ok(found)
    }

    fn artist(&self, (profile, feed): (Profile, Vec<FeedTrack>)) -> SoundcloudArtist {
        let following = self.follows().iter().any(|f| f.id == profile.id);
        SoundcloudArtist {
            id: profile.id,
            name: profile.name,
            permalink: profile.permalink,
            url: profile.url,
            avatar: profile.avatar,
            followers: profile.followers,
            tracks: profile.tracks,
            verified: profile.verified,
            following,
            recent: feed.into_iter().take(RECENT).map(track).collect(),
        }
    }
}

/// Check followed artists' feeds for as long as the server runs. Call once at startup.
pub fn start(app: &AppState) {
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(150)).await;
        loop {
            check_follows(&app).await;
            tokio::time::sleep(Duration::from_secs(30 * 60)).await;
        }
    });
}

/// Wish for every track a followed artist has posted since the last look.
pub async fn check_follows(app: &AppState) {
    let due: Vec<SoundcloudFollow> = app
        .soundcloud
        .follows()
        .iter()
        .filter(|f| f.last_checked.is_none_or(|t| now().saturating_sub(t) >= CHECK_EVERY.as_secs()))
        .cloned()
        .collect();
    for follow in due {
        let Ok(feed) = app.soundcloud.client.feed(follow.id).await else { continue };
        let mut seen = follow.seen.clone();
        for item in feed {
            if seen.contains(&item.id) || item.published_at.is_some_and(|at| at < follow.since) {
                continue;
            }
            tracing::info!(artist = %follow.name, title = %item.title, "new track from a followed SoundCloud artist");
            // Tracks often carry the artist already ("Artist - Title"); don't say it twice.
            let query = if normalize(&item.title).contains(&normalize(&follow.name)) {
                item.title.clone()
            } else {
                format!("{} {}", follow.name, item.title)
            };
            crate::automation::queue(
                app,
                query,
                &follow.added_by,
                MinQuality::Any,
                app.automation.auto_download(),
                &format!("{} on SoundCloud", follow.name),
            );
            seen.push(item.id);
        }
        let mut follows = app.soundcloud.follows();
        if let Some(f) = follows.iter_mut().find(|f| f.id == follow.id) {
            f.seen = seen;
            f.last_checked = Some(now());
        }
        app.soundcloud.save(&follows);
        drop(follows);
        changed(app, Topic::Wishlist);
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ArtistParams {
    /// Their name, to find their account by.
    name: Option<String>,
    /// Or their address: the part after `soundcloud.com/`.
    permalink: Option<String>,
}

/// `GET /api/v1/soundcloud/artist?name=…` or `?permalink=…`
#[utoipa::path(
    get,
    operation_id = "soundcloud_artist",
    path = "/api/v1/soundcloud/artist",
    tag = "soundcloud",
    params(ArtistParams),
    responses(
        (status = 200, description = "Their SoundCloud", body = delune_core::api::SoundcloudArtist),
        (status = 404, description = "Not found there", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn artist(State(app): State<AppState>, _user: CurrentUser, Query(params): Query<ArtistParams>) -> Response {
    let found = match (params.permalink.as_deref().map(str::trim), params.name.as_deref().map(str::trim)) {
        (Some(permalink), _) if !permalink.is_empty() => app.soundcloud.by_permalink(permalink).await,
        (_, Some(name)) if !name.is_empty() => app.soundcloud.by_name(name).await,
        _ => return error(StatusCode::BAD_REQUEST, "no-artist", "Say which artist."),
    };
    match found {
        Ok(Some(found)) => Json(app.soundcloud.artist(found)).into_response(),
        Ok(None) => soundcloud_error(&SoundcloudError::NotFound),
        Err(e) => soundcloud_error(&e),
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TrackParams {
    /// `artist/track`, as in the track's address.
    path: String,
}

/// `GET /api/v1/soundcloud/track?path=artist/track`
#[utoipa::path(
    get,
    operation_id = "soundcloud_track",
    path = "/api/v1/soundcloud/track",
    tag = "soundcloud",
    params(TrackParams),
    responses(
        (status = 200, description = "The track", body = delune_core::api::SoundcloudTrackDetail),
        (status = 404, description = "No such track", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn track_detail(
    State(app): State<AppState>,
    _user: CurrentUser,
    Query(params): Query<TrackParams>,
) -> Response {
    let path = params.path.trim().trim_start_matches("https://soundcloud.com/");
    match app.soundcloud.client.track(path).await {
        Ok(t) => Json(SoundcloudTrackDetail {
            free_download: t.free_download_link.or_else(|| t.downloadable.then(|| t.url.clone())),
            title: t.title,
            artist: t.artist,
            url: t.url,
            artwork: t.artwork,
            duration_secs: t.duration_secs,
            album: t.album,
        })
        .into_response(),
        Err(e) => soundcloud_error(&e),
    }
}

/// `GET /api/v1/soundcloud/follows`
#[utoipa::path(
    get,
    operation_id = "soundcloud_follows",
    path = "/api/v1/soundcloud/follows",
    tag = "soundcloud",
    responses(
        (status = 200, description = "Followed SoundCloud artists", body = [delune_core::api::SoundcloudFollow]),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn follows(State(app): State<AppState>, _user: CurrentUser) -> Json<Vec<SoundcloudFollow>> {
    Json(app.soundcloud.follows().clone())
}

/// `PUT /api/v1/soundcloud/follows/{permalink}`: follow an artist from now on.
#[utoipa::path(
    put,
    operation_id = "soundcloud_follow",
    path = "/api/v1/soundcloud/follows/{artist}",
    tag = "soundcloud",
    params(("artist" = String, Path, description = "Their address, after soundcloud.com/")),
    responses(
        (status = 200, description = "Following", body = [delune_core::api::SoundcloudFollow]),
        (status = 404, description = "No such artist", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed to download", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn follow(State(app): State<AppState>, user: CurrentUser, UrlPath(permalink): UrlPath<String>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.download, "follow artists") {
        return denied;
    }
    let (profile, feed) = match app.soundcloud.by_permalink(permalink.trim()).await {
        Ok(Some(found)) => found,
        Ok(None) => return soundcloud_error(&SoundcloudError::NotFound),
        Err(e) => return soundcloud_error(&e),
    };
    {
        let mut follows = app.soundcloud.follows();
        if !follows.iter().any(|f| f.id == profile.id) {
            follows.push(SoundcloudFollow {
                id: profile.id,
                name: profile.name,
                permalink: profile.permalink,
                avatar: profile.avatar,
                added_by: user.username.clone(),
                since: now(),
                last_checked: Some(now()),
                // What's already out isn't new.
                seen: feed.iter().map(|t| t.id).collect(),
            });
            app.soundcloud.save(&follows);
        }
    }
    changed(&app, Topic::Soundcloud);
    Json(app.soundcloud.follows().clone()).into_response()
}

/// `DELETE /api/v1/soundcloud/follows/{artist}`
#[utoipa::path(
    delete,
    operation_id = "soundcloud_unfollow",
    path = "/api/v1/soundcloud/follows/{artist}",
    tag = "soundcloud",
    params(("artist" = String, Path, description = "Their SoundCloud id or address")),
    responses(
        (status = 200, description = "Unfollowed", body = [delune_core::api::SoundcloudFollow]),
        (status = 403, description = "Not allowed to download", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn unfollow(State(app): State<AppState>, user: CurrentUser, UrlPath(artist): UrlPath<String>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.download, "follow artists") {
        return denied;
    }
    {
        let mut follows = app.soundcloud.follows();
        follows.retain(|f| f.id.to_string() != artist && !f.permalink.eq_ignore_ascii_case(&artist));
        app.soundcloud.save(&follows);
    }
    changed(&app, Topic::Soundcloud);
    Json(app.soundcloud.follows().clone()).into_response()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    /// A stand-in for SoundCloud with one artist, whose feed grows by a track.
    async fn fake_soundcloud(newer: Arc<std::sync::atomic::AtomicBool>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let profile = r#"<script>window.__sc_hydration = [{"hydratable":"user","data":{"id":7,"username":"Fred again..","permalink":"fredagain","followers_count":719034,"track_count":2,"verified":true}}];</script>"#;
        let obscure = r#"<script>window.__sc_hydration = [{"hydratable":"user","data":{"id":8,"username":"Fred Again","permalink":"fred-again","followers_count":3,"verified":false}}];</script>"#;
        let track = r#"<script>window.__sc_hydration = [{"hydratable":"sound","data":{"id":1,"title":"Jungle","permalink_url":"https://soundcloud.com/fredagain/jungle","downloadable":true,"has_downloads_left":true,"user":{"username":"Fred again.."}}}];</script>"#;
        let router = axum::Router::new()
            .route("/fredagain", get(move || async move { axum::response::Html(profile) }))
            .route("/fred-again", get(move || async move { axum::response::Html(obscure) }))
            .route("/fredagain/jungle", get(move || async move { axum::response::Html(track) }))
            .route(
                "/users/{user}/sounds.rss",
                get(move || {
                    let newer = newer.load(std::sync::atomic::Ordering::SeqCst);
                    async move {
                        let item = |id: u32, title: &str| {
                            format!("<item><guid>tag:soundcloud,2010:tracks/{id}</guid><title>{title}</title><link>https://soundcloud.com/fredagain/{id}</link></item>")
                        };
                        let mut items = item(1, "Jungle");
                        if newer {
                            items = item(2, "Adore u") + &items;
                        }
                        format!("<rss><channel>{items}</channel></rss>")
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        base
    }

    async fn call(app: &axum::Router, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
        let request = Request::builder().method(method).uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    #[tokio::test]
    async fn finds_artists_follows_them_and_wishes_for_new_tracks() {
        let newer = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let base = fake_soundcloud(newer.clone()).await;
        let db = Arc::new(Database::in_memory());
        let state = AppState {
            soundcloud: Arc::new(SoundCloud::with_client(Some(db.clone()), Client::new(&base, &base))),
            db,
            ..AppState::default()
        };
        let app = crate::router(state.clone());

        let (status, artist) = call(&app, "GET", "/api/v1/soundcloud/artist?name=Fred%20again..").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!((artist["permalink"].as_str(), artist["following"].as_bool()), (Some("fredagain"), Some(false)));
        assert_eq!(artist["recent"][0]["title"], "Jungle");
        let (status, _) = call(&app, "GET", "/api/v1/soundcloud/artist?name=Nobody%20At%20All").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (_, detail) = call(&app, "GET", "/api/v1/soundcloud/track?path=fredagain/jungle").await;
        assert_eq!(detail["free_download"], "https://soundcloud.com/fredagain/jungle", "the artist's download button");

        let (status, follows) = call(&app, "PUT", "/api/v1/soundcloud/follows/fredagain").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(follows[0]["seen"], serde_json::json!([1]), "what's already out isn't wished for");

        // A new track appears; the next check puts it on the wishlist, once.
        newer.store(true, std::sync::atomic::Ordering::SeqCst);
        state.soundcloud.follows()[0].last_checked = None;
        check_follows(&state).await;
        state.soundcloud.follows()[0].last_checked = None;
        check_follows(&state).await;
        let wished: Vec<String> = state.wishlist.with_items(|items| items.iter().map(|i| i.query.clone()).collect());
        assert_eq!(wished, ["Fred again.. Adore u"]);

        let id = follows[0]["id"].as_u64().unwrap();
        let (_, follows) = call(&app, "DELETE", &format!("/api/v1/soundcloud/follows/{id}")).await;
        assert!(follows.as_array().unwrap().is_empty());
    }
}
