//! The web UI, compiled into the binary.
//!
//! `web/dist` is embedded at build time, so a release binary is fully
//! self-contained. Unknown paths fall back to `index.html` so client-side routes
//! survive a page refresh. If the UI hasn't been built (`bun run build` in `web/`),
//! the server still runs and says how to build it.

use axum::{
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

pub async fn serve_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Unknown API routes are real 404s, not the SPA.
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(file) = Assets::get(path).filter(|_| !path.is_empty()) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Vite fingerprints everything under assets/, so those can be cached forever.
        let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        return ([(header::CONTENT_TYPE, mime.as_ref()), (header::CACHE_CONTROL, cache)], file.data).into_response();
    }

    match Assets::get("index.html") {
        Some(index) => ([(header::CACHE_CONTROL, "no-cache")], Html(index.data)).into_response(),
        None => Html(
            "<!doctype html><title>delune</title><p>The web UI isn't built into this binary. \
             Run <code>bun install &amp;&amp; bun run build</code> in <code>web/</code>, then rebuild delune.</p>",
        )
        .into_response(),
    }
}
