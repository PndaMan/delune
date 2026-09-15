//! Finding a linked release on MusicBrainz.
//!
//! Streaming services often give a barcode (UPC) for albums and an ISRC for tracks.
//! Those identify a release exactly, so MusicBrainz is asked by those first; without
//! them, a release group whose title and artist both match exactly will do. A match
//! tells delune when the album first came out (a 2011 remaster of a 1973 album is
//! still a 1973 album) and, for a track, which album it's from.
//!
//! MusicBrainz allows one request a second per client, so requests queue behind a
//! shared gate.

use std::time::Duration;

use delune_core::EntityKind;
use delune_core::api::{MatchedBy, MusicBrainzMatch, ResolvedLink};
use serde_json::Value;

use crate::query::clean_title;

/// MusicBrainz's rate limit.
pub(crate) const MUSICBRAINZ_GAP: Duration = Duration::from_millis(1100);
/// Search scores below this aren't trusted.
const MIN_SCORE: u64 = 90;

fn str_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn year_of(date: Option<&str>) -> Option<u16> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok()).filter(|y| (1000..3000).contains(y))
}

/// Letters and digits only, lower case: "OK Computer" and "ok computer!" are the same.
fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn credit(entity: &Value) -> Option<String> {
    let credits = entity.get("artist-credit")?.as_array()?;
    let mut names = String::new();
    for c in credits {
        names.push_str(str_at(c, "/name").unwrap_or_default());
        names.push_str(c.get("joinphrase").and_then(Value::as_str).unwrap_or_default());
    }
    (!names.is_empty()).then_some(names)
}

/// Quote a phrase for a MusicBrainz (Lucene) search.
pub(crate) fn phrase(s: &str) -> String {
    let escaped: String =
        s.chars().flat_map(|c| if matches!(c, '"' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
    format!("\"{escaped}\"")
}

/// Barcodes are digits, 8 to 14 of them; anything else isn't worth asking about.
pub(crate) fn valid_barcode(upc: &str) -> bool {
    (8..=14).contains(&upc.len()) && upc.bytes().all(|b| b.is_ascii_digit())
}

/// ISRCs look like `GBAYE9700378`.
pub(crate) fn valid_isrc(isrc: &str) -> bool {
    isrc.len() == 12 && isrc.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The release in a `release?query=barcode:…` search. Its year is the release's own;
/// the release group's first date comes from a second request.
pub(crate) fn release_by_barcode(json: &Value) -> Option<MusicBrainzMatch> {
    let release = json.get("releases")?.as_array()?.iter().find(|r| score(r) >= MIN_SCORE)?;
    Some(MusicBrainzMatch {
        release_id: str_at(release, "/id").map(str::to_owned),
        release_group_id: str_at(release, "/release-group/id")?.to_owned(),
        title: str_at(release, "/title")?.to_owned(),
        artist: credit(release),
        original_year: year_of(str_at(release, "/date")),
        matched_by: MatchedBy::Barcode,
    })
}

/// The earliest date a release group came out, from `release-group/{id}`.
pub(crate) fn first_release_year(json: &Value) -> Option<u16> {
    year_of(str_at(json, "/first-release-date"))
}

/// The album a recording in a `recording?query=isrc:…` search first appeared on,
/// preferring albums over singles and compilations.
pub(crate) fn album_by_isrc(json: &Value) -> Option<MusicBrainzMatch> {
    let recordings = json.get("recordings")?.as_array()?;
    let releases = recordings.iter().filter_map(|r| r.get("releases")?.as_array()).flatten();
    let kind_rank = |r: &Value| match str_at(r, "/release-group/primary-type") {
        Some("Album")
            if r.pointer("/release-group/secondary-types").and_then(Value::as_array).is_none_or(Vec::is_empty) =>
        {
            0
        }
        Some("Album") => 1,
        Some("EP") => 2,
        _ => 3,
    };
    let date = |r: &Value| str_at(r, "/date").unwrap_or("9999").to_owned();
    let best = releases.min_by_key(|r| (kind_rank(r), date(r)))?;
    let group = str_at(best, "/release-group/id")?;
    // The group's earliest date among the releases listed here.
    let original = recordings
        .iter()
        .filter_map(|r| r.get("releases")?.as_array())
        .flatten()
        .filter(|r| str_at(r, "/release-group/id") == Some(group))
        .filter_map(|r| year_of(str_at(r, "/date")))
        .min();
    Some(MusicBrainzMatch {
        release_id: str_at(best, "/id").map(str::to_owned),
        release_group_id: group.to_owned(),
        title: str_at(best, "/release-group/title").or_else(|| str_at(best, "/title"))?.to_owned(),
        artist: credit(best).or_else(|| recordings.first().and_then(credit)),
        original_year: original,
        matched_by: MatchedBy::Isrc,
    })
}

/// A release group from a `release-group?query=…` search whose title and artist
/// both match exactly (ignoring case, punctuation and edition noise).
pub(crate) fn album_by_name(json: &Value, title: &str, artist: &str) -> Option<MusicBrainzMatch> {
    let (want_title, want_artist) = (squash(&clean_title(title)), squash(artist));
    if want_title.is_empty() || want_artist.is_empty() {
        return None;
    }
    let group = json.get("release-groups")?.as_array()?.iter().find(|g| {
        score(g) >= MIN_SCORE
            && str_at(g, "/title").is_some_and(|t| squash(&clean_title(t)) == want_title)
            && credit(g).is_some_and(|a| squash(&a) == want_artist)
    })?;
    Some(MusicBrainzMatch {
        release_id: None,
        release_group_id: str_at(group, "/id")?.to_owned(),
        title: str_at(group, "/title")?.to_owned(),
        artist: credit(group),
        original_year: year_of(str_at(group, "/first-release-date")),
        matched_by: MatchedBy::Name,
    })
}

fn score(entity: &Value) -> u64 {
    entity.get("score").and_then(Value::as_u64).unwrap_or(0)
}

/// Fill in what a match adds: the original year, and a track's album.
pub(crate) fn apply(link: &mut ResolvedLink, found: MusicBrainzMatch) {
    if let Some(year) = found.original_year {
        link.year = Some(link.year.map_or(year, |y| y.min(year)));
    }
    if link.kind == EntityKind::Track && link.album.is_none() {
        link.album = Some(found.title.clone());
    }
    link.musicbrainz = Some(found);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn barcodes_and_isrcs_are_checked_before_asking() {
        assert!(valid_barcode("0724384260958") && !valid_barcode("12ab") && !valid_barcode("1234567"));
        assert!(valid_isrc("GBAYE9700378") && !valid_isrc("GB-AYE-97-00378"));
        assert_eq!(phrase(r#"Say "Hi" \o/"#), r#""Say \"Hi\" \\o/""#);
    }

    #[test]
    fn reads_a_barcode_search() {
        let found = release_by_barcode(&json!({"releases": [{
            "id": "rel-1", "score": 100, "title": "Discovery", "date": "2001-03-07",
            "artist-credit": [{"name": "Daft Punk", "joinphrase": ""}],
            "release-group": {"id": "rg-1", "primary-type": "Album"}
        }]}))
        .unwrap();
        assert_eq!((found.release_group_id.as_str(), found.original_year), ("rg-1", Some(2001)));
        assert_eq!(found.artist.as_deref(), Some("Daft Punk"));
        assert_eq!(release_by_barcode(&json!({"releases": [{"id": "x", "score": 40}]})), None);
        assert_eq!(first_release_year(&json!({"first-release-date": "1997-05-21"})), Some(1997));
    }

    #[test]
    fn prefers_the_original_album_for_an_isrc() {
        let found = album_by_isrc(&json!({"recordings": [{
            "id": "rec", "title": "Airbag", "artist-credit": [{"name": "Radiohead"}],
            "releases": [
                {"id": "comp", "title": "Best Of", "date": "2008-06-02",
                 "release-group": {"id": "rg-comp", "title": "Best Of", "primary-type": "Album", "secondary-types": ["Compilation"]}},
                {"id": "reissue", "title": "OK Computer OKNOTOK", "date": "2017-06-23",
                 "release-group": {"id": "rg-okc", "title": "OK Computer", "primary-type": "Album"}},
                {"id": "orig", "title": "OK Computer", "date": "1997-05-21",
                 "release-group": {"id": "rg-okc", "title": "OK Computer", "primary-type": "Album"}}
            ]
        }]}))
        .unwrap();
        assert_eq!(found.release_group_id, "rg-okc");
        assert_eq!((found.release_id.as_deref(), found.original_year), (Some("orig"), Some(1997)));
        assert_eq!(found.title, "OK Computer");
    }

    #[test]
    fn name_matches_must_be_exact() {
        let groups = json!({"release-groups": [
            {"id": "tribute", "score": 100, "title": "OK Computer", "first-release-date": "2006",
             "artist-credit": [{"name": "Easy Star All-Stars"}]},
            {"id": "okc", "score": 98, "title": "OK Computer", "first-release-date": "1997-05-21",
             "artist-credit": [{"name": "Radiohead"}]}
        ]});
        let found = album_by_name(&groups, "OK Computer (Remastered)", "radiohead").unwrap();
        assert_eq!((found.release_group_id.as_str(), found.original_year), ("okc", Some(1997)));
        assert_eq!(album_by_name(&groups, "Kid A", "Radiohead"), None);
    }

    #[test]
    fn a_match_gives_the_original_year_and_a_tracks_album() {
        let mut link = ResolvedLink {
            provider: delune_core::Provider::Spotify,
            kind: EntityKind::Track,
            title: "Airbag".into(),
            artist: Some("Radiohead".into()),
            album: None,
            year: Some(2017),
            tracks: vec![],
            query: "Radiohead Airbag".into(),
            upc: None,
            isrc: None,
            musicbrainz: None,
        };
        let found = MusicBrainzMatch {
            release_id: None,
            release_group_id: "rg".into(),
            title: "OK Computer".into(),
            artist: None,
            original_year: Some(1997),
            matched_by: MatchedBy::Isrc,
        };
        apply(&mut link, found);
        assert_eq!((link.year, link.album.as_deref()), (Some(1997), Some("OK Computer")));
    }
}
