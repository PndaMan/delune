//! # delune-server
//!
//! The HTTP API every client talks to. The web UI is served from the same origin,
//! and the TUI connects over the same routes, so there is exactly one way to drive
//! delune.
//!
//! Routes are versioned under `/api/v1`. Long-running work (searches, downloads,
//! scans) streams progress over Server-Sent Events rather than being polled.

pub mod accounts;
pub mod alerts;
pub mod artwork;
pub mod automation;
pub mod bandcamp;
pub mod chat;
pub mod diagnostics;
pub mod downloads;
pub mod events;
pub mod external;
pub mod favourites;
pub mod finishing;
pub mod library;
pub mod music;
pub mod naming;
pub mod nat;
pub mod notifications;
pub mod openapi;
pub mod requests;
pub mod review;
pub mod search;
pub mod setup;
pub mod sharing;
pub mod soundcloud;
pub mod stats;
pub mod store;
pub mod users;
mod web;
pub mod webpush;
pub mod wishlist;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    middleware,
    routing::{delete, get, patch, post, put},
};
use delune_core::{
    Provider, ProviderRole, SourcePolicy,
    api::{Health, HealthStatus, SoulseekState, SoulseekStatus},
};
use delune_resolve::{EntityKind, Parsed, classify};
use delune_soulseek::{SessionState, StopReason};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

/// Everything needed to start the server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Soulseek account. Without one, search is unavailable but the server still runs.
    pub soulseek: Option<delune_soulseek::Config>,
    /// Where delune keeps its own files: staged downloads and, later, its database.
    pub data_dir: PathBuf,
    pub library: review::LibrarySettings,
    /// Navidrome to rescan after imports.
    pub navidrome: Option<(String, delune_navidrome::Credentials)>,
    /// Connections set by flags or environment variables rather than the config file.
    pub locked: setup::Locked,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            soulseek: None,
            data_dir: PathBuf::from("delune-data"),
            library: review::LibrarySettings::default(),
            navidrome: None,
            locked: setup::Locked::default(),
        }
    }
}

/// Shared state handed to every request handler.
#[derive(Debug, Clone)]
pub struct AppState {
    pub soulseek: Option<delune_soulseek::Client>,
    pub soulseek_username: Option<String>,
    pub search_timeout: Duration,
    pub artwork: Arc<artwork::ArtworkService>,
    pub downloads: Arc<downloads::Downloads>,
    pub data_dir: PathBuf,
    pub library: Arc<review::LibrarySettings>,
    pub navidrome: Option<delune_navidrome::Client>,
    pub library_cache: Arc<library::LibraryCache>,
    pub resolver: Arc<delune_resolve::Resolver>,
    pub accounts: Arc<accounts::Accounts>,
    pub browse: Arc<users::BrowseCache>,
    /// Soulseek users people starred, whose shares are kept.
    pub favourites: Arc<favourites::Favourites>,
    /// Bandcamp: albums to buy, and each person's linked account.
    pub bandcamp: Arc<bandcamp::Bandcamp>,
    /// SoundCloud artists, and the ones people follow.
    pub soundcloud: Arc<soundcloud::SoundCloud>,
    pub chat: Arc<chat::Chat>,
    pub sharing: Arc<sharing::Sharing>,
    pub wishlist: Arc<wishlist::Wishlist>,
    pub totals: Arc<sharing::Totals>,
    pub automation: Arc<automation::Automation>,
    pub finishing: Arc<finishing::Finishing>,
    /// The naming template and options imports use now.
    pub naming: Arc<naming::Naming>,
    /// Navidrome's address and account, for showing in settings.
    pub navidrome_account: Option<(String, String)>,
    pub soulseek_port: Option<u16>,
    pub locked: setup::Locked,
    pub notifications: Arc<notifications::Notifier>,
    /// Where notifications go besides delune: push, ntfy, Discord.
    pub alerts: Arc<alerts::Alerts>,
    pub requests: Arc<requests::Requests>,
    /// Where everything above is saved.
    pub db: Arc<store::Database>,
    /// A program people who manage delune may point it at for other sources.
    pub external: Arc<external::External>,
    /// For looking things up about artists, albums and songs.
    pub music_http: reqwest::Client,
    /// What changed, for clients watching `/api/v1/events`.
    pub changes: Arc<events::Changes>,
    pub nat: Arc<nat::Nat>,
    /// Nudged when the port-mapping setting changes.
    pub nat_wake: Arc<tokio::sync::watch::Sender<u64>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            soulseek: None,
            soulseek_username: None,
            search_timeout: Duration::from_secs(20),
            artwork: artwork::service(),
            downloads: Arc::default(),
            data_dir: PathBuf::from("delune-data"),
            library: Arc::default(),
            navidrome: None,
            library_cache: Arc::default(),
            resolver: Arc::default(),
            accounts: Arc::new(accounts::Accounts::in_memory(None)),
            browse: Arc::default(),
            favourites: Arc::default(),
            bandcamp: Arc::default(),
            soundcloud: Arc::default(),
            chat: Arc::default(),
            sharing: Arc::default(),
            wishlist: Arc::default(),
            totals: Arc::default(),
            automation: Arc::default(),
            finishing: Arc::default(),
            naming: Arc::default(),
            navidrome_account: None,
            soulseek_port: None,
            locked: setup::Locked::default(),
            notifications: Arc::default(),
            alerts: Arc::default(),
            requests: Arc::default(),
            db: Arc::default(),
            external: Arc::default(),
            changes: Arc::default(),
            music_http: music::http_client(),
            nat: Arc::default(),
            nat_wake: Arc::new(tokio::sync::watch::channel(0).0),
        }
    }
}

impl AppState {
    /// Start background services described by `config`. Needs a Tokio runtime.
    ///
    /// # Errors
    ///
    /// When delune's database can't be opened.
    pub fn start(config: ServerConfig) -> std::io::Result<Self> {
        diagnostics::mark_start();
        let db = Arc::new(store::Database::open(&config.data_dir).map_err(|e| {
            std::io::Error::other(format!("couldn't open the database in {}: {e}", config.data_dir.display()))
        })?);
        let navidrome_url = config.navidrome.as_ref().map(|(url, _)| url.clone());
        let navidrome_account = config.navidrome.as_ref().map(|(url, c)| (url.clone(), c.username.clone()));
        let accounts = Arc::new(accounts::Accounts::open(&db, &config.data_dir, navidrome_url));
        let chat = Arc::new(chat::Chat::open(&db));
        let sharing = Arc::new(sharing::Sharing::open(&db, &config.data_dir));
        let wishlist = Arc::new(wishlist::Wishlist::open(&db));
        let totals = Arc::new(sharing::Totals::open(&db));
        let automation = Arc::new(automation::Automation::open(&db));
        let finishing = Arc::new(finishing::Finishing::open(&db));
        let naming = Arc::new(naming::Naming::open(&db, &config.library));
        let notifications = Arc::new(notifications::Notifier::open(&db));
        let alerts = Arc::new(alerts::Alerts::open(&db));
        let requests = Arc::new(requests::Requests::open(&db));
        let external = Arc::new(external::External::open(&db));
        let favourites = Arc::new(favourites::Favourites::open(&db));
        let bandcamp = Arc::new(bandcamp::Bandcamp::open(&db));
        let soundcloud = Arc::new(soundcloud::SoundCloud::open(&db));
        let navidrome =
            config.navidrome.and_then(|(url, credentials)| match delune_navidrome::Client::new(&url, credentials) {
                Ok(client) => Some(client),
                Err(error) => {
                    tracing::warn!(%error, "ignoring Navidrome settings");
                    None
                }
            });
        let downloads = Arc::new(downloads::Downloads::open(&db));
        downloads.set_slots(sharing.settings().downloads_at_once);
        let mut state = Self {
            downloads: downloads.clone(),
            data_dir: config.data_dir,
            library: Arc::new(config.library),
            navidrome,
            accounts,
            chat,
            sharing,
            wishlist,
            totals,
            automation,
            finishing,
            naming,
            navidrome_account,
            locked: config.locked,
            notifications,
            alerts,
            requests,
            external,
            favourites,
            bandcamp,
            soundcloud,
            db,
            ..Self::default()
        };
        if let Some(slsk) = config.soulseek {
            state.soulseek_username = Some(slsk.username.clone());
            state.soulseek_port = slsk.listen_port;
            state.search_timeout = slsk.search_timeout;
            state.soulseek = Some(delune_soulseek::Client::start(slsk));
        }
        {
            // Saving is also the moment clients hear that something about a download
            // moved (progress, a review finishing), whatever changed it.
            let app = state.clone();
            tokio::spawn(async move {
                let mut every = tokio::time::interval(Duration::from_secs(2));
                loop {
                    every.tick().await;
                    if downloads.save_if_changed() {
                        events::changed(&app, events::Topic::Progress);
                    }
                }
            });
        }
        alerts::start(&state);
        notifications::start(&state);
        accounts::start(&state);
        downloads::resume(&state);
        chat::start(&state);
        sharing::start(&state);
        nat::start(&state);
        wishlist::start(&state);
        sharing::Totals::start(&state);
        automation::start(&state);
        favourites::start(&state);
        bandcamp::start(&state);
        soundcloud::start(&state);
        Ok(state)
    }
}

/// Build the application router. Separate from [`serve`] so tests can call routes
/// in-process without opening a socket.
pub fn router(state: AppState) -> Router {
    // Everything except health, signing in and the web UI itself needs a session.
    let signed_in = Router::new()
        .route("/api/v1/users", get(accounts::people))
        .route("/api/v1/session/appearance", put(accounts::set_appearance))
        .route("/api/v1/session/avatar", put(accounts::set_avatar).delete(accounts::remove_avatar))
        .route("/api/v1/avatars/{username}", get(accounts::avatar))
        .route("/api/v1/users/approval", put(accounts::set_approval))
        .route("/api/v1/users/{username}/permissions", put(accounts::set_permissions))
        .route("/api/v1/users/{username}/sessions", delete(accounts::revoke_person))
        .route("/api/v1/session/devices", get(accounts::devices))
        .route("/api/v1/session/devices/sign-out-others", post(accounts::revoke_other_devices))
        .route("/api/v1/session/devices/{id}", delete(accounts::revoke_device))
        .route("/api/v1/classify", get(classify_input))
        .route("/api/v1/sources", get(sources))
        .route("/api/v1/soulseek", get(soulseek_status))
        .route("/api/v1/soulseek/users/{username}", get(users::user))
        .route("/api/v1/soulseek/users/{username}/picture", get(users::picture))
        .route("/api/v1/soulseek/users/{username}/shares", get(users::share_tree))
        .route("/api/v1/soulseek/users/{username}/folder", get(users::folder))
        .route("/api/v1/bandcamp/release", get(bandcamp::release))
        .route("/api/v1/bandcamp/account", get(bandcamp::account).put(bandcamp::link).delete(bandcamp::unlink))
        .route("/api/v1/bandcamp/purchases", get(bandcamp::purchases))
        .route("/api/v1/bandcamp/purchases/sync", post(bandcamp::sync))
        .route("/api/v1/bandcamp/purchases/{id}/download", post(bandcamp::download))
        .route("/api/v1/soundcloud/artist", get(soundcloud::artist))
        .route("/api/v1/soundcloud/track", get(soundcloud::track_detail))
        .route("/api/v1/soundcloud/follows", get(soundcloud::follows))
        .route("/api/v1/soundcloud/follows/{artist}", put(soundcloud::follow).delete(soundcloud::unfollow))
        .route("/api/v1/soulseek/favourites", get(favourites::list))
        .route("/api/v1/soulseek/favourites/{username}", put(favourites::add).delete(favourites::remove))
        .route("/api/v1/soulseek/stats", get(sharing::stats))
        .route("/api/v1/soulseek/uploads", get(sharing::uploads))
        .route("/api/v1/soulseek/uploads/clear", post(sharing::clear_uploads))
        .route("/api/v1/soulseek/uploads/history", get(sharing::history))
        .route("/api/v1/soulseek/uploads/{id}", delete(sharing::cancel_upload))
        .route("/api/v1/sharing", get(sharing::status).put(sharing::update))
        .route("/api/v1/wishlist", get(wishlist::list).post(wishlist::add))
        .route("/api/v1/wishlist/batch", post(wishlist::add_many))
        .route("/api/v1/automation", get(automation::settings).put(automation::update))
        .route("/api/v1/import-options", get(finishing::get_options).put(finishing::set_options))
        .route("/api/v1/follows", get(automation::follows).post(automation::follow))
        .route("/api/v1/radar", get(automation::radar))
        .route("/api/v1/follows/albums", get(automation::album_follows).post(automation::follow_album))
        .route("/api/v1/follows/albums/{id}", delete(automation::unfollow_album))
        .route("/api/v1/follows/{id}", delete(automation::unfollow))
        .route("/api/v1/wishlist/{id}", patch(wishlist::update).delete(wishlist::remove))
        .route("/api/v1/sharing/rescan", post(sharing::rescan))
        .route("/api/v1/soulseek/chat", get(chat::overview))
        .route("/api/v1/soulseek/chat/events", get(chat::events))
        .route("/api/v1/soulseek/chat/users/{username}", get(chat::conversation).post(chat::send).delete(chat::forget))
        .route("/api/v1/soulseek/chat/rooms", post(chat::refresh_rooms))
        .route("/api/v1/soulseek/chat/rooms/{room}", get(chat::room).put(chat::join).delete(chat::leave))
        .route("/api/v1/soulseek/chat/rooms/{room}/messages", post(chat::say))
        .route("/api/v1/search", get(search::stream))
        .route("/api/v1/downloads", get(downloads::list).post(downloads::create))
        .route("/api/v1/downloads/{id}", delete(downloads::remove))
        .route("/api/v1/downloads/{id}/stop", post(downloads::stop))
        .route("/api/v1/downloads/{id}/resume", post(downloads::resume_one))
        .route("/api/v1/downloads/{id}/prioritise", post(downloads::prioritise))
        .route("/api/v1/downloads/{id}/review", get(review::report))
        .route("/api/v1/downloads/{id}/import", post(review::import))
        .route("/api/v1/downloads/{id}/files/{name}", get(review::play))
        .route("/api/v1/downloads/{id}/files/{name}/spectrogram", get(review::spectrogram))
        .route("/api/v1/library/album", get(library::album))
        .route("/api/v1/library/recent", get(stats::recent))
        .route("/api/v1/library/cover/{id}", get(stats::cover))
        .route("/api/v1/stats", get(stats::stats))
        .route("/api/v1/events", get(events::stream))
        .route("/api/v1/music/search", get(music::search))
        .route("/api/v1/music/artist", get(music::artist))
        .route("/api/v1/music/album", get(music::album))
        .route("/api/v1/music/lyrics", get(music::lyrics))
        .route("/api/v1/artwork", get(artwork::lookup))
        .route("/api/v1/artwork/image", get(artwork::image))
        .route("/api/v1/requests", get(requests::list).post(requests::create))
        .route("/api/v1/requests/{id}", delete(requests::remove))
        .route("/api/v1/requests/{id}/decision", post(requests::decide))
        .route("/api/v1/notifications", get(notifications::list).delete(notifications::clear))
        .route("/api/v1/notifications/read", post(notifications::read))
        .route("/api/v1/notifications/settings", get(alerts::settings).put(alerts::update))
        .route("/api/v1/notifications/devices", post(alerts::add_device))
        .route("/api/v1/notifications/devices/{id}", delete(alerts::remove_device))
        .route("/api/v1/notifications/test", post(alerts::test))
        .route("/api/v1/external", get(external::settings).put(external::update))
        .route("/api/v1/external/fetch", post(external::fetch))
        .route("/api/v1/diagnostics", get(diagnostics::diagnostics))
        .route("/api/v1/setup", get(setup::status).put(setup::update))
        .route("/api/v1/setup/check", post(setup::check))
        .route("/api/v1/naming", get(naming::get).put(naming::update))
        .route("/api/v1/naming/detect", post(naming::detect))
        .route("/api/v1/naming/tokens", get(naming::tokens))
        .route("/api/v1/naming/preview", post(naming::preview))
        .route_layer(middleware::from_fn_with_state(state.clone(), accounts::require_session));

    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/openapi.json", get(openapi::document))
        .route("/api/v1/session", get(accounts::session).post(accounts::sign_in).delete(accounts::sign_out))
        .merge(signed_in)
        .fallback(web::serve_asset)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

/// `GET /api/v1/health`: whether the server is up.
#[utoipa::path(
    get,
    operation_id = "health",
    path = "/api/v1/health",
    tag = "system",
    security(()),
    responses(
        (status = 200, description = "OK", body = delune_core::api::Health),
    ),
)]
async fn health() -> Json<Health> {
    Json(Health { name: "delune".into(), version: env!("CARGO_PKG_VERSION").into(), status: HealthStatus::Ok })
}

#[derive(Debug, Deserialize)]
struct ClassifyParams {
    q: String,
}

/// What the search bar will do with the current input.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Classification {
    Empty,
    Text { query: String },
    Link { provider: Provider, entity: EntityKind, id: String },
    ShortLink { provider: Provider },
}

/// `GET /api/v1/classify?q=…` — lets clients show "Spotify album" vs "text search"
/// as the user types, using the same parser the server resolves with.
#[utoipa::path(
    get,
    operation_id = "classify_input",
    path = "/api/v1/classify",
    tag = "search",
    params(
        ("q" = String, Query, description = "What was typed or pasted"),
    ),
    responses(
        (status = 200, description = "OK", body = Classification),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
async fn classify_input(Query(params): Query<ClassifyParams>) -> Json<Classification> {
    Json(match classify(&params.q) {
        delune_resolve::Query::Text(q) if q.is_empty() => Classification::Empty,
        delune_resolve::Query::Text(query) => Classification::Text { query },
        delune_resolve::Query::Link(Parsed::Link(l)) => {
            Classification::Link { provider: l.provider, entity: l.kind, id: l.id }
        }
        delune_resolve::Query::Link(Parsed::ShortLink { provider, .. }) => Classification::ShortLink { provider },
    })
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct SourceInfo {
    pub provider: Provider,
    pub name: String,
    pub role: ProviderRole,
    pub enabled: bool,
    /// 1-based search position when enabled.
    pub order: Option<usize>,
}

/// `GET /api/v1/sources` — download sources and whether each is in use.
#[utoipa::path(
    get,
    operation_id = "sources",
    path = "/api/v1/sources",
    tag = "system",
    responses(
        (status = 200, description = "OK", body = Vec<SourceInfo>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
async fn sources() -> Json<Vec<SourceInfo>> {
    // Settings persistence lands with the database; until then, the default policy.
    let order = SourcePolicy::default().search_order();
    Json(
        Provider::ALL
            .into_iter()
            .filter(|p| p.role() != ProviderRole::MetadataOnly)
            .map(|provider| {
                let position = order.iter().position(|p| *p == provider);
                SourceInfo {
                    provider,
                    name: provider.name().to_owned(),
                    role: provider.role(),
                    enabled: position.is_some(),
                    order: position.map(|i| i + 1),
                }
            })
            .collect(),
    )
}

/// `GET /api/v1/soulseek` — connection state for status indicators.
#[utoipa::path(
    get,
    operation_id = "soulseek_status",
    path = "/api/v1/soulseek",
    tag = "soulseek",
    responses(
        (status = 200, description = "OK", body = delune_core::api::SoulseekStatus),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
async fn soulseek_status(State(app): State<AppState>) -> Json<SoulseekStatus> {
    Json(soulseek_status_of(&app))
}

/// The Soulseek connection as clients show it.
pub(crate) fn soulseek_status_of(app: &AppState) -> SoulseekStatus {
    let Some(client) = &app.soulseek else {
        return SoulseekStatus {
            state: SoulseekState::NotConfigured,
            username: None,
            message: Some("No Soulseek account is configured.".into()),
            public_ip: None,
            listen_port: None,
            reachable: false,
            port_mapping: app.nat.status(),
        };
    };
    let session = client.state().borrow().clone();
    let (state, message) = match session.clone() {
        SessionState::Connecting { attempt } if attempt > 1 => {
            (SoulseekState::Connecting, Some(format!("Connecting (attempt {attempt})")))
        }
        SessionState::Connecting { .. } => (SoulseekState::Connecting, None),
        SessionState::Online { .. } => (SoulseekState::Online, None),
        SessionState::Reconnecting { reason, retry_in } => {
            (SoulseekState::Reconnecting, Some(format!("{reason}. Retrying in {}s.", retry_in.as_secs())))
        }
        SessionState::Stopped(reason) => (
            SoulseekState::Stopped,
            Some(match reason {
                StopReason::LoginRejected(r) => {
                    format!("Soulseek refused the login ({r:?}). Check the username and password.")
                }
                StopReason::LoggedInElsewhere => "This Soulseek account signed in from another client.".into(),
                StopReason::Shutdown => "Soulseek client has shut down.".into(),
            }),
        ),
    };
    let public_ip = match &session {
        SessionState::Online { public_ip, .. } => Some(public_ip.to_string()),
        _ => None,
    };
    SoulseekStatus {
        state,
        username: app.soulseek_username.clone(),
        message,
        public_ip,
        listen_port: match &session {
            SessionState::Online { listen_port, .. } => *listen_port,
            _ => None,
        },
        reachable: client.incoming_connections() > 0,
        port_mapping: app.nat.status(),
    }
}

/// Bind and serve until the process receives Ctrl+C / SIGTERM.
pub async fn serve(addr: SocketAddr, config: ServerConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(address = %listener.local_addr()?, data_dir = %config.data_dir.display(), "delune server listening");
    if config.soulseek.is_none() {
        tracing::warn!("no Soulseek account configured; search is disabled");
    }
    let state = AppState::start(config)?;
    axum::serve(listener, router(state)).with_graceful_shutdown(shutdown_signal()).await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use delune_core::api::ApiError;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get(uri: &str) -> (StatusCode, bytes::Bytes) {
        let response =
            router(AppState::default()).oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = response.status();
        (status, response.into_body().collect().await.unwrap().to_bytes())
    }

    async fn get_json<T: serde::de::DeserializeOwned>(uri: &str) -> T {
        let (status, body) = get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn health_reports_ok() {
        let health: Health = get_json("/api/v1/health").await;
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.name, "delune");
    }

    #[tokio::test]
    async fn api_needs_a_session_when_accounts_are_on() {
        let accounts = Arc::new(accounts::Accounts::in_memory(Some("http://navidrome.invalid".into())));
        let (token, _) = accounts.signed_in("sam", false);
        let app = router(AppState { accounts, ..AppState::default() });
        let status = |uri: &'static str, token: Option<&str>| {
            let app = app.clone();
            let token = token.map(str::to_owned);
            async move {
                let mut request = Request::get(uri);
                if let Some(token) = token {
                    request = request.header("cookie", format!("delune_session={token}"));
                }
                app.oneshot(request.body(Body::empty()).unwrap()).await.unwrap().status()
            }
        };

        assert_eq!(status("/api/v1/health", None).await, StatusCode::OK);
        assert_eq!(status("/api/v1/session", None).await, StatusCode::OK, "answers null, not 401");
        assert_eq!(status("/api/v1/downloads", None).await, StatusCode::UNAUTHORIZED);
        assert_eq!(status("/api/v1/classify?q=x", Some("wrong")).await, StatusCode::UNAUTHORIZED);

        assert_eq!(status("/api/v1/session", Some(&token)).await, StatusCode::OK);
        assert_eq!(status("/api/v1/downloads", Some(&token)).await, StatusCode::OK);
        assert_eq!(status("/api/v1/users", Some(&token)).await, StatusCode::FORBIDDEN, "members can't manage people");
    }

    #[tokio::test]
    async fn naming_settings_are_validated_saved_and_detected() {
        let dir = std::env::temp_dir().join(format!("delune-naming-{}", std::process::id()));
        let library = dir.join("music");
        let album = library.join("Radiohead").join("1997 - OK Computer");
        std::fs::create_dir_all(&album).unwrap();
        let db = Arc::new(store::Database::open(&dir).unwrap());
        let naming = Arc::new(naming::Naming::open(&db, &review::LibrarySettings::default()));
        let app = router(AppState {
            naming: naming.clone(),
            library: Arc::new(review::LibrarySettings { library_dir: Some(library), ..Default::default() }),
            ..AppState::default()
        });
        let send = |method: &str, uri: &str, body: &str| {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap();
            let app = app.clone();
            async move {
                let response = app.oneshot(request).await.unwrap();
                let status = response.status();
                (status, response.into_body().collect().await.unwrap().to_bytes())
            }
        };
        let options = serde_json::to_string(&delune_library::NamingOptions::default()).unwrap();

        let (status, _) =
            send("PUT", "/api/v1/naming", &format!(r#"{{"template":"{{nope}}","options":{options}}}"#)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) =
            send("PUT", "/api/v1/naming", &format!(r#"{{"template":"{{artist}}/{{title}}","options":{options}}}"#))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(naming.current().0.as_str(), "{artist}/{title}");
        let reopened = naming::Naming::open(&db, &review::LibrarySettings::default());
        assert_eq!(reopened.current().0.as_str(), "{artist}/{title}", "saved across restarts");

        // An empty library has nothing to detect.
        let (status, body) = send("POST", "/api/v1/naming/detect", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(serde_json::from_slice::<ApiError>(&body).unwrap().code, "empty-library");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn requests_are_approved_followed_and_notified() {
        use delune_core::api::{DownloadJob, JobStatus, MusicRequest, Notifications, RequestStatus, ReviewState};
        let accounts = Arc::new(accounts::Accounts::in_memory(Some("http://navidrome.invalid".into())));
        let (sam, _) = accounts.signed_in("sam", false);
        let (alex, _) = accounts.signed_in("alex", true);
        let state = AppState { accounts, ..AppState::default() };
        let app = router(state.clone());
        let call = |method: &str, uri: &str, token: &str, body: Option<&str>| {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("cookie", format!("delune_session={token}"))
                .header("content-type", "application/json")
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
                .unwrap();
            let app = app.clone();
            async move {
                let response = app.oneshot(request).await.unwrap();
                let status = response.status();
                (status, response.into_body().collect().await.unwrap().to_bytes())
            }
        };

        let (status, body) = call(
            "POST",
            "/api/v1/requests",
            &sam,
            Some(r#"{"title":"Untrue","artist":"Burial","query":"Burial Untrue","note":"for the drive"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let request: MusicRequest = serde_json::from_slice(&body).unwrap();
        assert_eq!(request.status, RequestStatus::Pending);
        let (status, _) =
            call("POST", "/api/v1/requests", &sam, Some(r#"{"title":"Untrue","query":"burial untrue"}"#)).await;
        assert_eq!(status, StatusCode::OK, "asking again is the same request");

        let inbox = |token: &str| {
            let token = token.to_owned();
            async move {
                let (_, body) = call("GET", "/api/v1/notifications", &token, None).await;
                serde_json::from_slice::<Notifications>(&body).unwrap()
            }
        };
        assert_eq!(inbox(&alex).await.items[0].title, "sam asked for Untrue by Burial");

        let decide = format!("/api/v1/requests/{}/decision", request.id);
        let (status, _) = call("POST", &decide, &sam, Some(r#"{"approve":true}"#)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "members can't approve");
        let (status, body) = call("POST", &decide, &alex, Some(r#"{"approve":true}"#)).await;
        assert_eq!(status, StatusCode::OK);
        let approved: MusicRequest = serde_json::from_slice(&body).unwrap();
        assert_eq!(approved.status, RequestStatus::Searching);
        assert!(inbox(&sam).await.items[0].title.starts_with("alex approved your request"));

        // The wishlist finds and downloads it for sam, who then imports it.
        let wish = approved.wishlist_id.clone().unwrap();
        state.wishlist.with_items(|items| {
            let item = items.iter_mut().find(|i| i.id == wish).unwrap();
            assert_eq!(item.added_by, "sam");
            item.download_id = Some("job1".into());
        });
        let job = DownloadJob {
            id: "job1".into(),
            username: "peer".into(),
            folder: "x".into(),
            title: "Untrue".into(),
            parent: Some("Burial".into()),
            created_at: 1,
            status: JobStatus::Imported,
            files: vec![],
            bytes: 0,
            total_bytes: 0,
            review: ReviewState::Ready,
            requested_by: Some("sam".into()),
            imported_to: None,
            imported_at: None,
            priority: 0,
            waiting_for_slot: None,
            error: None,
        };
        requests::job_changed(&state, &job);
        let (_, body) = call("GET", "/api/v1/requests", &sam, None).await;
        let listed: Vec<MusicRequest> = serde_json::from_slice(&body).unwrap();
        assert_eq!((listed[0].status, listed[0].download_id.as_deref()), (RequestStatus::Available, Some("job1")));
        let sam_inbox = inbox(&sam).await;
        assert_eq!(sam_inbox.items[0].title, "Untrue by Burial is in the library");
        assert_eq!(sam_inbox.unread, 2);
    }

    #[tokio::test]
    async fn classify_links_and_text() {
        let link: Classification = get_json("/api/v1/classify?q=https%3A%2F%2Fwww.deezer.com%2Falbum%2F302127").await;
        assert_eq!(
            link,
            Classification::Link { provider: Provider::Deezer, entity: EntityKind::Album, id: "302127".into() }
        );
        let text: Classification = get_json("/api/v1/classify?q=ok%20computer").await;
        assert_eq!(text, Classification::Text { query: "ok computer".into() });
        let empty: Classification = get_json("/api/v1/classify?q=%20").await;
        assert_eq!(empty, Classification::Empty);
    }

    #[tokio::test]
    async fn fresh_install_uses_only_soulseek() {
        let sources: Vec<SourceInfo> = get_json("/api/v1/sources").await;
        let enabled: Vec<_> = sources.iter().filter(|s| s.enabled).map(|s| s.provider).collect();
        assert_eq!(enabled, vec![Provider::Soulseek]);
        assert!(sources.iter().all(|s| s.role != ProviderRole::MetadataOnly));
    }

    #[tokio::test]
    async fn search_without_soulseek_explains_what_to_do() {
        let status: SoulseekStatus = get_json("/api/v1/soulseek").await;
        assert_eq!(status.state, SoulseekState::NotConfigured);

        let (code, body) = get("/api/v1/search?q=ok%20computer").await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        let error: ApiError = serde_json::from_slice(&body).unwrap();
        assert_eq!(error.code, "soulseek-not-configured");
    }
}
