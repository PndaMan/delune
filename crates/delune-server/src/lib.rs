//! # delune-server
//!
//! The HTTP API every client talks to. The web UI is served from the same origin,
//! and the TUI connects over the same routes, so there is exactly one way to drive
//! delune.
//!
//! Routes are versioned under `/api/v1`. Long-running work (searches, downloads,
//! scans) streams progress over Server-Sent Events rather than being polled.

pub mod accounts;
pub mod artwork;
pub mod automation;
pub mod chat;
pub mod downloads;
pub mod finishing;
pub mod library;
pub mod naming;
pub mod review;
pub mod search;
pub mod sharing;
pub mod users;
mod web;
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            soulseek: None,
            data_dir: PathBuf::from("delune-data"),
            library: review::LibrarySettings::default(),
            navidrome: None,
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
    pub chat: Arc<chat::Chat>,
    pub sharing: Arc<sharing::Sharing>,
    pub wishlist: Arc<wishlist::Wishlist>,
    pub totals: Arc<sharing::Totals>,
    pub automation: Arc<automation::Automation>,
    pub finishing: Arc<finishing::Finishing>,
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
            chat: Arc::default(),
            sharing: Arc::default(),
            wishlist: Arc::default(),
            totals: Arc::default(),
            automation: Arc::default(),
            finishing: Arc::default(),
        }
    }
}

impl AppState {
    /// Start background services described by `config`. Needs a Tokio runtime.
    #[must_use]
    pub fn start(config: ServerConfig) -> Self {
        let navidrome_url = config.navidrome.as_ref().map(|(url, _)| url.clone());
        let accounts = Arc::new(accounts::Accounts::open(&config.data_dir, navidrome_url));
        let chat = Arc::new(chat::Chat::open(&config.data_dir));
        let sharing = Arc::new(sharing::Sharing::open(&config.data_dir));
        let wishlist = Arc::new(wishlist::Wishlist::open(&config.data_dir));
        let totals = Arc::new(sharing::Totals::open(&config.data_dir));
        let automation = Arc::new(automation::Automation::open(&config.data_dir));
        let finishing = Arc::new(finishing::Finishing::open(&config.data_dir));
        let navidrome =
            config.navidrome.and_then(|(url, credentials)| match delune_navidrome::Client::new(&url, credentials) {
                Ok(client) => Some(client),
                Err(error) => {
                    tracing::warn!(%error, "ignoring Navidrome settings");
                    None
                }
            });
        let downloads = Arc::new(downloads::Downloads::open(&config.data_dir));
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
            ..Self::default()
        };
        if let Some(slsk) = config.soulseek {
            state.soulseek_username = Some(slsk.username.clone());
            state.search_timeout = slsk.search_timeout;
            state.soulseek = Some(delune_soulseek::Client::start(slsk));
        }
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(2));
            loop {
                every.tick().await;
                downloads.save_if_changed();
            }
        });
        downloads::resume(&state);
        chat::start(&state);
        sharing::start(&state);
        wishlist::start(&state);
        sharing::Totals::start(&state);
        automation::start(&state);
        state
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
        .route("/api/v1/classify", get(classify_input))
        .route("/api/v1/sources", get(sources))
        .route("/api/v1/soulseek", get(soulseek_status))
        .route("/api/v1/soulseek/users/{username}", get(users::user))
        .route("/api/v1/soulseek/users/{username}/picture", get(users::picture))
        .route("/api/v1/soulseek/users/{username}/shares", get(users::share_tree))
        .route("/api/v1/soulseek/users/{username}/folder", get(users::folder))
        .route("/api/v1/soulseek/stats", get(sharing::stats))
        .route("/api/v1/soulseek/uploads", get(sharing::uploads))
        .route("/api/v1/soulseek/uploads/clear", post(sharing::clear_uploads))
        .route("/api/v1/soulseek/uploads/{id}", delete(sharing::cancel_upload))
        .route("/api/v1/sharing", get(sharing::status).put(sharing::update))
        .route("/api/v1/wishlist", get(wishlist::list).post(wishlist::add))
        .route("/api/v1/wishlist/batch", post(wishlist::add_many))
        .route("/api/v1/automation", get(automation::settings).put(automation::update))
        .route("/api/v1/import-options", get(finishing::get_options).put(finishing::set_options))
        .route("/api/v1/follows", get(automation::follows).post(automation::follow))
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
        .route("/api/v1/library/album", get(library::album))
        .route("/api/v1/artwork", get(artwork::lookup))
        .route("/api/v1/artwork/image", get(artwork::image))
        .route("/api/v1/naming/tokens", get(naming::tokens))
        .route("/api/v1/naming/preview", post(naming::preview))
        .route_layer(middleware::from_fn_with_state(state.clone(), accounts::require_session));

    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/session", get(accounts::session).post(accounts::sign_in).delete(accounts::sign_out))
        .merge(signed_in)
        .fallback(web::serve_asset)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> Json<Health> {
    Json(Health { name: "delune".into(), version: env!("CARGO_PKG_VERSION").into(), status: HealthStatus::Ok })
}

#[derive(Debug, Deserialize)]
struct ClassifyParams {
    q: String,
}

/// What the search bar will do with the current input.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Classification {
    Empty,
    Text { query: String },
    Link { provider: Provider, entity: EntityKind, id: String },
    ShortLink { provider: Provider },
}

/// `GET /api/v1/classify?q=…` — lets clients show "Spotify album" vs "text search"
/// as the user types, using the same parser the server resolves with.
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceInfo {
    pub provider: Provider,
    pub name: String,
    pub role: ProviderRole,
    pub enabled: bool,
    /// 1-based search position when enabled.
    pub order: Option<usize>,
}

/// `GET /api/v1/sources` — download sources and whether each is in use.
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
async fn soulseek_status(State(app): State<AppState>) -> Json<SoulseekStatus> {
    let Some(client) = &app.soulseek else {
        return Json(SoulseekStatus {
            state: SoulseekState::NotConfigured,
            username: None,
            message: Some("No Soulseek account is configured.".into()),
        });
    };
    let session = client.state().borrow().clone();
    let (state, message) = match session {
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
                StopReason::LoggedInElsewhere => {
                    "This Soulseek account signed in from another client. Restart delune to reconnect.".into()
                }
                StopReason::Shutdown => "Soulseek client has shut down.".into(),
            }),
        ),
    };
    Json(SoulseekStatus { state, username: app.soulseek_username.clone(), message })
}

/// Bind and serve until the process receives Ctrl+C / SIGTERM.
pub async fn serve(addr: SocketAddr, config: ServerConfig) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(address = %listener.local_addr()?, data_dir = %config.data_dir.display(), "delune server listening");
    if config.soulseek.is_none() {
        tracing::warn!("no Soulseek account configured; search is disabled");
    }
    let state = AppState::start(config);
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
