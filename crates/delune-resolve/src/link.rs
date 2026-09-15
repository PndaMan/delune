//! Turning whatever the user pasted into a structured [`Link`].
//!
//! This is the first thing that runs when someone pastes into the search bar, so it
//! is strict about what it accepts but forgiving about how it's written: missing
//! scheme, locale prefixes (`/intl-de/`, `/gb/`, `/en/`), tracking query strings,
//! trailing slashes and `spotify:` URIs all parse.
//!
//! Parsing is pure and offline. Short links (`spotify.link`, `deezer.page.link`,
//! `on.soundcloud.com`) can't be understood without following a redirect, so they
//! come back as [`Parsed::ShortLink`] and the resolver expands them over HTTP.

pub use delune_core::EntityKind;
use delune_core::Provider;
use serde::{Deserialize, Serialize};
use url::Url;

/// A link we fully understood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub provider: Provider,
    pub kind: EntityKind,
    /// Provider-native identifier. For Bandcamp and SoundCloud, which have no
    /// numeric IDs in URLs, this is the canonical path (e.g. `artist/album-slug`).
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Parsed {
    Link(Link),
    /// A redirecting short link that must be expanded before it can be parsed.
    ShortLink {
        provider: Provider,
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("that doesn't look like a link")]
    NotAUrl,
    #[error("{0} links aren't supported")]
    UnsupportedHost(String),
    #[error("this {provider} link doesn't point to a track, album, artist or playlist")]
    UnsupportedPath { provider: Provider },
}

/// Parse user input into a [`Parsed`] link.
///
/// # Examples
///
/// ```
/// use delune_resolve::link::{parse, EntityKind, Parsed};
/// use delune_core::Provider;
///
/// let Parsed::Link(link) = parse("https://open.spotify.com/intl-de/album/6dVIqQ8qmQ5GBnJ9shOYGE?si=abc").unwrap()
/// else { panic!() };
/// assert_eq!(link.provider, Provider::Spotify);
/// assert_eq!(link.kind, EntityKind::Album);
/// assert_eq!(link.id, "6dVIqQ8qmQ5GBnJ9shOYGE");
/// ```
pub fn parse(input: &str) -> Result<Parsed, ParseError> {
    let input = input.trim();

    if let Some(rest) = input.strip_prefix("spotify:") {
        return parse_spotify_uri(rest);
    }

    let url =
        Url::parse(input).or_else(|_| Url::parse(&format!("https://{input}"))).map_err(|_| ParseError::NotAUrl)?;
    let host = url.host_str().ok_or(ParseError::NotAUrl)?.trim_start_matches("www.").to_ascii_lowercase();
    if !host.contains('.') {
        return Err(ParseError::NotAUrl);
    }
    let segments: Vec<&str> = url.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default();

    match host.as_str() {
        "open.spotify.com" | "play.spotify.com" => spotify(&segments),
        "spotify.link" => short(Provider::Spotify, &url),
        "music.apple.com" | "itunes.apple.com" => apple(&segments, &url),
        "tidal.com" | "listen.tidal.com" => tidal(&segments),
        "deezer.com" => deezer(&segments),
        "deezer.page.link" | "link.deezer.com" => short(Provider::Deezer, &url),
        "open.qobuz.com" | "play.qobuz.com" | "qobuz.com" => qobuz(&segments),
        "music.youtube.com" | "youtube.com" | "m.youtube.com" | "youtu.be" => youtube(&host, &segments, &url),
        "soundcloud.com" | "m.soundcloud.com" => soundcloud(&segments),
        "on.soundcloud.com" => short(Provider::SoundCloud, &url),
        "musicbrainz.org" | "beta.musicbrainz.org" => musicbrainz(&segments),
        h if h.ends_with(".bandcamp.com") => bandcamp(h, &segments),
        _ => Err(ParseError::UnsupportedHost(host)),
    }
}

// Returns `Result` so every parser arm can end in the same expression type.
#[allow(clippy::unnecessary_wraps)]
fn link(provider: Provider, kind: EntityKind, id: impl Into<String>) -> Result<Parsed, ParseError> {
    Ok(Parsed::Link(Link { provider, kind, id: id.into() }))
}

#[allow(clippy::unnecessary_wraps)] // same reason as `link`
fn short(provider: Provider, url: &Url) -> Result<Parsed, ParseError> {
    Ok(Parsed::ShortLink { provider, url: url.to_string() })
}

fn kind_from(word: &str) -> Option<EntityKind> {
    Some(match word {
        "track" | "song" => EntityKind::Track,
        "album" => EntityKind::Album,
        "artist" => EntityKind::Artist,
        "playlist" => EntityKind::Playlist,
        _ => return None,
    })
}

/// Find the first `<kind>/<id>` pair in a path, skipping locale prefixes.
fn kind_and_next<'a>(segments: &[&'a str]) -> Option<(EntityKind, &'a str)> {
    segments.windows(2).find_map(|w| kind_from(w[0]).map(|k| (k, w[1])))
}

fn spotify(segments: &[&str]) -> Result<Parsed, ParseError> {
    kind_and_next(segments).map_or(Err(ParseError::UnsupportedPath { provider: Provider::Spotify }), |(k, id)| {
        link(Provider::Spotify, k, id)
    })
}

fn parse_spotify_uri(rest: &str) -> Result<Parsed, ParseError> {
    let mut parts = rest.split(':');
    match (parts.next().and_then(kind_from), parts.next()) {
        (Some(kind), Some(id)) if !id.is_empty() => link(Provider::Spotify, kind, id),
        _ => Err(ParseError::UnsupportedPath { provider: Provider::Spotify }),
    }
}

/// `music.apple.com/{cc}/album/{slug}/{id}?i={track}` — the `i` parameter turns an
/// album link into a link to one song on that album.
fn apple(segments: &[&str], url: &Url) -> Result<Parsed, ParseError> {
    let err = ParseError::UnsupportedPath { provider: Provider::AppleMusic };
    let pos = segments.iter().position(|s| kind_from(s).is_some()).ok_or_else(|| err.clone())?;
    let kind = kind_from(segments[pos]).ok_or_else(|| err.clone())?;
    // The ID is the last segment; the slug in between is optional.
    let id = segments.get(pos + 1..).and_then(<[&str]>::last).ok_or(err)?;
    if kind == EntityKind::Album
        && let Some((_, track)) = url.query_pairs().find(|(k, _)| k == "i")
    {
        return link(Provider::AppleMusic, EntityKind::Track, track.into_owned());
    }
    link(Provider::AppleMusic, kind, *id)
}

fn tidal(segments: &[&str]) -> Result<Parsed, ParseError> {
    kind_and_next(segments)
        .map_or(Err(ParseError::UnsupportedPath { provider: Provider::Tidal }), |(k, id)| link(Provider::Tidal, k, id))
}

fn deezer(segments: &[&str]) -> Result<Parsed, ParseError> {
    kind_and_next(segments).map_or(Err(ParseError::UnsupportedPath { provider: Provider::Deezer }), |(k, id)| {
        link(Provider::Deezer, k, id)
    })
}

/// `open.qobuz.com/album/{id}` or `qobuz.com/{locale}/album/{slug}/{id}`.
fn qobuz(segments: &[&str]) -> Result<Parsed, ParseError> {
    let err = ParseError::UnsupportedPath { provider: Provider::Qobuz };
    let pos = segments.iter().position(|s| kind_from(s).is_some()).ok_or_else(|| err.clone())?;
    let kind = kind_from(segments[pos]).ok_or_else(|| err.clone())?;
    let id = segments.get(pos + 1..).and_then(<[&str]>::last).ok_or(err)?;
    link(Provider::Qobuz, kind, *id)
}

fn youtube(host: &str, segments: &[&str], url: &Url) -> Result<Parsed, ParseError> {
    let p = Provider::YoutubeMusic;
    let query = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned());
    if host == "youtu.be" {
        return segments
            .first()
            .map_or(Err(ParseError::UnsupportedPath { provider: p }), |id| link(p, EntityKind::Track, *id));
    }
    match segments {
        ["watch", ..] => {
            query("v").map_or(Err(ParseError::UnsupportedPath { provider: p }), |v| link(p, EntityKind::Track, v))
        }
        // YouTube Music albums are playlists whose ID starts with OLAK5uy_.
        ["playlist", ..] => query("list").map_or(Err(ParseError::UnsupportedPath { provider: p }), |list| {
            let kind = if list.starts_with("OLAK5uy_") { EntityKind::Album } else { EntityKind::Playlist };
            link(p, kind, list)
        }),
        ["browse", id, ..] if id.starts_with("MPREb_") => link(p, EntityKind::Album, *id),
        ["channel", id, ..] => link(p, EntityKind::Artist, *id),
        _ => Err(ParseError::UnsupportedPath { provider: p }),
    }
}

const SOUNDCLOUD_RESERVED: &[&str] =
    &["discover", "search", "stream", "you", "upload", "pages", "terms-of-use", "charts", "stations", "tags"];

fn soundcloud(segments: &[&str]) -> Result<Parsed, ParseError> {
    let p = Provider::SoundCloud;
    match segments {
        [user, ..] if SOUNDCLOUD_RESERVED.contains(user) => Err(ParseError::UnsupportedPath { provider: p }),
        [user, "sets", set, ..] => link(p, EntityKind::Playlist, format!("{user}/sets/{set}")),
        [user] | [user, "tracks" | "albums" | "popular-tracks"] => link(p, EntityKind::Artist, *user),
        [user, track, ..] => link(p, EntityKind::Track, format!("{user}/{track}")),
        [] => Err(ParseError::UnsupportedPath { provider: p }),
    }
}

fn bandcamp(host: &str, segments: &[&str]) -> Result<Parsed, ParseError> {
    let p = Provider::Bandcamp;
    let artist = host.trim_end_matches(".bandcamp.com");
    match segments {
        ["album", slug, ..] => link(p, EntityKind::Album, format!("{artist}/{slug}")),
        ["track", slug, ..] => link(p, EntityKind::Track, format!("{artist}/{slug}")),
        [] | ["music"] => link(p, EntityKind::Artist, artist),
        _ => Err(ParseError::UnsupportedPath { provider: p }),
    }
}

fn musicbrainz(segments: &[&str]) -> Result<Parsed, ParseError> {
    let p = Provider::MusicBrainz;
    let kind = match segments.first() {
        Some(&"release-group" | &"release") => EntityKind::Album,
        Some(&"recording" | &"track") => EntityKind::Track,
        Some(&"artist") => EntityKind::Artist,
        _ => return Err(ParseError::UnsupportedPath { provider: p }),
    };
    let id = segments.get(1).filter(|id| id.len() == 36).ok_or(ParseError::UnsupportedPath { provider: p })?;
    // Keep the entity type in the ID: a release and a release group are different lookups.
    link(p, kind, format!("{}/{id}", segments[0]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use EntityKind::{Album, Artist, Playlist, Track};

    fn ok(input: &str) -> Link {
        match parse(input) {
            Ok(Parsed::Link(l)) => l,
            other => panic!("{input}: expected link, got {other:?}"),
        }
    }

    fn check(input: &str, provider: Provider, kind: EntityKind, id: &str) {
        assert_eq!(ok(input), Link { provider, kind, id: id.into() }, "input: {input}");
    }

    #[test]
    fn spotify() {
        check(
            "https://open.spotify.com/album/6dVIqQ8qmQ5GBnJ9shOYGE",
            Provider::Spotify,
            Album,
            "6dVIqQ8qmQ5GBnJ9shOYGE",
        );
        check(
            "open.spotify.com/intl-de/track/2CVV8PtUYYsux8XOzWkCP0?si=x",
            Provider::Spotify,
            Track,
            "2CVV8PtUYYsux8XOzWkCP0",
        );
        check("spotify:artist:4Z8W4fKeB5YxbusRsdQVPb", Provider::Spotify, Artist, "4Z8W4fKeB5YxbusRsdQVPb");
        check(
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            Provider::Spotify,
            Playlist,
            "37i9dQZF1DXcBWIGoYBM5M",
        );
    }

    #[test]
    fn apple_music() {
        check("https://music.apple.com/gb/album/ok-computer/1097861387", Provider::AppleMusic, Album, "1097861387");
        check(
            "https://music.apple.com/us/album/airbag/1097861387?i=1097861771",
            Provider::AppleMusic,
            Track,
            "1097861771",
        );
        check("https://music.apple.com/us/artist/radiohead/657515", Provider::AppleMusic, Artist, "657515");
    }

    #[test]
    fn tidal_deezer_qobuz() {
        check("https://tidal.com/browse/album/58990510", Provider::Tidal, Album, "58990510");
        check("https://listen.tidal.com/track/58990511", Provider::Tidal, Track, "58990511");
        check("https://www.deezer.com/en/album/302127", Provider::Deezer, Album, "302127");
        check("https://open.qobuz.com/album/0634904078164", Provider::Qobuz, Album, "0634904078164");
        check(
            "https://www.qobuz.com/gb-en/album/ok-computer-radiohead/0634904078164",
            Provider::Qobuz,
            Album,
            "0634904078164",
        );
    }

    #[test]
    fn youtube_music() {
        check("https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=RD", Provider::YoutubeMusic, Track, "dQw4w9WgXcQ");
        check(
            "https://music.youtube.com/playlist?list=OLAK5uy_kDq9hkA8L3yb0kFeD_E7XbWNMCQh2gFfA",
            Provider::YoutubeMusic,
            Album,
            "OLAK5uy_kDq9hkA8L3yb0kFeD_E7XbWNMCQh2gFfA",
        );
        check("https://music.youtube.com/playlist?list=PL123", Provider::YoutubeMusic, Playlist, "PL123");
        check("https://youtu.be/dQw4w9WgXcQ", Provider::YoutubeMusic, Track, "dQw4w9WgXcQ");
    }

    #[test]
    fn soundcloud_and_bandcamp() {
        check("https://soundcloud.com/flume/never-be-like-you", Provider::SoundCloud, Track, "flume/never-be-like-you");
        check("https://soundcloud.com/flume/sets/skin", Provider::SoundCloud, Playlist, "flume/sets/skin");
        check("https://soundcloud.com/flume", Provider::SoundCloud, Artist, "flume");
        check("https://c418.bandcamp.com/album/volume-alpha", Provider::Bandcamp, Album, "c418/volume-alpha");
        check("https://c418.bandcamp.com/", Provider::Bandcamp, Artist, "c418");
    }

    #[test]
    fn musicbrainz() {
        check(
            "https://musicbrainz.org/release-group/b1392450-e666-3926-a536-22c65f834433",
            Provider::MusicBrainz,
            Album,
            "release-group/b1392450-e666-3926-a536-22c65f834433",
        );
    }

    #[test]
    fn short_links_need_expansion() {
        assert!(matches!(
            parse("https://spotify.link/abc123"),
            Ok(Parsed::ShortLink { provider: Provider::Spotify, .. })
        ));
        assert!(matches!(
            parse("https://deezer.page.link/xyz"),
            Ok(Parsed::ShortLink { provider: Provider::Deezer, .. })
        ));
    }

    #[test]
    fn rejects_non_links_and_unknown_hosts() {
        assert_eq!(parse("radiohead ok computer"), Err(ParseError::NotAUrl));
        assert!(matches!(parse("https://example.com/album/1"), Err(ParseError::UnsupportedHost(_))));
        assert!(matches!(parse("https://open.spotify.com/"), Err(ParseError::UnsupportedPath { .. })));
        assert!(matches!(parse("https://soundcloud.com/discover"), Err(ParseError::UnsupportedPath { .. })));
    }
}
