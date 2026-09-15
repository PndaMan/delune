//! Album artwork for search results.
//!
//! Soulseek results are folders, and folder names are messy: `(1997) Homogenic
//! [FLAC]`, `CD1 - Kid A`, `Boards of Canada - Geogaddi (2002) {Japanese}`. To show a
//! cover next to each result we clean the name, look the album up on Deezer (free,
//! no key), and only accept a match when both artist and title agree — a missing
//! cover is fine, a wrong one isn't.
//!
//! Images are served through [`image`] so the browser loads them same-origin (which
//! lets the web UI sample colours from them) and so the proxy can refuse any host
//! that isn't a known image CDN.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use url::Url;

use crate::AppState;

/// Hosts the image proxy will fetch from.
const ALLOWED_IMAGE_HOSTS: &[&str] = &["cdn-images.dzcdn.net", "e-cdns-images.dzcdn.net"];
const MAX_IMAGE_BYTES: usize = 2 << 20;
const MAX_CACHED_LOOKUPS: usize = 2_000;
const MAX_CACHED_IMAGE_BYTES: usize = 96 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artwork {
    pub artist: String,
    pub album: String,
    /// ~250px, for result rows. Same-origin proxy URL.
    pub thumb: String,
    /// ~1000px, for detail views. Same-origin proxy URL.
    pub cover: String,
}

/// Shared lookup and image caches.
#[derive(Debug)]
pub struct ArtworkService {
    http: reqwest::Client,
    lookups: Mutex<HashMap<String, Option<Artwork>>>,
    images: Mutex<ImageCache>,
    /// Deezer allows ~50 requests per 5 seconds; stay well below it.
    deezer: Semaphore,
    deezer_base: String,
}

#[derive(Debug, Default)]
struct ImageCache {
    entries: HashMap<String, (Bytes, String)>,
    bytes: usize,
}

impl Default for ArtworkService {
    fn default() -> Self {
        Self::new("https://api.deezer.com")
    }
}

impl ArtworkService {
    #[must_use]
    pub fn new(deezer_base: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("delune/", env!("CARGO_PKG_VERSION"), " (+https://github.com/PndaMan/delune)"))
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        Self {
            http,
            lookups: Mutex::default(),
            images: Mutex::default(),
            deezer: Semaphore::new(4),
            deezer_base: deezer_base.trim_end_matches('/').to_owned(),
        }
    }

    async fn find(&self, artist: Option<&str>, album: &str, context: Option<&str>) -> Option<Artwork> {
        let (artist, album) = clean_names(artist, album);
        if album.is_empty() {
            return None;
        }
        let context = context.map(normalize).filter(|c| !c.is_empty());
        let key = format!(
            "{}\u{1f}{}\u{1f}{}",
            normalize(artist.as_deref().unwrap_or("")),
            normalize(&album),
            context.as_deref().unwrap_or("")
        );
        if let Some(hit) = self.lookups.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return hit.clone();
        }

        let mut result = self.search_deezer(artist.as_deref(), &album, None).await;
        // "Dark Side of the Moon - 7.1 Multichannel": try without the trailing descriptor.
        if matches!(result, Ok(None))
            && let Some((head, _)) = album.rsplit_once(" - ")
            && head.len() >= 4
        {
            result = self.search_deezer(artist.as_deref(), head, None).await;
        }
        // Parent folders are often categories ("Albums", "failed_imports") rather than
        // artists. Retry by title alone, accepting only an artist the user searched for.
        if matches!(result, Ok(None)) && context.is_some() {
            result = self.search_deezer(None, &album, context.as_deref()).await;
        }
        let Ok(result) = result else {
            // Network trouble: don't cache, so it's retried next time.
            return None;
        };
        let mut lookups = self.lookups.lock().unwrap_or_else(PoisonError::into_inner);
        if lookups.len() >= MAX_CACHED_LOOKUPS {
            lookups.clear();
        }
        lookups.insert(key, result.clone());
        result
    }

    /// Search Deezer for `album`. With `artist`, both must match. Without it, the
    /// match's artist must appear in `context` (the user's search text), so a generic
    /// title like "Greatest Hits" can't pick up a stranger's cover.
    async fn search_deezer(
        &self,
        artist: Option<&str>,
        album: &str,
        context: Option<&str>,
    ) -> Result<Option<Artwork>, reqwest::Error> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default)]
            data: Vec<Album>,
        }
        #[derive(Deserialize)]
        struct Album {
            title: String,
            artist: ArtistRef,
            cover_medium: Option<String>,
            cover_xl: Option<String>,
        }
        #[derive(Deserialize)]
        struct ArtistRef {
            name: String,
        }

        let _permit = self.deezer.acquire().await;
        let query = match artist {
            Some(artist) => format!("artist:\"{artist}\" album:\"{album}\""),
            None => album.to_owned(),
        };
        let response: Response = self
            .http
            .get(format!("{}/search/album", self.deezer_base))
            .query(&[("q", query.as_str()), ("limit", "10")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let wanted_album = normalize(album);
        let wanted_artist = artist.map(normalize);
        let best = response.data.into_iter().find(|candidate| {
            let title_ok = names_match(&wanted_album, &normalize(&candidate.title))
                || names_match(&wanted_album, &normalize(&strip_brackets(&candidate.title)));
            let found_artist = normalize(&candidate.artist.name);
            let artist_ok = match (&wanted_artist, context) {
                (Some(a), _) => names_match(a, &found_artist),
                (None, Some(context)) => found_artist.len() >= 3 && context.contains(&found_artist),
                (None, None) => false,
            };
            title_ok && artist_ok
        });

        Ok(best.and_then(|a| {
            Some(Artwork {
                artist: a.artist.name,
                album: a.title,
                thumb: proxy_url(a.cover_medium.as_deref()?),
                cover: proxy_url(a.cover_xl.as_deref()?),
            })
        }))
    }
}

fn proxy_url(src: &str) -> String {
    let mut url = Url::parse("http://x/api/v1/artwork/image").expect("static URL");
    url.query_pairs_mut().append_pair("src", src);
    format!("/api/v1/artwork/image?{}", url.query().unwrap_or_default())
}

#[derive(Debug, Deserialize)]
pub struct LookupParams {
    artist: Option<String>,
    album: String,
    /// What the user searched for, used to confirm artists when folder names don't say.
    context: Option<String>,
}

/// `GET /api/v1/artwork?artist=…&album=…&context=…` — `null` when there's no
/// confident match. (Not a 404: "no cover" is a normal answer, not an error.)
pub async fn lookup(State(app): State<AppState>, Query(params): Query<LookupParams>) -> Response {
    let found = app.artwork.find(params.artist.as_deref(), &params.album, params.context.as_deref()).await;
    let max_age = if found.is_some() { "private, max-age=86400" } else { "private, max-age=3600" };
    ([(header::CACHE_CONTROL, max_age)], Json(found)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ImageParams {
    src: String,
}

/// `GET /api/v1/artwork/image?src=…` — fetch and cache an image from an allowed CDN.
pub async fn image(State(app): State<AppState>, Query(params): Query<ImageParams>) -> Response {
    if !is_allowed_image(&params.src) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let service = &app.artwork;
    let cached = service.images.lock().unwrap_or_else(PoisonError::into_inner).entries.get(&params.src).cloned();
    let (bytes, content_type) = match cached {
        Some(hit) => hit,
        None => match fetch_image(&service.http, &params.src).await {
            Some(fresh) => {
                let mut cache = service.images.lock().unwrap_or_else(PoisonError::into_inner);
                if cache.bytes + fresh.0.len() > MAX_CACHED_IMAGE_BYTES {
                    *cache = ImageCache::default();
                }
                cache.bytes += fresh.0.len();
                cache.entries.insert(params.src.clone(), fresh.clone());
                fresh
            }
            None => return StatusCode::BAD_GATEWAY.into_response(),
        },
    };
    (
        [(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "public, max-age=604800, immutable".to_owned())],
        bytes,
    )
        .into_response()
}

async fn fetch_image(http: &reqwest::Client, src: &str) -> Option<(Bytes, String)> {
    let response = http.get(src).send().await.ok()?.error_for_status().ok()?;
    let content_type = response.headers().get(header::CONTENT_TYPE)?.to_str().ok()?.to_owned();
    if !content_type.starts_with("image/") {
        return None;
    }
    if response.content_length().is_some_and(|len| len > MAX_IMAGE_BYTES as u64) {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() <= MAX_IMAGE_BYTES).then_some((bytes, content_type))
}

fn is_allowed_image(src: &str) -> bool {
    Url::parse(src).is_ok_and(|url| {
        url.scheme() == "https"
            && url.port().is_none()
            && url.username().is_empty()
            && url.host_str().is_some_and(|host| ALLOWED_IMAGE_HOSTS.contains(&host))
    })
}

/// Turn a Soulseek folder title and parent into a plausible artist and album.
pub(crate) fn clean_names(artist: Option<&str>, album: &str) -> (Option<String>, String) {
    let generic = |s: &str| {
        let n = normalize(s);
        n.is_empty()
            || [
                "music",
                "album",
                "albums",
                "flac",
                "mp3",
                "downloads",
                "download",
                "complete",
                "completed",
                "soulseek",
                "shared",
                "various",
                "va",
                "failedimports",
                "unsorted",
                "incoming",
                "new",
                "misc",
                "lossless",
            ]
            .contains(&n.as_str())
    };
    let mut artist = artist.filter(|a| !generic(a)).map(|a| strip_brackets(a).trim().to_owned());
    let mut album = strip_brackets(album);

    // "CD1 - Kid A" / "Disc 2 - Amnesiac"
    if let Some((head, tail)) = album.split_once(" - ")
        && is_disc_label(head)
    {
        album = tail.to_owned();
    }

    // "Album - 1997"
    if let Some((head, tail)) = album.rsplit_once(" - ")
        && tail.trim().len() == 4
        && tail.trim().chars().all(|c| c.is_ascii_digit())
    {
        album = head.to_owned();
    }

    // "Artist - Album" folder names, with or without a matching parent folder.
    if let Some((head, tail)) = album.split_once(" - ") {
        let head_is_year = head.trim().len() == 4 && head.trim().chars().all(|c| c.is_ascii_digit());
        // Otherwise the folder names its own artist, which beats a parent folder that
        // is often just a category like "failed_imports".
        if !head_is_year {
            artist = Some(head.trim().to_owned());
        }
        album = tail.to_owned();
    }

    // "Pink Floyd - 1973 - The Dark Side Of The Moon": a year left over after the artist.
    if let Some((head, tail)) = album.split_once(" - ")
        && head.trim().len() == 4
        && head.trim().chars().all(|c| c.is_ascii_digit())
    {
        album = tail.to_owned();
    }

    // "1998 Music Has the Right to Children": a release year glued to the front.
    if let Some((head, tail)) = album.trim_start().split_once(' ')
        && head.parse::<u16>().is_ok_and(|year| (1950..=2035).contains(&year))
        && tail.chars().any(char::is_alphabetic)
    {
        album = tail.to_owned();
    }

    (artist, album.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn is_disc_label(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    ["cd", "disc", "disk"].iter().any(|p| {
        lower.strip_prefix(p).is_some_and(|rest| {
            let rest = rest.trim();
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        })
    })
}

/// Remove `(…)`, `[…]` and `{…}` groups: years, formats, catalogue numbers, editions.
fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Lowercase letters and digits only, with accents folded for the common cases.
pub(crate) fn normalize(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

pub(crate) fn names_match(a: &str, b: &str) -> bool {
    let strip_the = |s: &str| s.strip_prefix("the").map_or_else(|| s.to_owned(), str::to_owned);
    !a.is_empty() && (a == b || strip_the(a) == strip_the(b))
}

/// Construct the service used by the server.
#[must_use]
pub fn service() -> Arc<ArtworkService> {
    Arc::new(ArtworkService::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(artist: Option<&str>, album: &str) -> (Option<String>, String) {
        clean_names(artist, album)
    }

    #[test]
    fn folder_names_beat_category_parents() {
        assert_eq!(
            clean(Some("failed_imports"), "Pink Floyd - The Dark Side of the Moon (1973)"),
            (Some("Pink Floyd".into()), "The Dark Side of the Moon".into())
        );
        assert_eq!(
            clean(Some("Album"), "The Dark Side of the Moon (1973)"),
            (None, "The Dark Side of the Moon".into())
        );
    }

    #[test]
    fn cleans_real_folder_names() {
        assert_eq!(clean(Some("Björk"), "(1997) Homogenic"), (Some("Björk".into()), "Homogenic".into()));
        assert_eq!(clean(Some("Radiohead"), "Kid A (2000) (CD 01)"), (Some("Radiohead".into()), "Kid A".into()));
        assert_eq!(
            clean(Some("Radiohead - KID A MNESIA (2021) [CD FLAC]"), "CD1 - Kid A"),
            (Some("Radiohead - KID A MNESIA".into()), "Kid A".into())
        );
        assert_eq!(
            clean(Some("Music"), "Boards of Canada - Geogaddi (2002) [FLAC] {Japanese BRC-51}"),
            (Some("Boards of Canada".into()), "Geogaddi".into())
        );
        assert_eq!(clean(Some("Radiohead"), "2000 - Kid A"), (Some("Radiohead".into()), "Kid A".into()));
        assert_eq!(clean(Some("Björk"), "Homogenic - 1997"), (Some("Björk".into()), "Homogenic".into()));
        assert_eq!(
            clean(None, "Aphex Twin - Selected Ambient Works 85-92"),
            (Some("Aphex Twin".into()), "Selected Ambient Works 85-92".into())
        );
        assert_eq!(
            clean(Some("Boards of Canada"), "1998 Music Has The Right To Children (CD, Matador)"),
            (Some("Boards of Canada".into()), "Music Has The Right To Children".into())
        );
        assert_eq!(clean(Some("Dr. Dre"), "2001"), (Some("Dr. Dre".into()), "2001".into()));
    }

    #[test]
    fn normalizes_for_comparison() {
        assert_eq!(normalize("Björk"), "bjork");
        assert_eq!(normalize("OK Computer OKNOTOK 1997 2017"), "okcomputeroknotok19972017");
        assert!(names_match(&normalize("The Beatles"), &normalize("Beatles")));
        assert!(!names_match("", ""));
    }

    #[test]
    fn image_proxy_only_allows_known_https_hosts() {
        assert!(is_allowed_image("https://cdn-images.dzcdn.net/images/cover/abc/250x250-000000-80-0-0.jpg"));
        assert!(!is_allowed_image("http://cdn-images.dzcdn.net/images/cover/abc.jpg"), "plain http");
        assert!(!is_allowed_image("https://cdn-images.dzcdn.net.evil.com/x.jpg"), "suffix trick");
        assert!(!is_allowed_image("https://user@cdn-images.dzcdn.net/x.jpg"), "userinfo");
        assert!(!is_allowed_image("https://cdn-images.dzcdn.net:8443/x.jpg"), "custom port");
        assert!(!is_allowed_image("https://169.254.169.254/latest/meta-data"), "metadata service");
        assert!(!is_allowed_image("file:///etc/passwd"));
    }

    #[test]
    fn proxy_urls_are_encoded() {
        assert_eq!(
            proxy_url("https://cdn-images.dzcdn.net/a b.jpg?x=1&y=2"),
            "/api/v1/artwork/image?src=https%3A%2F%2Fcdn-images.dzcdn.net%2Fa+b.jpg%3Fx%3D1%26y%3D2"
        );
    }
}
