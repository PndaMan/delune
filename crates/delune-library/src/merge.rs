//! Adding tracks to an album the library already has.
//!
//! Tracks downloaded to fill gaps must land in the album's existing folder and carry
//! the same album tags, or Navidrome shows them as a second album. Albums also get
//! reordered after release (tracks added, moved), so the album's current tracklist
//! decides every track number, including the ones already in the library.

use std::path::{Path, PathBuf};

use delune_core::Quality;
use serde::{Deserialize, Serialize};

use crate::inspect;
use crate::naming::{NamingOptions, Template, TrackFields};

/// The library's copy of an album, as found on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingAlbum {
    /// Relative to the library root, `/`-separated.
    pub folder: String,
    pub album_artist: String,
    pub album: String,
    pub year: Option<u16>,
    pub tracks: Vec<ExistingTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingTrack {
    /// File name inside the album folder.
    pub file: String,
    pub title: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub quality: Option<Quality>,
}

/// One track of an album's current tracklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedTrack {
    pub position: u32,
    pub title: String,
}

/// An existing file getting a new number (and so a new name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renumber {
    /// Relative to the library root, `/`-separated.
    pub from: String,
    pub to: String,
    pub track: u32,
    pub old_track: Option<u32>,
}

const AUDIO: &[&str] = &["flac", "alac", "wav", "aif", "aiff", "mp3", "m4a", "aac", "opus", "ogg", "oga", "wv", "ape"];

/// A comparison key: case, accents, punctuation and bracketed extras don't count.
#[must_use]
pub fn title_key(title: &str) -> String {
    let mut depth = 0u32;
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '&' => out.push_str("and"),
            c if c.is_alphanumeric() => out.push(fold(c)),
            _ => {}
        }
    }
    out
}

const fn fold(c: char) -> char {
    match c {
        'à'..='å' | 'ā' => 'a',
        'ç' | 'č' => 'c',
        'è'..='ë' | 'ē' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

fn same_title(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || (a.len() >= 5 && b.len() >= 5 && (a.contains(b) || b.contains(a))))
}

/// A title's place in the tracklist. An exact match wins over a loose one, so
/// "Beto's Horns" and "Beto's Horns (Dapa remix)" keep their own places.
#[must_use]
pub fn position(tracklist: &[ListedTrack], title: &str) -> Option<u32> {
    let key = title_key(title);
    let exact = tracklist.iter().find(|t| title_key(&t.title) == key);
    exact
        .or_else(|| {
            let mut loose = tracklist.iter().filter(|t| same_title(&title_key(&t.title), &key));
            let first = loose.next()?;
            // Ambiguous: better to leave the number alone than guess.
            loose.next().is_none().then_some(first)
        })
        .map(|t| t.position)
}

fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

fn has_audio(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| entries.flatten().any(|e| is_audio(&e.path())))
}

fn loose(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Find the library's folder for an album Navidrome says it has.
///
/// Tries where the naming template would put it, then any folder under a matching
/// artist folder whose name contains the album's. `None` when nothing fits.
#[must_use]
pub fn find_existing(
    root: &Path,
    template: &Template,
    options: &NamingOptions,
    album_artist: &str,
    album: &str,
    year: Option<u16>,
) -> Option<ExistingAlbum> {
    let fields = TrackFields {
        title: "x".into(),
        artist: album_artist.to_owned(),
        album_artist: album_artist.to_owned(),
        album: album.to_owned(),
        year,
        track: 1,
        disc: 1,
        disc_count: 1,
        ..TrackFields::default()
    };
    let rendered = template.render(&fields, options);
    let mut found: Option<PathBuf> =
        rendered.rsplit_once('/').map(|(dir, _)| root.join(dir)).filter(|dir| dir.is_dir() && has_audio(dir));

    if found.is_none() {
        let wanted_artist = loose(album_artist);
        let wanted_album = loose(album);
        if wanted_album.len() < 2 {
            return None;
        }
        let artist_dirs = std::fs::read_dir(root).ok()?.flatten().map(|e| e.path()).filter(|p| {
            p.is_dir()
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    let n = loose(n);
                    !n.is_empty()
                        && (n == wanted_artist || n.starts_with(&wanted_artist) || wanted_artist.starts_with(&n))
                })
        });
        'artists: for artist_dir in artist_dirs {
            let Ok(entries) = std::fs::read_dir(&artist_dir) else { continue };
            let mut albums: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            albums.sort();
            for dir in albums {
                let name = dir.file_name().and_then(|n| n.to_str()).map(loose).unwrap_or_default();
                if name.contains(&wanted_album) && has_audio(&dir) {
                    found = Some(dir);
                    break 'artists;
                }
            }
        }
    }

    let dir = found?;
    let folder = dir.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    let mut tracks = Vec::new();
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| is_audio(p)).collect();
    files.sort();
    for path in files {
        let file = path.file_name()?.to_str()?.to_owned();
        let info = inspect::inspect(&path).ok();
        let tags = info.as_ref().map(|i| i.tags.clone()).unwrap_or_default();
        let (number, name_title) = crate::import::parse_file_name(&path);
        tracks.push(ExistingTrack {
            title: tags.title.unwrap_or(name_title),
            track: tags.track.or(number),
            disc: tags.disc,
            quality: info.map(|i| i.quality),
            file,
        });
    }
    Some(ExistingAlbum { folder, album_artist: album_artist.to_owned(), album: album.to_owned(), year, tracks })
}

/// Whether a tracklist can number this album: one disc, and it covers what's there.
#[must_use]
pub fn usable(tracklist: &[ListedTrack], existing: &ExistingAlbum) -> bool {
    !tracklist.is_empty()
        && existing.tracks.iter().all(|t| t.disc.unwrap_or(1) <= 1)
        && existing.tracks.iter().filter(|t| position(tracklist, &t.title).is_some()).count() * 2
            >= existing.tracks.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(titles: &[&str]) -> Vec<ListedTrack> {
        titles
            .iter()
            .enumerate()
            .map(|(i, t)| ListedTrack { position: u32::try_from(i + 1).unwrap(), title: (*t).into() })
            .collect()
    }

    #[test]
    fn finds_places_in_the_tracklist() {
        let tracklist = list(&["Lights Burn Dimmer", "Beto's Horns", "Beto's Horns (Dapa remix)", "Jungle"]);
        assert_eq!(position(&tracklist, "lights burn dimmer"), Some(1));
        assert_eq!(position(&tracklist, "Beto’s Horns"), Some(2), "exact beats the remix");
        assert_eq!(position(&tracklist, "Jungle (feat. Elley Duhé)"), Some(4), "brackets don't count");
        assert_eq!(position(&tracklist, "Kyle"), None);
    }

    #[test]
    fn finds_the_album_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Fred again..").join("2022 - USB");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("01 - Kyle.flac"), "x").unwrap();
        let template = Template::parse("{album_artist}/[{year} - ]{album}/{track} - {title}").unwrap();
        let options = NamingOptions::default();

        let found = find_existing(root.path(), &template, &options, "Fred again..", "USB", Some(2022)).unwrap();
        assert_eq!(found.folder, "Fred again../2022 - USB");
        assert_eq!(found.tracks[0].title, "Kyle");
        assert_eq!(found.tracks[0].track, Some(1));

        // A different year in Navidrome: found by name under the artist.
        let found = find_existing(root.path(), &template, &options, "Fred again..", "USB", Some(2024)).unwrap();
        assert_eq!(found.folder, "Fred again../2022 - USB");
        assert!(find_existing(root.path(), &template, &options, "Fred again..", "Actual Life", None).is_none());
    }
}
