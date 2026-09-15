//! # delune-server
//!
//! The HTTP API every client talks to. The web UI is served from the same origin,
//! and the TUI connects over the same routes, so there is exactly one way to drive
//! delune.
//!
//! Routes are versioned under `/api/v1`. Long-running work (searches, downloads,
//! scans) will stream progress over Server-Sent Events rather than being polled.

mod web;

use std::net::SocketAddr;

use axum::{Json, Router, extract::Query, routing::get};
use delune_core::{
    Provider, ProviderRole, SourcePolicy,
    api::{Health, HealthStatus},
};
use delune_resolve::{EntityKind, Parsed, classify};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

/// Build the application router. Separate from [`serve`] so tests can call routes
/// in-process without opening a socket.
pub fn router() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/classify", get(classify_input))
        .route("/api/v1/sources", get(sources))
        .fallback(web::serve_asset)
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

/// Bind and serve until the process receives Ctrl+C / SIGTERM.
pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(address = %listener.local_addr()?, "delune server listening");
    axum::serve(listener, router()).with_graceful_shutdown(shutdown_signal()).await
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
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_reports_ok() {
        let response = router().oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let health: Health = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.name, "delune");
    }

    async fn get_json<T: serde::de::DeserializeOwned>(uri: &str) -> T {
        let response = router().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
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
}
