//! # delune-server
//!
//! The HTTP API every client talks to. The web UI is served from the same origin,
//! and the TUI connects over the same routes, so there is exactly one way to drive
//! delune.
//!
//! Routes are versioned under `/api/v1`. Long-running work (searches, downloads,
//! scans) streams progress over Server-Sent Events rather than being polled.

pub mod artwork;
pub mod downloads;
pub mod naming;
pub mod search;
mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{delete, get, post},
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { soulseek: None, data_dir: PathBuf::from("delune-data") }
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
        }
    }
}

impl AppState {
    /// Start background services described by `config`. Needs a Tokio runtime.
    #[must_use]
    pub fn start(config: ServerConfig) -> Self {
        let mut state = Self { data_dir: config.data_dir, ..Self::default() };
        if let Some(slsk) = config.soulseek {
            state.soulseek_username = Some(slsk.username.clone());
            state.search_timeout = slsk.search_timeout;
            state.soulseek = Some(delune_soulseek::Client::start(slsk));
        }
        state
    }
}

/// Build the application router. Separate from [`serve`] so tests can call routes
/// in-process without opening a socket.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/classify", get(classify_input))
        .route("/api/v1/sources", get(sources))
        .route("/api/v1/soulseek", get(soulseek_status))
        .route("/api/v1/search", get(search::stream))
        .route("/api/v1/downloads", get(downloads::list).post(downloads::create))
        .route("/api/v1/downloads/{id}", delete(downloads::remove))
        .route("/api/v1/artwork", get(artwork::lookup))
        .route("/api/v1/artwork/image", get(artwork::image))
        .route("/api/v1/naming/tokens", get(naming::tokens))
        .route("/api/v1/naming/preview", post(naming::preview))
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
