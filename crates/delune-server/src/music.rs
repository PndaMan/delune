//! What delune knows about an artist, an album or a song, wherever it comes from.
//!
//! This is the detail behind the artist page and the album and song views: a
//! discography from Deezer's public API, what your Navidrome library already has,
//! and lyrics from LRCLIB. Nothing here downloads anything; it's what people look at
//! before deciding to.

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{
    AlbumHit, AlbumInfo, AlbumTrack, ApiError, ArtistAlbum, ArtistHit, ArtistInfo, LibraryAlbum, MusicSearch,
};
use serde::Deserialize;
use serde_json::Value;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::artwork::{names_match, normalize};

const DEEZER: &str = "https://api.deezer.com";
/// Albums asked of Deezer for one artist.
const DISCOGRAPHY: usize = 100;

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ArtistParams {
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct AlbumParams {
    artist: Option<String>,
    album: String,
}

#[derive(Debug, Deserialize)]
pub struct LyricsParams {
    artist: String,
    title: String,
    duration_secs: Option<u32>,
}

async fn deezer(app: &AppState, path: &str, query: &[(&str, &str)]) -> Option<Value> {
    let response = app
        .music_http
        .get(format!("{DEEZER}{path}"))
        .query(query)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    let value: Value = response.json().await.ok()?;
    if value.get("error").is_some() { None } else { Some(value) }
}

fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn year_of(date: Option<&str>) -> Option<u16> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()).filter(|y| (1000..3000).contains(y))
}

/// `GET /api/v1/music/artist?name=…`: their releases, and what your library has.
#[utoipa::path(
    get,
    operation_id = "music_artist",
    path = "/api/v1/music/artist",
    tag = "library",
    params(("name" = String, Query, description = "The artist's name")),
    responses(
        (status = 200, description = "OK", body = delune_core::api::ArtistInfo),
        (status = 404, description = "No artist by that name", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn artist(State(app): State<AppState>, _user: CurrentUser, Query(params): Query<ArtistParams>) -> Response {
    let name = params.name.trim();
    if name.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no-name", "Say which artist.");
    }
    let library = library_albums(&app, name).await;
    // Deezer's first few results for a common name are often obscure acts with the
    // same name, so look at a wider slice and take the best known exact match.
    let found = deezer(&app, "/search/artist", &[("q", name), ("limit", "25")]).await;
    // Several artists can share a name ("Burial"); the best known one is meant.
    let artist = found
        .as_ref()
        .and_then(|v| v.get("data")?.as_array())
        .and_then(|artists| {
            let wanted = normalize(name);
            let exact = |a: &&Value| str_at(a, "/name").is_some_and(|n| names_match(&normalize(n), &wanted));
            let fans = |a: &&Value| a.get("nb_fan").and_then(Value::as_u64).unwrap_or(0);
            artists.iter().filter(exact).max_by_key(fans).or_else(|| artists.first())
        })
        .cloned();

    let Some(artist) = artist else {
        // Nothing on Deezer: the library may still know them.
        if library.is_empty() {
            return error(StatusCode::NOT_FOUND, "no-such-artist", "Nothing found for that artist.");
        }
        return Json(ArtistInfo {
            name: name.to_owned(),
            picture: None,
            listeners: None,
            albums: Vec::new(),
            in_library: library,
        })
        .into_response();
    };

    let id = artist.get("id").and_then(Value::as_u64).unwrap_or(0).to_string();
    let releases = deezer(&app, &format!("/artist/{id}/albums"), &[("limit", &DISCOGRAPHY.to_string())]).await;
    let owned: Vec<String> = library.iter().map(|a| normalize(&a.title)).collect();
    let albums = releases
        .as_ref()
        .and_then(|v| v.get("data")?.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|album| {
                    let title = str_at(album, "/title")?.to_owned();
                    let known = normalize(&title);
                    Some(ArtistAlbum {
                        title,
                        year: year_of(str_at(album, "/release_date")),
                        cover: str_at(album, "/cover_medium").map(str::to_owned),
                        kind: str_at(album, "/record_type").unwrap_or("album").to_owned(),
                        in_library: owned.iter().any(|o| names_match(o, &known)),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Json(ArtistInfo {
        name: str_at(&artist, "/name").unwrap_or(name).to_owned(),
        picture: str_at(&artist, "/picture_medium").map(str::to_owned),
        listeners: artist.get("nb_fan").and_then(Value::as_u64),
        albums,
        in_library: library,
    })
    .into_response()
}

/// Albums by `artist` that Navidrome has.
async fn library_albums(app: &AppState, artist: &str) -> Vec<LibraryAlbum> {
    let Some(navidrome) = &app.navidrome else { return Vec::new() };
    let Ok(found) = navidrome.search(artist, 60, 0).await else { return Vec::new() };
    let wanted = normalize(artist);
    found
        .album
        .into_iter()
        .filter(|album| album.artist.as_deref().is_some_and(|a| names_match(&normalize(a), &wanted)))
        .map(|album| LibraryAlbum {
            id: album.id,
            title: album.name,
            year: album.year,
            track_count: album.song_count.unwrap_or(0),
        })
        .collect()
}

/// `GET /api/v1/music/album?artist=…&album=…`: its tracklist, and whether you have it.
#[utoipa::path(
    get,
    operation_id = "music_album",
    path = "/api/v1/music/album",
    tag = "library",
    params(
        ("artist" = Option<String>, Query, description = "The album artist, when known"),
        ("album" = String, Query, description = "The album title"),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::AlbumInfo),
        (status = 404, description = "No album by that name", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn album(State(app): State<AppState>, _user: CurrentUser, Query(params): Query<AlbumParams>) -> Response {
    let (artist, title) = (params.artist.as_deref().map(str::trim), params.album.trim());
    if title.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no-album", "Say which album.");
    }
    // Deezer's field query finds nothing for some artists, so plain words are the fallback.
    let artist = artist.filter(|a| !a.is_empty());
    let mut queries = Vec::new();
    if let Some(artist) = artist {
        queries.push(format!("artist:\"{artist}\" album:\"{title}\""));
        queries.push(format!("{artist} {title}"));
    } else {
        queries.push(title.to_owned());
    }
    let wanted = normalize(title);
    let wanted_artist = artist.map(normalize);
    let mut best = None;
    for query in queries {
        let found = deezer(&app, "/search/album", &[("q", query.as_str()), ("limit", "10")]).await;
        best = found.as_ref().and_then(|v| v.get("data")?.as_array()).and_then(|albums| {
            albums
                .iter()
                .find(|album| {
                    let title_ok = str_at(album, "/title").is_some_and(|t| names_match(&normalize(t), &wanted));
                    let artist_ok = wanted_artist
                        .as_ref()
                        .is_none_or(|a| str_at(album, "/artist/name").is_some_and(|n| names_match(&normalize(n), a)));
                    title_ok && artist_ok
                })
                .cloned()
        });
        if best.is_some() {
            break;
        }
    }
    let in_library = crate::library::lookup(&app, artist, title, None).await;

    let Some(best) = best else {
        return Json(AlbumInfo {
            title: title.to_owned(),
            artist: artist.map(str::to_owned),
            year: None,
            cover: None,
            tracks: Vec::new(),
            in_library,
        })
        .into_response();
    };
    let id = best.get("id").and_then(Value::as_u64).unwrap_or(0).to_string();
    let full = deezer(&app, &format!("/album/{id}"), &[]).await;
    let tracks = full
        .as_ref()
        .and_then(|v| v.pointer("/tracks/data")?.as_array())
        .map(|items| {
            items
                .iter()
                .enumerate()
                .filter_map(|(index, track)| {
                    Some(AlbumTrack {
                        position: track
                            .get("track_position")
                            .and_then(Value::as_u64)
                            .and_then(|p| u32::try_from(p).ok())
                            .unwrap_or_else(|| u32::try_from(index + 1).unwrap_or(1)),
                        title: str_at(track, "/title")?.to_owned(),
                        artist: str_at(track, "/artist/name").map(str::to_owned),
                        duration_secs: track
                            .get("duration")
                            .and_then(Value::as_u64)
                            .and_then(|d| u32::try_from(d).ok()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Json(AlbumInfo {
        title: str_at(&best, "/title").unwrap_or(title).to_owned(),
        artist: str_at(&best, "/artist/name").map(str::to_owned).or_else(|| artist.map(str::to_owned)),
        year: year_of(str_at(full.as_ref().unwrap_or(&best), "/release_date")),
        cover: str_at(&best, "/cover_xl").or_else(|| str_at(&best, "/cover_medium")).map(str::to_owned),
        tracks,
        in_library,
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    q: String,
}

/// `GET /api/v1/music/search?q=…`: artists and albums for a wishlist type-ahead.
#[utoipa::path(
    get,
    operation_id = "music_search",
    path = "/api/v1/music/search",
    tag = "library",
    params(("q" = String, Query, description = "What someone has typed so far")),
    responses(
        (status = 200, description = "OK", body = delune_core::api::MusicSearch),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn search(State(app): State<AppState>, _user: CurrentUser, Query(params): Query<SearchParams>) -> Response {
    let query = params.q.trim();
    if query.chars().count() < 2 {
        return Json(MusicSearch::default()).into_response();
    }
    // Both lists at once: people type an artist as often as an album.
    let (artist_query, album_query) = ([("q", query), ("limit", "6")], [("q", query), ("limit", "12")]);
    let (artists, albums) =
        tokio::join!(deezer(&app, "/search/artist", &artist_query), deezer(&app, "/search/album", &album_query),);
    let artists = artists
        .as_ref()
        .and_then(|v| v.get("data")?.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|a| {
                    Some(ArtistHit {
                        name: str_at(a, "/name")?.to_owned(),
                        picture: str_at(a, "/picture_medium").map(str::to_owned),
                        listeners: a.get("nb_fan").and_then(Value::as_u64),
                    })
                })
                .take(5)
                .collect()
        })
        .unwrap_or_default();
    let albums = albums
        .as_ref()
        .and_then(|v| v.get("data")?.as_array())
        .map(|items| {
            let mut seen = std::collections::HashSet::new();
            items
                .iter()
                .filter_map(|album| {
                    let title = str_at(album, "/title")?.to_owned();
                    let artist = str_at(album, "/artist/name")?.to_owned();
                    // Deezer lists the same album once per market; one of each will do.
                    if !seen.insert((normalize(&artist), normalize(&title))) {
                        return None;
                    }
                    Some(AlbumHit {
                        title,
                        artist,
                        year: None,
                        cover: str_at(album, "/cover_medium").map(str::to_owned),
                        track_count: album.get("nb_tracks").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
                    })
                })
                .take(8)
                .collect()
        })
        .unwrap_or_default();
    Json(MusicSearch { artists, albums }).into_response()
}

/// `GET /api/v1/music/lyrics?artist=…&title=…`: words for a song, from LRCLIB.
#[utoipa::path(
    get,
    operation_id = "music_lyrics",
    path = "/api/v1/music/lyrics",
    tag = "library",
    params(
        ("artist" = String, Query, description = "The track artist"),
        ("title" = String, Query, description = "The track title"),
        ("duration_secs" = Option<u32>, Query, description = "Its length, to pick the right version"),
    ),
    responses(
        (status = 200, description = "OK", body = crate::music::Words),
        (status = 404, description = "No lyrics for that song", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn lyrics(State(app): State<AppState>, _user: CurrentUser, Query(params): Query<LyricsParams>) -> Response {
    let found =
        crate::finishing::lyrics_for(&app.music_http, &params.title, &params.artist, params.duration_secs).await;
    match found {
        Some(lyrics) => Json(Words { synced: lyrics.synced, plain: lyrics.plain }).into_response(),
        None => error(StatusCode::NOT_FOUND, "no-lyrics", "LRCLIB doesn't have this one."),
    }
}

/// `GET /api/v1/music/lyrics`
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct Words {
    /// LRC, with timings.
    pub synced: Option<String>,
    pub plain: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_deezer_shapes() {
        let album = serde_json::json!({"title": "Untrue", "release_date": "2007-11-05", "cover_medium": "u"});
        assert_eq!(str_at(&album, "/title"), Some("Untrue"));
        assert_eq!(year_of(str_at(&album, "/release_date")), Some(2007));
        assert_eq!(year_of(Some("nope")), None);
    }
}
