//! The API described as OpenAPI 3.1, served at `/api/v1/openapi.json`.
//!
//! Every route carries a `#[utoipa::path]` saying what it takes and returns, and a
//! test checks that no route registered in the router is missing from here.

use axum::Json;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "delune",
        description = "Find music on Soulseek, review it, and add it to your Navidrome library. Most routes need a session: sign in with `POST /api/v1/session` and send the `delune_session` cookie, or `Authorization: Bearer <token>` (ask for a token with `\"token\": true`).",
        license(name = "AGPL-3.0-only", identifier = "AGPL-3.0-only")
    ),
    servers((url = "/", description = "This delune")),
    modifiers(&Security),
    security(("session" = []), ("bearer" = [])),
    paths(
        crate::health,
        crate::classify_input,
        crate::sources,
        crate::soulseek_status,
        crate::accounts::session,
        crate::accounts::sign_in,
        crate::accounts::sign_out,
        crate::accounts::set_appearance,
        crate::accounts::set_avatar,
        crate::accounts::remove_avatar,
        crate::accounts::avatar,
        crate::accounts::devices,
        crate::accounts::revoke_device,
        crate::accounts::revoke_other_devices,
        crate::accounts::people,
        crate::accounts::set_permissions,
        crate::accounts::set_approval,
        crate::accounts::revoke_person,
        crate::users::user,
        crate::users::picture,
        crate::users::share_tree,
        crate::users::folder,
        crate::bandcamp::release,
        crate::bandcamp::account,
        crate::bandcamp::link,
        crate::bandcamp::unlink,
        crate::bandcamp::purchases,
        crate::bandcamp::sync,
        crate::bandcamp::download,
        crate::soundcloud::artist,
        crate::soundcloud::track_detail,
        crate::soundcloud::follows,
        crate::soundcloud::follow,
        crate::soundcloud::unfollow,
        crate::favourites::list,
        crate::favourites::add,
        crate::favourites::remove,
        crate::sharing::stats,
        crate::sharing::uploads,
        crate::sharing::clear_uploads,
        crate::sharing::cancel_upload,
        crate::sharing::status,
        crate::sharing::update,
        crate::sharing::rescan,
        crate::wishlist::list,
        crate::wishlist::add,
        crate::wishlist::add_many,
        crate::wishlist::update,
        crate::wishlist::remove,
        crate::automation::settings,
        crate::automation::update,
        crate::automation::follows,
        crate::automation::follow,
        crate::automation::unfollow,
        crate::finishing::get_options,
        crate::finishing::set_options,
        crate::chat::overview,
        crate::chat::events,
        crate::chat::conversation,
        crate::chat::send,
        crate::chat::forget,
        crate::chat::refresh_rooms,
        crate::chat::room,
        crate::chat::join,
        crate::chat::leave,
        crate::chat::say,
        crate::search::stream,
        crate::downloads::list,
        crate::downloads::create,
        crate::downloads::remove,
        crate::downloads::stop,
        crate::downloads::resume_one,
        crate::downloads::prioritise,
        crate::review::report,
        crate::review::import,
        crate::library::album,
        crate::events::stream,
        crate::music::search,
        crate::music::artist,
        crate::music::album,
        crate::music::lyrics,
        crate::artwork::lookup,
        crate::artwork::image,
        crate::requests::list,
        crate::requests::create,
        crate::requests::remove,
        crate::requests::decide,
        crate::notifications::list,
        crate::notifications::read,
        crate::notifications::clear,
        crate::external::settings,
        crate::external::update,
        crate::external::fetch,
        crate::setup::status,
        crate::setup::check,
        crate::setup::update,
        crate::naming::get,
        crate::naming::update,
        crate::naming::detect,
        crate::naming::tokens,
        crate::naming::preview
    ),
    tags(
        (name = "session", description = "Signing in, your devices and how delune looks for you"),
        (name = "people", description = "Everyone who signs in, and what they may do"),
        (name = "search", description = "Searching Soulseek, and understanding pasted links"),
        (name = "downloads", description = "Download jobs"),
        (name = "review", description = "Checking downloads and importing them"),
        (name = "requests", description = "Asking for albums, and approving requests"),
        (name = "notifications", description = "What people should know about"),
        (name = "wishlist", description = "Searches repeated until something good turns up"),
        (name = "automation", description = "Followed artists and quality upgrades"),
        (name = "library", description = "What's in Navidrome, and artwork"),
        (name = "soulseek", description = "The Soulseek connection and its users"),
        (name = "chat", description = "Private messages and rooms"),
        (name = "bandcamp", description = "Albums on Bandcamp, and your purchases there"),
        (name = "soundcloud", description = "Artists on SoundCloud, and following them"),
        (name = "sharing", description = "Sharing the library"),
        (name = "uploads", description = "Other people downloading from delune"),
        (name = "settings", description = "Connections, naming and import settings"),
        (name = "system", description = "Health and sources"),
    )
)]
#[derive(Debug)]
pub struct ApiDoc;

/// Sessions travel in a cookie (the web UI) or a bearer header (the TUI, scripts).
struct Security;

impl utoipa::Modify for Security {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
        let components = openapi.components.get_or_insert_with(Default::default);
        components
            .add_security_scheme("session", SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("delune_session"))));
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
        );
    }
}

/// `GET /api/v1/openapi.json`
pub async fn document() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `.route("/api/v1/…", method(…))` in the router is described.
    #[test]
    fn every_route_is_described() {
        let doc = ApiDoc::openapi();
        let router = include_str!("lib.rs");
        let mut missing = Vec::new();
        for line in router.lines().map(str::trim).filter(|l| l.starts_with(".route(\"/api/v1/")) {
            let path = line.split('"').nth(1).unwrap_or_default();
            if path == "/api/v1/openapi.json" {
                continue;
            }
            let described = doc.paths.paths.get(path);
            for method in ["get", "post", "put", "patch", "delete"] {
                let registered = line.contains(&format!("{method}("));
                if !registered {
                    continue;
                }
                let has = described.is_some_and(|item| match method {
                    "get" => item.get.is_some(),
                    "post" => item.post.is_some(),
                    "put" => item.put.is_some(),
                    "patch" => item.patch.is_some(),
                    _ => item.delete.is_some(),
                });
                if !has {
                    missing.push(format!("{method} {path}"));
                }
            }
        }
        assert!(missing.is_empty(), "routes without an OpenAPI description: {missing:?}");
        assert!(doc.paths.paths.len() > 50);
    }
}
