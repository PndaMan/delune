//! Finishing touches on imported files: embedded artwork and lyrics.
//!
//! Both are best effort. A file whose tags can't be written keeps its audio
//! untouched, and the reason comes back for the log.

use std::fs;
use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::picture::{Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use serde::{Deserialize, Serialize};

/// Where lyrics go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LyricsMode {
    Off,
    /// A `.lrc` file beside the track (plain text as `.txt` when there are no timings).
    #[default]
    Sidecar,
    /// Inside the file's tags.
    Embed,
    Both,
}

/// Lyrics for one track: synced (LRC) when known, plain otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
}

fn tag_of(file: &mut lofty::file::TaggedFile) -> &mut Tag {
    if file.primary_tag().is_none() {
        let tag_type = file.primary_tag_type();
        file.insert_tag(Tag::new(tag_type));
    }
    file.primary_tag_mut().expect("a primary tag was just inserted")
}

/// Album tags to set on a file; `None` leaves a tag as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumTags {
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<u16>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

/// Set album tags, so a track joins the album it was filed with.
///
/// # Errors
///
/// When the file can't be read or its tags can't be written.
pub fn write_album_tags(audio: &Path, tags: &AlbumTags) -> Result<(), String> {
    let mut file = lofty::read_from_path(audio).map_err(|e| e.to_string())?;
    let tag = tag_of(&mut file);
    if let Some(album) = &tags.album {
        tag.set_album(album.clone());
    }
    if let Some(album_artist) = &tags.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, album_artist.clone());
    }
    if let Some(year) = tags.year {
        tag.insert_text(ItemKey::Year, year.to_string());
    }
    if let Some(track) = tags.track {
        tag.set_track(track);
    }
    if let Some(disc) = tags.disc {
        tag.set_disk(disc);
    }
    file.save_to_path(audio, WriteOptions::default()).map_err(|e| e.to_string())
}

/// Embed `image` as the front cover, unless the file already has one.
///
/// # Errors
///
/// When the file or image can't be read, or the tags can't be written.
pub fn embed_cover(audio: &Path, image: &Path) -> Result<bool, String> {
    let mut file = lofty::read_from_path(audio).map_err(|e| e.to_string())?;
    let tag = tag_of(&mut file);
    if tag.pictures().iter().any(|p| p.pic_type() == PictureType::CoverFront) {
        return Ok(false);
    }
    let mut reader = fs::File::open(image).map_err(|e| e.to_string())?;
    let mut picture = Picture::from_reader(&mut reader).map_err(|e| e.to_string())?;
    picture.set_pic_type(PictureType::CoverFront);
    tag.push_picture(picture);
    file.save_to_path(audio, WriteOptions::default()).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Save lyrics beside the track and/or in its tags.
///
/// # Errors
///
/// When a sidecar can't be written or the tags can't be saved.
pub fn write_lyrics(audio: &Path, lyrics: &Lyrics, mode: LyricsMode) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    if matches!(mode, LyricsMode::Sidecar | LyricsMode::Both) {
        let sidecar = match (&lyrics.synced, &lyrics.plain) {
            (Some(synced), _) => Some((audio.with_extension("lrc"), synced)),
            (None, Some(plain)) => Some((audio.with_extension("txt"), plain)),
            (None, None) => None,
        };
        if let Some((path, text)) = sidecar
            && !path.exists()
        {
            fs::write(&path, text).map_err(|e| e.to_string())?;
            written.push(path);
        }
    }
    if matches!(mode, LyricsMode::Embed | LyricsMode::Both)
        && let Some(text) = lyrics.synced.as_ref().or(lyrics.plain.as_ref())
    {
        let mut file = lofty::read_from_path(audio).map_err(|e| e.to_string())?;
        let tag = tag_of(&mut file);
        // ID3v2 only takes unsynchronised lyrics as text; other formats take either.
        if !tag.insert_text(ItemKey::Lyrics, text.clone()) {
            let plain = lyrics.plain.clone().unwrap_or_else(|| strip_timestamps(text));
            tag.insert_text(ItemKey::UnsyncLyrics, plain);
        }
        file.save_to_path(audio, WriteOptions::default()).map_err(|e| e.to_string())?;
        written.push(audio.to_owned());
    }
    Ok(written)
}

/// "[01:02.30] words" → "words".
fn strip_timestamps(lrc: &str) -> String {
    lrc.lines()
        .map(|line| {
            let mut rest = line;
            while let Some(stripped) = rest.strip_prefix('[').and_then(|r| r.split_once(']')).map(|(_, r)| r) {
                rest = stripped;
            }
            rest.trim()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_lrc_timings() {
        assert_eq!(strip_timestamps("[00:01.00]Hello\n[00:02.50][00:05.00]World"), "Hello\nWorld");
    }

    #[test]
    fn writes_sidecars_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("delune-lyrics-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("01 Song.flac");
        let lyrics = Lyrics { synced: Some("[00:01.00]Hi".into()), plain: Some("Hi".into()) };
        let written = write_lyrics(&audio, &lyrics, LyricsMode::Sidecar).unwrap();
        assert_eq!(written, [dir.join("01 Song.lrc")]);
        assert_eq!(fs::read_to_string(dir.join("01 Song.lrc")).unwrap(), "[00:01.00]Hi");
        assert!(write_lyrics(&audio, &lyrics, LyricsMode::Sidecar).unwrap().is_empty(), "existing sidecars are kept");

        let plain_only = Lyrics { synced: None, plain: Some("Just words".into()) };
        let other = dir.join("02 Other.flac");
        assert_eq!(write_lyrics(&other, &plain_only, LyricsMode::Sidecar).unwrap(), [dir.join("02 Other.txt")]);
        fs::remove_dir_all(dir).unwrap();
    }
}
