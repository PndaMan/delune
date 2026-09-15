//! From a parsed link to the release it names, over HTTP.
//!
//! Every service gets the cheapest source that works without an account:
//!
//! | Service        | Source                                                  |
//! |----------------|---------------------------------------------------------|
//! | Deezer         | public API (`api.deezer.com`)                           |
//! | Apple Music    | iTunes lookup API                                       |
//! | Spotify        | the embed page's JSON payload (the Web API needs keys)  |
//! | YouTube Music  | oEmbed, plus the album's first video for the artist     |
//! | SoundCloud     | oEmbed                                                  |
//! | Bandcamp       | JSON-LD on the release page                             |
//! | Tidal          | JSON-LD or Open Graph tags on the public page           |
//! | Qobuz          | Open Graph tags on the public page                      |
//! | MusicBrainz    | web service (`/ws/2`), with its required user agent     |
//!
//! Short links are expanded one redirect at a time, and only while each hop is still
//! a link we recognise, so a pasted URL can't steer requests anywhere else. Results
//! are cached for an hour: the same link pasted twice resolves once.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use delune_core::api::{ResolvedLink, ResolvedTrack};
use delune_core::{EntityKind, Provider};
use reqwest::header::{ACCEPT_LANGUAGE, LOCATION, USER_AGENT};
use serde_json::Value;

use crate::html;
use crate::link::{Link, Parsed, parse};
use crate::matching;
use crate::query::{search_query, split_video_title};

/// Pages are fetched as a browser would; some services serve bots an empty shell.
const BROWSER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";
/// MusicBrainz asks every client to identify itself.
const MUSICBRAINZ_AGENT: &str = concat!("delune/", env!("CARGO_PKG_VERSION"), " ( https://github.com/PndaMan/delune )");
const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const CACHE_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("couldn't reach {provider}: {source}")]
    Http {
        provider: Provider,
        #[source]
        source: reqwest::Error,
    },
    #[error("{provider} doesn't have anything at that link")]
    NotFound { provider: Provider },
    #[error("couldn't read what {provider} sent back")]
    Unreadable { provider: Provider },
    #[error("{0}")]
    Unsupported(String),
}

pub struct Resolver {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, ResolvedLink)>>,
    /// When MusicBrainz was last asked, to keep to its rate limit.
    musicbrainz_gate: tokio::sync::Mutex<Option<Instant>>,
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver").finish_non_exhaustive()
    }
}

impl Resolver {
    #[must_use]
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .connect_timeout(Duration::from_secs(6))
            // Redirects are followed by hand, see `expand`.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self { http, cache: Mutex::default(), musicbrainz_gate: tokio::sync::Mutex::new(None) }
    }

    /// Resolve a link, expanding short links first.
    ///
    /// # Errors
    ///
    /// When the service can't be reached, has nothing at that address, or points at
    /// something delune can't search for.
    pub async fn resolve(&self, parsed: &Parsed) -> Result<ResolvedLink, ResolveError> {
        let link = match parsed {
            Parsed::Link(link) => link.clone(),
            Parsed::ShortLink { provider, url } => self.expand(*provider, url).await?,
        };
        if !safe_id(&link.id) {
            return Err(ResolveError::NotFound { provider: link.provider });
        }

        let key = format!("{:?}/{:?}/{}", link.provider, link.kind, link.id);
        if let Some(hit) = self.cached(&key) {
            return Ok(hit);
        }
        let resolved = self.fetch(&link).await?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() > 1_000 {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_FOR);
        }
        cache.insert(key, (Instant::now(), resolved.clone()));
        Ok(resolved)
    }

    /// The link with what MusicBrainz adds (the original year, a track's album), or
    /// `None` when MusicBrainz has no confident match. Slower than [`Self::resolve`],
    /// since MusicBrainz takes one request a second, so callers needn't wait for it.
    pub async fn match_musicbrainz(&self, link: &ResolvedLink) -> Option<ResolvedLink> {
        if link.musicbrainz.is_some() {
            return Some(link.clone());
        }
        let key = format!(
            "musicbrainz/{:?}/{:?}/{}/{}",
            link.provider,
            link.kind,
            link.title,
            link.artist.as_deref().unwrap_or("")
        );
        if let Some(hit) = self.cached(&key) {
            return Some(hit);
        }
        let mut enriched = link.clone();
        self.find_on_musicbrainz(&mut enriched).await;
        enriched.musicbrainz.as_ref()?;
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).insert(key, (Instant::now(), enriched.clone()));
        Some(enriched)
    }

    async fn find_on_musicbrainz(&self, link: &mut ResolvedLink) {
        if link.provider == Provider::MusicBrainz || !matches!(link.kind, EntityKind::Album | EntityKind::Track) {
            return;
        }
        let base = "https://musicbrainz.org/ws/2";
        let found = if let Some(upc) = link.upc.clone().filter(|u| matching::valid_barcode(u)) {
            let url = format!("{base}/release?fmt=json&limit=5&query={}", urlencode(&format!("barcode:{upc}")));
            let mut found = self.musicbrainz(&url).await.as_ref().and_then(matching::release_by_barcode);
            if let Some(found) = &mut found {
                let url = format!("{base}/release-group/{}?fmt=json", found.release_group_id);
                if let Some(year) = self.musicbrainz(&url).await.as_ref().and_then(matching::first_release_year) {
                    found.original_year = Some(year);
                }
            }
            found
        } else if let Some(isrc) = link.isrc.clone().filter(|i| matching::valid_isrc(i)) {
            let url = format!("{base}/recording?fmt=json&limit=5&query={}", urlencode(&format!("isrc:{isrc}")));
            self.musicbrainz(&url).await.as_ref().and_then(matching::album_by_isrc)
        } else if let (EntityKind::Album, Some(artist)) = (link.kind, link.artist.clone()) {
            let query = format!(
                "releasegroup:{} AND artist:{}",
                matching::phrase(&crate::query::clean_title(&link.title)),
                matching::phrase(&artist)
            );
            let url = format!("{base}/release-group?fmt=json&limit=5&query={}", urlencode(&query));
            self.musicbrainz(&url).await.as_ref().and_then(|json| matching::album_by_name(json, &link.title, &artist))
        } else {
            None
        };
        if let Some(found) = found {
            matching::apply(link, found);
        }
    }

    /// A MusicBrainz request, waiting its turn under the rate limit.
    async fn musicbrainz(&self, url: &str) -> Option<Value> {
        let mut last = self.musicbrainz_gate.lock().await;
        if let Some(wait) = last.and_then(|at| matching::MUSICBRAINZ_GAP.checked_sub(at.elapsed())) {
            tokio::time::sleep(wait).await;
        }
        let mut result = self.json(Provider::MusicBrainz, url, MUSICBRAINZ_AGENT).await;
        // A 503 means we were too quick after all (someone else on this address, perhaps).
        if matches!(&result, Err(ResolveError::Http { source, .. }) if source.status() == Some(reqwest::StatusCode::SERVICE_UNAVAILABLE))
        {
            tokio::time::sleep(matching::MUSICBRAINZ_GAP * 2).await;
            result = self.json(Provider::MusicBrainz, url, MUSICBRAINZ_AGENT).await;
        }
        *last = Some(Instant::now());
        result.ok()
    }

    fn cached(&self, key: &str) -> Option<ResolvedLink> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.get(key).filter(|(at, _)| at.elapsed() < CACHE_FOR).map(|(_, r)| r.clone())
    }

    /// Follow a short link's redirects until they land on a link we understand.
    async fn expand(&self, provider: Provider, url: &str) -> Result<Link, ResolveError> {
        let mut current = url.to_owned();
        for _ in 0..MAX_REDIRECTS {
            let response = self
                .http
                .get(&current)
                .header(USER_AGENT, BROWSER_AGENT)
                .send()
                .await
                .map_err(|source| ResolveError::Http { provider, source })?;
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|l| l.to_str().ok())
                .and_then(|l| response.url().join(l).ok())
                .ok_or(ResolveError::NotFound { provider })?;
            match parse(location.as_str()) {
                Ok(Parsed::Link(link)) => return Ok(link),
                Ok(Parsed::ShortLink { url, .. }) => current = url,
                // Deezer's short links bounce through an app-install page carrying the
                // real address; anything else unrecognised stops here.
                Err(_) => return Err(ResolveError::NotFound { provider }),
            }
        }
        Err(ResolveError::NotFound { provider })
    }

    async fn fetch(&self, link: &Link) -> Result<ResolvedLink, ResolveError> {
        let id = link.id.as_str();
        match (link.provider, link.kind) {
            (Provider::Deezer, kind) => {
                let path = match kind {
                    EntityKind::Album => "album",
                    EntityKind::Track => "track",
                    EntityKind::Artist => "artist",
                    EntityKind::Playlist => "playlist",
                };
                let json =
                    self.json(link.provider, &format!("https://api.deezer.com/{path}/{id}"), BROWSER_AGENT).await?;
                deezer(kind, &json)
            }
            (Provider::AppleMusic, kind) => {
                let url = match kind {
                    EntityKind::Album => format!("https://itunes.apple.com/lookup?id={id}&entity=song"),
                    EntityKind::Track | EntityKind::Artist => format!("https://itunes.apple.com/lookup?id={id}"),
                    EntityKind::Playlist => return Err(unsupported_playlist(link.provider)),
                };
                apple(kind, &self.json(link.provider, &url, BROWSER_AGENT).await?)
            }
            (Provider::Spotify, kind) => {
                let path = kind_path(kind);
                let page = self.text(link.provider, &format!("https://open.spotify.com/embed/{path}/{id}")).await?;
                spotify(kind, &page)
            }
            (Provider::YoutubeMusic, kind) => self.youtube(kind, id).await,
            (Provider::SoundCloud, kind) => {
                let url = format!("https://soundcloud.com/oembed?format=json&url=https://soundcloud.com/{id}");
                soundcloud(kind, &self.json(link.provider, &url, BROWSER_AGENT).await?)
            }
            (Provider::Bandcamp, kind) => {
                let (artist, slug) = id.split_once('/').unwrap_or((id, ""));
                let url = match kind {
                    EntityKind::Album => format!("https://{artist}.bandcamp.com/album/{slug}"),
                    EntityKind::Track => format!("https://{artist}.bandcamp.com/track/{slug}"),
                    _ => format!("https://{artist}.bandcamp.com/"),
                };
                page_metadata(link.provider, kind, &self.text(link.provider, &url).await?)
            }
            (Provider::Tidal, kind) => {
                let url = format!("https://tidal.com/{}/{id}", kind_path(kind));
                page_metadata(link.provider, kind, &self.text(link.provider, &url).await?)
            }
            (Provider::Qobuz, kind) => {
                let url = format!("https://www.qobuz.com/us-en/{}/-/{id}", kind_path(kind));
                page_metadata(link.provider, kind, &self.text(link.provider, &url).await?)
            }
            (Provider::MusicBrainz, kind) => {
                let (entity, _) = id.split_once('/').unwrap_or((id, ""));
                let inc = match entity {
                    "release" => "artist-credits+recordings",
                    "release-group" | "recording" => "artist-credits+releases",
                    _ => "",
                };
                let url = format!("https://musicbrainz.org/ws/2/{id}?fmt=json&inc={inc}");
                musicbrainz(kind, entity, &self.json(link.provider, &url, MUSICBRAINZ_AGENT).await?)
            }
            (Provider::Soulseek, _) => Err(ResolveError::Unsupported("Soulseek links can't be resolved".into())),
        }
    }

    async fn youtube(&self, kind: EntityKind, id: &str) -> Result<ResolvedLink, ResolveError> {
        let provider = Provider::YoutubeMusic;
        match kind {
            EntityKind::Track => {
                let json = self.oembed(&format!("https://www.youtube.com/watch?v={id}")).await?;
                let (artist, title) = split_video_title(
                    str_at(&json, "/title").unwrap_or_default(),
                    str_at(&json, "/author_name").unwrap_or_default(),
                );
                Ok(finish(provider, kind, title, Some(artist), None, None, vec![]))
            }
            EntityKind::Album | EntityKind::Playlist => {
                let json = self.oembed(&format!("https://www.youtube.com/playlist?list={id}")).await?;
                let raw = str_at(&json, "/title").ok_or(ResolveError::NotFound { provider })?;
                let title = raw
                    .strip_prefix("Album - ")
                    .or_else(|| raw.strip_prefix("EP - "))
                    .or_else(|| raw.strip_prefix("Single - "));
                let is_album = title.is_some();
                let title = title.unwrap_or(raw).to_owned();
                if !is_album {
                    return Err(unsupported_playlist(provider));
                }
                // Album playlists don't name the artist, but their tracks come from the
                // artist's "Topic" channel. The thumbnail is the first track's video.
                let video = str_at(&json, "/thumbnail_url")
                    .and_then(|t| t.split("/vi/").nth(1))
                    .and_then(|t| t.split('/').next());
                let mut artist = None;
                if let Some(video) = video.filter(|v| safe_id(v))
                    && let Ok(first) = self.oembed(&format!("https://www.youtube.com/watch?v={video}")).await
                {
                    artist = str_at(&first, "/author_name").map(|a| a.trim_end_matches(" - Topic").to_owned());
                }
                Ok(finish(provider, EntityKind::Album, title, artist, None, None, vec![]))
            }
            EntityKind::Artist => {
                let page = self.text(provider, &format!("https://www.youtube.com/channel/{id}")).await?;
                let name = html::meta(&page, "og:title").ok_or(ResolveError::NotFound { provider })?;
                let name = name.trim_end_matches(" - Topic").to_owned();
                Ok(finish(provider, kind, name, None, None, None, vec![]))
            }
        }
    }

    async fn oembed(&self, target: &str) -> Result<Value, ResolveError> {
        let url = format!("https://www.youtube.com/oembed?format=json&url={}", urlencode(target));
        self.json(Provider::YoutubeMusic, &url, BROWSER_AGENT).await
    }

    async fn text(&self, provider: Provider, url: &str) -> Result<String, ResolveError> {
        let mut current = reqwest::Url::parse(url).map_err(|_| ResolveError::NotFound { provider })?;
        let mut hops = 0;
        let mut response = loop {
            let response = self
                .http
                .get(current.clone())
                .header(USER_AGENT, BROWSER_AGENT)
                .header(ACCEPT_LANGUAGE, "en")
                .send()
                .await
                .map_err(|source| ResolveError::Http { provider, source })?;
            // Pages redirect to their canonical address (Qobuz adds the slug). Follow
            // that, but never off the host we chose to ask.
            let next = response
                .status()
                .is_redirection()
                .then(|| response.headers().get(LOCATION)?.to_str().ok())
                .flatten()
                .and_then(|location| current.join(location).ok())
                .filter(|next| next.host_str() == current.host_str() && next.scheme() == "https");
            match next {
                Some(next) if hops < MAX_REDIRECTS => {
                    hops += 1;
                    current = next;
                }
                _ => break response,
            }
        };
        if response.status().is_client_error() || response.status().is_redirection() {
            return Err(ResolveError::NotFound { provider });
        }
        if let Err(source) = response.error_for_status_ref() {
            return Err(ResolveError::Http { provider, source });
        }
        // Stop reading at a sane size: these are release pages, not downloads.
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|source| ResolveError::Http { provider, source })? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_BODY {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    async fn json(&self, provider: Provider, url: &str, agent: &str) -> Result<Value, ResolveError> {
        let response = self
            .http
            .get(url)
            .header(USER_AGENT, agent)
            .send()
            .await
            .map_err(|source| ResolveError::Http { provider, source })?;
        if response.status().is_client_error() {
            return Err(ResolveError::NotFound { provider });
        }
        if let Err(source) = response.error_for_status_ref() {
            return Err(ResolveError::Http { provider, source });
        }
        let value: Value = response.json().await.map_err(|_| ResolveError::Unreadable { provider })?;
        // Deezer reports missing things as `200 {"error": …}`.
        if value.get("error").is_some() {
            return Err(ResolveError::NotFound { provider });
        }
        Ok(value)
    }
}

/// IDs end up in URL paths, so only characters IDs actually use are allowed.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'))
        && !id.split('/').any(|part| part.is_empty() || part == "." || part == "..")
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

const fn kind_path(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Album => "album",
        EntityKind::Track => "track",
        EntityKind::Artist => "artist",
        EntityKind::Playlist => "playlist",
    }
}

fn unsupported_playlist(provider: Provider) -> ResolveError {
    ResolveError::Unsupported(format!(
        "{provider} playlists can't be searched yet. Paste a link to an album or a track instead."
    ))
}

fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn year_of(date: Option<&str>) -> Option<u16> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()).filter(|y| (1000..3000).contains(y))
}

fn finish(
    provider: Provider,
    kind: EntityKind,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    year: Option<u16>,
    tracks: Vec<ResolvedTrack>,
) -> ResolvedLink {
    let query = match kind {
        EntityKind::Artist => search_query(None, &title),
        _ => search_query(artist.as_deref(), &title),
    };
    ResolvedLink { provider, kind, title, artist, album, year, tracks, query, upc: None, isrc: None, musicbrainz: None }
}

fn deezer(kind: EntityKind, json: &Value) -> Result<ResolvedLink, ResolveError> {
    let provider = Provider::Deezer;
    let tracks = |json: &Value| {
        json.pointer("/tracks/data").and_then(Value::as_array).map_or_else(Vec::new, |items| {
            items
                .iter()
                .filter_map(|t| {
                    Some(ResolvedTrack {
                        album: str_at(t, "/album/title").map(str::to_owned),
                        title: str_at(t, "/title")?.to_owned(),
                        artist: str_at(t, "/artist/name").map(str::to_owned),
                        duration_secs: t.get("duration").and_then(Value::as_u64).and_then(|d| u32::try_from(d).ok()),
                    })
                })
                .collect()
        })
    };
    let artist = str_at(json, "/artist/name").map(str::to_owned);
    Ok(match kind {
        EntityKind::Album => {
            let title = str_at(json, "/title").ok_or(ResolveError::Unreadable { provider })?.to_owned();
            let mut link =
                finish(provider, kind, title, artist, None, year_of(str_at(json, "/release_date")), tracks(json));
            link.upc = str_at(json, "/upc").map(str::to_owned);
            link
        }
        EntityKind::Track => {
            let title = str_at(json, "/title").ok_or(ResolveError::Unreadable { provider })?.to_owned();
            let album = str_at(json, "/album/title").map(str::to_owned);
            let mut link = finish(provider, kind, title, artist, album, year_of(str_at(json, "/release_date")), vec![]);
            link.isrc = str_at(json, "/isrc").map(str::to_owned);
            link
        }
        EntityKind::Artist => {
            let name = str_at(json, "/name").ok_or(ResolveError::Unreadable { provider })?.to_owned();
            finish(provider, kind, name, None, None, None, vec![])
        }
        EntityKind::Playlist => {
            let title = str_at(json, "/title").ok_or(ResolveError::Unreadable { provider })?.to_owned();
            let owner = str_at(json, "/creator/name").map(str::to_owned);
            finish(provider, kind, title, owner, None, None, tracks(json))
        }
    })
}

fn apple(kind: EntityKind, json: &Value) -> Result<ResolvedLink, ResolveError> {
    let provider = Provider::AppleMusic;
    let results = json.get("results").and_then(Value::as_array).ok_or(ResolveError::Unreadable { provider })?;
    let first = results.first().ok_or(ResolveError::NotFound { provider })?;
    let artist = str_at(first, "/artistName").map(str::to_owned);
    let year = year_of(str_at(first, "/releaseDate"));
    Ok(match kind {
        EntityKind::Album => {
            let title = str_at(first, "/collectionName").ok_or(ResolveError::Unreadable { provider })?.to_owned();
            let tracks = results
                .iter()
                .filter(|r| str_at(r, "/wrapperType") == Some("track"))
                .filter_map(|t| {
                    Some(ResolvedTrack {
                        album: None,
                        title: str_at(t, "/trackName")?.to_owned(),
                        artist: str_at(t, "/artistName").map(str::to_owned),
                        duration_secs: t
                            .get("trackTimeMillis")
                            .and_then(Value::as_u64)
                            .and_then(|ms| u32::try_from(ms / 1000).ok()),
                    })
                })
                .collect();
            finish(provider, kind, title, artist, None, year, tracks)
        }
        EntityKind::Track => {
            let title = str_at(first, "/trackName").ok_or(ResolveError::Unreadable { provider })?.to_owned();
            let album = str_at(first, "/collectionName").map(str::to_owned);
            finish(provider, kind, title, artist, album, year, vec![])
        }
        EntityKind::Artist => {
            let name = artist.ok_or(ResolveError::Unreadable { provider })?;
            finish(provider, kind, name, None, None, None, vec![])
        }
        EntityKind::Playlist => return Err(unsupported_playlist(provider)),
    })
}

fn spotify(kind: EntityKind, page: &str) -> Result<ResolvedLink, ResolveError> {
    let provider = Provider::Spotify;
    let payload = html::scripts(page, "id", "__NEXT_DATA__").next().ok_or(ResolveError::NotFound { provider })?;
    let data: Value = serde_json::from_str(payload).map_err(|_| ResolveError::Unreadable { provider })?;
    let entity = data.pointer("/props/pageProps/state/data/entity").ok_or(ResolveError::NotFound { provider })?;
    let title = str_at(entity, "/name")
        .or_else(|| str_at(entity, "/title"))
        .ok_or(ResolveError::Unreadable { provider })?
        .to_owned();
    let artists = entity
        .get("artists")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| str_at(x, "/name")).collect::<Vec<_>>().join(", "));
    let artist = artists.filter(|a| !a.is_empty()).or_else(|| str_at(entity, "/subtitle").map(str::to_owned));
    let year = year_of(str_at(entity, "/releaseDate/isoString"));
    let tracks = entity.get("trackList").and_then(Value::as_array).map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|t| {
                Some(ResolvedTrack {
                    album: None,
                    title: str_at(t, "/title")?.to_owned(),
                    artist: str_at(t, "/subtitle").map(str::to_owned),
                    duration_secs: t
                        .get("duration")
                        .and_then(Value::as_u64)
                        .and_then(|ms| u32::try_from(ms / 1000).ok()),
                })
            })
            .collect()
    });
    Ok(match kind {
        EntityKind::Playlist => finish(provider, kind, title, artist, None, None, tracks),
        EntityKind::Artist => finish(provider, kind, title, None, None, None, vec![]),
        _ => finish(provider, kind, title, artist, None, year, tracks),
    })
}

fn soundcloud(kind: EntityKind, json: &Value) -> Result<ResolvedLink, ResolveError> {
    let provider = Provider::SoundCloud;
    let author = str_at(json, "/author_name").map(str::to_owned);
    let raw = str_at(json, "/title").ok_or(ResolveError::Unreadable { provider })?;
    // "Flickermood by Forss"
    let title = author.as_deref().and_then(|a| raw.strip_suffix(&format!(" by {a}"))).unwrap_or(raw).to_owned();
    Ok(match kind {
        EntityKind::Artist => finish(provider, kind, author.unwrap_or(title), None, None, None, vec![]),
        // SoundCloud "sets" are usually albums and EPs.
        EntityKind::Playlist | EntityKind::Album => {
            finish(provider, EntityKind::Album, title, author, None, None, vec![])
        }
        EntityKind::Track => finish(provider, kind, title, author, None, None, vec![]),
    })
}

/// JSON-LD first (Bandcamp, Tidal), then Open Graph tags (Qobuz, and anything else).
fn page_metadata(provider: Provider, kind: EntityKind, page: &str) -> Result<ResolvedLink, ResolveError> {
    for block in html::scripts(page, "type", "application/ld+json") {
        let Ok(value) = serde_json::from_str::<Value>(block) else { continue };
        let items = match &value {
            Value::Array(items) => items.clone(),
            other => other.get("@graph").and_then(Value::as_array).cloned().unwrap_or_else(|| vec![other.clone()]),
        };
        for item in &items {
            if let Some(resolved) = from_json_ld(provider, kind, item) {
                return Ok(resolved);
            }
        }
    }

    let og_title = html::meta(page, "og:title").ok_or(ResolveError::NotFound { provider })?;
    let description = html::meta(page, "og:description").unwrap_or_default();
    let trimmed = og_title.trim_end_matches(" - Qobuz").trim_end_matches(" on TIDAL").trim();

    // Qobuz: "…download OK Computer by Radiohead in Hi-Res…"
    if let Some(rest) = description.split_once("download ").map(|(_, r)| r)
        && let Some((title, rest)) = rest.split_once(" by ")
        && let Some((artist, _)) = rest.split_once(" in ")
    {
        return Ok(finish(
            provider,
            kind,
            html::decode_entities(title),
            Some(artist.trim().to_owned()),
            None,
            None,
            vec![],
        ));
    }
    // "Artist - Title" (Tidal) or "Title, Artist" (Qobuz).
    let (artist, title) = if provider == Provider::Qobuz {
        trimmed.rsplit_once(", ").map_or((None, trimmed), |(t, a)| (Some(a), t))
    } else {
        trimmed.split_once(" - ").map_or((None, trimmed), |(a, t)| (Some(a), t))
    };
    if kind == EntityKind::Artist {
        return Ok(finish(provider, kind, trimmed.to_owned(), None, None, None, vec![]));
    }
    Ok(finish(provider, kind, title.to_owned(), artist.map(str::to_owned), None, None, vec![]))
}

fn from_json_ld(provider: Provider, kind: EntityKind, item: &Value) -> Option<ResolvedLink> {
    let kind_ld = str_at(item, "/@type")?;
    let name = str_at(item, "/name")?.to_owned();
    let artist = artist_of(item);
    // Without an artist the search would be too vague; let Open Graph tags try.
    if artist.is_none() && kind != EntityKind::Artist {
        return None;
    }
    let year = year_of(str_at(item, "/datePublished")).or_else(|| {
        // Bandcamp writes "13 Jun 2013 00:00:00 GMT".
        str_at(item, "/datePublished")?.split_whitespace().find_map(|w| w.parse::<u16>().ok().filter(|y| *y > 1000))
    });
    match (kind, kind_ld) {
        (EntityKind::Album, "MusicAlbum") => {
            let tracks =
                item.pointer("/track/itemListElement").and_then(Value::as_array).map_or_else(Vec::new, |list| {
                    list.iter()
                        .filter_map(|entry| {
                            let track = entry.get("item").unwrap_or(entry);
                            Some(ResolvedTrack {
                                album: None,
                                title: str_at(track, "/name")?.to_owned(),
                                artist: artist_of(track),
                                duration_secs: str_at(track, "/duration").and_then(html::iso_duration_secs),
                            })
                        })
                        .collect()
                });
            Some(finish(provider, kind, name, artist, None, year, tracks))
        }
        (EntityKind::Track, "MusicRecording") => {
            let album = str_at(item, "/inAlbum/name").map(str::to_owned);
            Some(finish(provider, kind, name, artist, album, year, vec![]))
        }
        (EntityKind::Artist, "MusicGroup" | "Person") => Some(finish(provider, kind, name, None, None, None, vec![])),
        _ => None,
    }
}

/// `byArtist` is an object, a list of objects, or occasionally a bare name.
fn artist_of(item: &Value) -> Option<String> {
    match item.get("byArtist")? {
        Value::String(name) => Some(name.trim().to_owned()).filter(|n| !n.is_empty()),
        Value::Array(list) => {
            let names: Vec<&str> = list.iter().filter_map(|a| str_at(a, "/name")).collect();
            (!names.is_empty()).then(|| names.join(", "))
        }
        other => str_at(other, "/name").map(str::to_owned),
    }
}

fn musicbrainz(kind: EntityKind, entity: &str, json: &Value) -> Result<ResolvedLink, ResolveError> {
    let provider = Provider::MusicBrainz;
    let credit = json.get("artist-credit").and_then(Value::as_array).map(|credits| {
        let mut names = String::new();
        for credit in credits {
            names.push_str(str_at(credit, "/name").unwrap_or_default());
            names.push_str(credit.get("joinphrase").and_then(Value::as_str).unwrap_or_default());
        }
        names
    });
    let title = str_at(json, "/title")
        .or_else(|| str_at(json, "/name"))
        .ok_or(ResolveError::Unreadable { provider })?
        .to_owned();
    Ok(match (kind, entity) {
        (EntityKind::Album, "release") => {
            let tracks = json.get("media").and_then(Value::as_array).map_or_else(Vec::new, |media| {
                media
                    .iter()
                    .filter_map(|m| m.get("tracks").and_then(Value::as_array))
                    .flatten()
                    .filter_map(|t| {
                        Some(ResolvedTrack {
                            album: None,
                            title: str_at(t, "/title")?.to_owned(),
                            artist: None,
                            duration_secs: t
                                .get("length")
                                .and_then(Value::as_u64)
                                .and_then(|ms| u32::try_from(ms / 1000).ok()),
                        })
                    })
                    .collect()
            });
            finish(provider, kind, title, credit, None, year_of(str_at(json, "/date")), tracks)
        }
        (EntityKind::Album, _) => {
            finish(provider, kind, title, credit, None, year_of(str_at(json, "/first-release-date")), vec![])
        }
        (EntityKind::Track, _) => {
            let album = str_at(json, "/releases/0/title").map(str::to_owned);
            finish(provider, kind, title, credit, album, year_of(str_at(json, "/first-release-date")), vec![])
        }
        (EntityKind::Artist, _) => finish(provider, kind, title, None, None, None, vec![]),
        (EntityKind::Playlist, _) => return Err(unsupported_playlist(provider)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ids_that_could_change_the_request() {
        assert!(safe_id("6dVIqQ8qmQ5GBnJ9shOYGE"));
        assert!(safe_id("boardsofcanada/tomorrows-harvest"));
        assert!(safe_id("release/b84ee12a-09ef-421b-82de-0441a926375b"));
        assert!(!safe_id("../../admin"));
        assert!(!safe_id("123?x=1"));
        assert!(!safe_id("a//b"));
        assert!(!safe_id(""));
    }

    #[test]
    fn reads_spotify_embed_pages() {
        let page = r#"<html><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{
            "type":"album","name":"OK Computer","subtitle":"Radiohead","releaseDate":null,
            "trackList":[{"title":"Airbag","subtitle":"Radiohead","duration":287880},{"title":"Paranoid Android","subtitle":"Radiohead","duration":387213}]
        }}}}}}</script></html>"#;
        let r = spotify(EntityKind::Album, page).unwrap();
        assert_eq!((r.title.as_str(), r.artist.as_deref()), ("OK Computer", Some("Radiohead")));
        assert_eq!(r.tracks.len(), 2);
        assert_eq!(r.tracks[0].duration_secs, Some(287));
        assert_eq!(r.query, "Radiohead OK Computer");
    }

    #[test]
    fn reads_deezer_albums_and_tracks() {
        let album: Value = serde_json::from_str(
            r#"{"title":"Discovery","artist":{"name":"Daft Punk"},"release_date":"2001-03-07",
                "tracks":{"data":[{"title":"One More Time","duration":320,"artist":{"name":"Daft Punk"}}]}}"#,
        )
        .unwrap();
        let r = deezer(EntityKind::Album, &album).unwrap();
        assert_eq!((r.title.as_str(), r.year, r.tracks.len()), ("Discovery", Some(2001), 1));

        let track: Value = serde_json::from_str(
            r#"{"title":"Digital Love","artist":{"name":"Daft Punk"},"album":{"title":"Discovery"}}"#,
        )
        .unwrap();
        let r = deezer(EntityKind::Track, &track).unwrap();
        assert_eq!((r.album.as_deref(), r.query.as_str()), (Some("Discovery"), "Daft Punk Digital Love"));
    }

    #[test]
    fn reads_itunes_lookups() {
        let json: Value = serde_json::from_str(
            r#"{"results":[{"wrapperType":"collection","collectionName":"OK Computer","artistName":"Radiohead","releaseDate":"1997-05-21T07:00:00Z"},
                {"wrapperType":"track","trackName":"Airbag","artistName":"Radiohead","trackTimeMillis":284000}]}"#,
        )
        .unwrap();
        let r = apple(EntityKind::Album, &json).unwrap();
        assert_eq!((r.title.as_str(), r.year, r.tracks.len()), ("OK Computer", Some(1997), 1));
    }

    #[test]
    fn reads_bandcamp_and_tidal_json_ld() {
        let bandcamp = r#"<script type="application/ld+json">{"@type":"MusicAlbum","name":"Tomorrow's Harvest",
            "byArtist":{"@type":"MusicGroup","name":"Boards of Canada"},"datePublished":"10 Jun 2013 00:00:00 GMT",
            "track":{"itemListElement":[{"position":1,"item":{"@type":"MusicRecording","name":"Gemini","duration":"P00H02M52S"}}]}}</script>"#;
        let r = page_metadata(Provider::Bandcamp, EntityKind::Album, bandcamp).unwrap();
        assert_eq!(
            (r.artist.as_deref(), r.year, r.tracks[0].duration_secs),
            (Some("Boards of Canada"), Some(2013), Some(172))
        );

        let tidal = r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"MusicRecording","name":"Paper Tiger",
            "byArtist":{"@type":"MusicGroup","name":"Beck"},"inAlbum":{"@type":"MusicAlbum","name":"Sea Change"}}</script>"#;
        let r = page_metadata(Provider::Tidal, EntityKind::Track, tidal).unwrap();
        assert_eq!(
            (r.title.as_str(), r.album.as_deref(), r.query.as_str()),
            ("Paper Tiger", Some("Sea Change"), "Beck Paper Tiger")
        );
    }

    #[test]
    fn reads_qobuz_open_graph() {
        let page = r#"<meta property="og:title" content="OK Computer, Radiohead - Qobuz">
            <meta property="og:description" content="Listen to unlimited streaming or download OK Computer by Radiohead in Hi-Res quality on Qobuz.">"#;
        let r = page_metadata(Provider::Qobuz, EntityKind::Album, page).unwrap();
        assert_eq!((r.title.as_str(), r.artist.as_deref()), ("OK Computer", Some("Radiohead")));
    }

    #[test]
    fn reads_soundcloud_and_musicbrainz() {
        let json: Value = serde_json::from_str(r#"{"title":"Flickermood by Forss","author_name":"Forss"}"#).unwrap();
        assert_eq!(soundcloud(EntityKind::Track, &json).unwrap().query, "Forss Flickermood");

        let json: Value = serde_json::from_str(
            r#"{"title":"The Dark Side of the Moon","date":"1973-03-24","artist-credit":[{"name":"Pink Floyd","joinphrase":""}],
                "media":[{"tracks":[{"title":"Speak to Me","length":67000}]}]}"#,
        )
        .unwrap();
        let r = musicbrainz(EntityKind::Album, "release", &json).unwrap();
        assert_eq!((r.artist.as_deref(), r.year, r.tracks.len()), (Some("Pink Floyd"), Some(1973), 1));
    }
}
