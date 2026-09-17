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

/// Words that make a bracketed part of a title a note rather than part of the name:
/// "(feat. …)" and "[2011 Remaster]" don't make a different song, "(Dapa remix)" does.
const NOTE_WORDS: &[&str] = &[
    "feat",
    "ft",
    "featuring",
    "with",
    "prod",
    "remaster",
    "explicit",
    "clean",
    "bonus",
    "album version",
    "mono",
    "stereo",
];

fn is_note(inner: &str) -> bool {
    let inner = inner.trim().to_lowercase();
    NOTE_WORDS.iter().any(|w| inner.starts_with(w) || (w.len() > 4 && inner.contains(w)))
}

/// A comparison key: case, accents, punctuation and bracketed notes don't count.
#[must_use]
pub fn title_key(title: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    while let Some(start) = rest.find(['(', '[']) {
        push_key(&mut out, &rest[..start]);
        let close = if rest.as_bytes()[start] == b'(' { ')' } else { ']' };
        let Some(end) = rest[start..].find(close).map(|e| start + e) else {
            rest = &rest[start + 1..];
            continue;
        };
        let inner = &rest[start + 1..end];
        if !is_note(inner) {
            push_key(&mut out, inner);
        }
        rest = &rest[end + 1..];
    }
    push_key(&mut out, rest);
    out
}

fn push_key(out: &mut String, text: &str) {
    for c in text.to_lowercase().chars() {
        match c {
            '&' => out.push_str("and"),
            c if c.is_alphanumeric() => out.push(fold(c)),
            _ => {}
        }
    }
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

/// Words that make a longer title another version of a song rather than the same one.
const VERSION_WORDS: &[&str] = &[
    "remix",
    "mix",
    "edit",
    "version",
    "live",
    "acoustic",
    "instrumental",
    "demo",
    "rework",
    "vip",
    "dub",
    "extended",
];

/// Whether two title keys (from [`title_key`]) name the same song, allowing for extra
/// words on one of them unless those words make it another version.
#[must_use]
pub fn same_song(a: &str, b: &str) -> bool {
    same_title(a, b)
}

/// The song title in a shared file name: "Artist - Album - 03 - Time.flac" and
/// "03. Time.flac" are both "Time".
#[must_use]
pub fn file_title(file_name: &str) -> String {
    let stem = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or(file_name).replace('_', " ");
    let parts: Vec<&str> = stem.split(" - ").map(str::trim).collect();
    let is_number = |p: &str| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit());
    if let Some(i) = parts.iter().position(|p| is_number(p))
        && i + 1 < parts.len()
    {
        return parts[i + 1..].join(" - ");
    }
    crate::import::parse_file_name(Path::new(file_name)).1
}

fn same_title(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    long == short
        || (short.len() >= 5
            && long.contains(short)
            && !VERSION_WORDS.iter().any(|w| long.replacen(short, "", 1).contains(w)))
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
    audio_count(dir) > 0
}

fn loose(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_alphanumeric()).map(fold).collect()
}

/// An artist folder's key: "Fred again..", "Fred again._" and "fred again" are one artist,
/// and so are "The Beatles" and "Beatles".
#[must_use]
pub fn artist_key(name: &str) -> String {
    let key = loose(name);
    key.strip_prefix("the").filter(|rest| rest.len() >= 3).map_or(key.clone(), str::to_owned)
}

/// Whether a bracketed part of a folder name is decoration a template or a ripper adds:
/// a year, or a format.
fn is_decoration(inner: &str) -> bool {
    let inner = inner.trim().to_lowercase();
    let year = inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit());
    let formats = [
        "flac", "mp3", "alac", "aac", "ogg", "opus", "wav", "web", "cd", "vinyl", "lossless", "hi-res", "hires",
        "24bit", "24-bit", "16bit", "320", "v0",
    ];
    year || formats.iter().any(|f| inner.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w == *f))
}

/// An album's key, from a title or a folder name, without the year or format a template
/// or ripper adds: `2022 - USB`, `USB (2022)` and `USB [FLAC]` are all `usb`.
#[must_use]
pub fn album_key(name: &str) -> String {
    let mut s = name.trim();
    if let Some((head, rest)) = s.split_once(" - ")
        && head.len() == 4
        && head.chars().all(|c| c.is_ascii_digit())
    {
        s = rest;
    }
    loop {
        let t = s.trim_end();
        let close = match t.chars().last() {
            Some(')') => '(',
            Some(']') => '[',
            _ => break,
        };
        let Some(open) = t.rfind(close) else { break };
        if !is_decoration(&t[open + 1..t.len() - 1]) {
            break;
        }
        s = &t[..open];
    }
    title_key(s)
}

fn audio_count(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| entries.flatten().filter(|e| is_audio(&e.path())).count())
}

/// Every folder in the library holding this album: under any spelling of the artist's
/// folder, whatever year or format the folder name carries. Fullest first.
#[must_use]
pub fn album_folders(root: &Path, album_artist: &str, album: &str) -> Vec<PathBuf> {
    let (wanted_artist, wanted_album) = (artist_key(album_artist), album_key(album));
    if wanted_artist.is_empty() || wanted_album.is_empty() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found: Vec<(usize, PathBuf)> = Vec::new();
    for artist_dir in entries.flatten().map(|e| e.path()) {
        let matches = artist_dir.is_dir()
            && artist_dir.file_name().and_then(|n| n.to_str()).is_some_and(|n| artist_key(n) == wanted_artist);
        if !matches {
            continue;
        }
        let Ok(albums) = std::fs::read_dir(&artist_dir) else { continue };
        for dir in albums.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            let name = dir.file_name().and_then(|n| n.to_str()).map(album_key).unwrap_or_default();
            let count = audio_count(&dir);
            if name == wanted_album && count > 0 {
                found.push((count, dir));
            }
        }
    }
    // Fullest first; the path breaks ties, so the answer doesn't depend on disk order.
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, dir)| dir).collect()
}

/// Find the library's folder for an album.
///
/// Tries where the naming template would put it, then every folder of the album under
/// any spelling of the artist's folder (see [`album_folders`]), taking the fullest.
/// `None` when nothing fits.
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
    let at_template =
        rendered.rsplit_once('/').map(|(dir, _)| root.join(dir)).filter(|dir| dir.is_dir() && has_audio(dir));
    let others = album_folders(root, album_artist, album);
    // The template's folder, unless another copy of the album is clearly the main one.
    let found = match (at_template, others.first()) {
        (Some(template_dir), Some(fullest)) if audio_count(fullest) > audio_count(&template_dir) => {
            Some(fullest.clone())
        }
        (Some(template_dir), _) => Some(template_dir),
        (None, fullest) => fullest.cloned(),
    };

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
        assert_eq!(position(&tracklist, "Beto's Horns (Dapa Remix)"), Some(3), "a remix is its own song");
        assert_eq!(position(&tracklist, "Jungle (feat. Elley Duhé)"), Some(4), "brackets don't count");
        assert_eq!(position(&tracklist, "Kyle"), None);
    }

    #[test]
    fn bracketed_notes_dont_count_but_versions_do() {
        assert_eq!(title_key("Time (2011 Remaster)"), title_key("Time"));
        assert_eq!(title_key("Jungle [feat. Elley Duhé]"), "jungle");
        assert_ne!(title_key("solo (KETTAMA remix)"), title_key("solo"));
        assert_eq!(title_key("Rock & Roll (Live)"), "rockandrolllive");
        assert_eq!(title_key("Unclosed (bracket"), "unclosedbracket");
    }

    #[test]
    fn titles_come_out_of_file_names() {
        assert_eq!(file_title("Fred again. - USB - 04 - solo.flac"), "solo");
        assert_eq!(file_title("Fred again. - USB - 31 - solo (KETTAMA remix).flac"), "solo (KETTAMA remix)");
        assert_eq!(file_title("03. Time.flac"), "Time");
        assert_eq!(file_title("Aquarius.flac"), "Aquarius");
        assert!(same_song(&title_key(&file_title("01 - solo.flac")), &title_key("Solo")));
        assert!(!same_song(&title_key(&file_title("31 - solo (KETTAMA remix).flac")), &title_key("solo")));
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

    #[test]
    fn album_and_artist_keys_ignore_decoration() {
        for name in ["USB", "2022 - USB", "USB (2025)", "USB (0000)", "USB [FLAC]", "USB (2022) [24bit FLAC]"] {
            assert_eq!(album_key(name), "usb", "{name}");
        }
        assert_ne!(album_key("USB002 REMIXES"), "usb");
        assert_ne!(album_key("USB (Deluxe)"), "usb");
        assert_eq!(artist_key("Fred again.."), artist_key("Fred again._"));
        assert_eq!(artist_key("The Beatles"), artist_key("Beatles"));
    }

    #[test]
    fn copies_under_other_spellings_are_found_and_the_fullest_wins() {
        let root = tempfile::tempdir().unwrap();
        let small = root.path().join("Fred again").join("USB (2026)");
        let big = root.path().join("Fred again._").join("USB (2025)");
        let other = root.path().join("Fred again._").join("USB002 REMIXES");
        for (dir, n) in [(&small, 2), (&big, 5), (&other, 9)] {
            std::fs::create_dir_all(dir).unwrap();
            for i in 1..=n {
                std::fs::write(dir.join(format!("{i:02} - Song {i}.flac")), "x").unwrap();
            }
        }
        assert_eq!(album_folders(root.path(), "Fred again..", "USB"), [big.clone(), small.clone()]);
        // The template would put it in "Fred again../USB", which doesn't exist; and even
        // the template's own folder gives way to a fuller copy.
        let template = Template::parse("{album_artist}/{album} ({year})/{track} - {title}").unwrap();
        let options = NamingOptions::default();
        let found = find_existing(root.path(), &template, &options, "Fred again", "USB", Some(2026)).unwrap();
        assert_eq!(found.folder, "Fred again._/USB (2025)");
        assert_eq!(found.tracks.len(), 5);
    }
}
