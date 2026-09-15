//! "Is this already in my library?"
//!
//! Search results and release views ask about each album they show. We search
//! Navidrome for the album title, accept only a match whose title and artist agree
//! (the same rules as artwork matching), and return the tracks the library holds so
//! clients can say "In your library" or "3 tracks missing".
//!
//! Answers are cached briefly: a results page asks about the same album many times.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Query, State},
};
use delune_core::api::{LibraryMatch, LibraryState, LibraryTrack};
use delune_core::{Codec, Quality};
use delune_navidrome::{AlbumWithSongs, Song};
use serde::Deserialize;

use crate::AppState;
use crate::artwork::{clean_names, names_match, normalize};

const CACHE_FOR: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub struct LibraryCache {
    entries: Mutex<HashMap<String, (Instant, LibraryMatch)>>,
}

impl LibraryCache {
    fn get(&self, key: &str) -> Option<LibraryMatch> {
        let entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.get(key).filter(|(at, _)| at.elapsed() < CACHE_FOR).map(|(_, m)| m.clone())
    }

    fn put(&self, key: String, value: LibraryMatch) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() > 2_000 {
            entries.retain(|_, (at, _)| at.elapsed() < CACHE_FOR);
        }
        entries.insert(key, (Instant::now(), value));
    }

    /// Forget everything, e.g. after an import changed the library.
    pub fn clear(&self) {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
}

#[derive(Debug, Deserialize)]
pub struct AlbumParams {
    artist: Option<String>,
    album: String,
    context: Option<String>,
}

fn unknown() -> LibraryMatch {
    LibraryMatch {
        state: LibraryState::Unknown,
        album: None,
        artist: None,
        year: None,
        tracks: vec![],
        quality_label: None,
    }
}

/// `GET /api/v1/library/album?artist=…&album=…&context=…`
pub async fn album(State(app): State<AppState>, Query(params): Query<AlbumParams>) -> Json<LibraryMatch> {
    let Some(navidrome) = &app.navidrome else { return Json(unknown()) };
    let (artist, album) = clean_names(params.artist.as_deref(), &params.album);
    if album.is_empty() {
        return Json(unknown());
    }
    let key = format!(
        "{}\u{1f}{}\u{1f}{}",
        normalize(artist.as_deref().unwrap_or("")),
        normalize(&album),
        normalize(params.context.as_deref().unwrap_or(""))
    );
    if let Some(hit) = app.library_cache.get(&key) {
        return Json(hit);
    }

    let found = match navidrome.search(&album, 20, 0).await {
        Ok(results) => results,
        Err(error) => {
            tracing::debug!(%error, "library check failed");
            return Json(unknown());
        }
    };
    let wanted = normalize(&album);
    let context = params.context.as_deref().map(normalize).unwrap_or_default();
    let matched = found.album.into_iter().find(|candidate| {
        let title_ok = names_match(&wanted, &normalize(&candidate.name));
        let found_artist = normalize(candidate.artist.as_deref().unwrap_or_default());
        let artist_ok = match &artist {
            Some(a) => names_match(&normalize(a), &found_artist),
            None => found_artist.len() >= 3 && context.contains(&found_artist),
        };
        title_ok && artist_ok
    });

    let result = match matched {
        None => LibraryMatch { state: LibraryState::NotInLibrary, ..unknown() },
        Some(summary) => match navidrome.album(&summary.id).await {
            Ok(full) => describe(&full),
            Err(_) => unknown(),
        },
    };
    app.library_cache.put(key, result.clone());
    Json(result)
}

fn describe(album: &AlbumWithSongs) -> LibraryMatch {
    let worst = album.song.iter().filter_map(song_quality).min_by_key(Quality::rank);
    LibraryMatch {
        state: LibraryState::InLibrary,
        album: Some(album.name.clone()),
        artist: album.artist.clone(),
        year: album.year,
        tracks: album
            .song
            .iter()
            .map(|s| LibraryTrack { title: s.title.clone(), track: s.track, disc: s.disc_number })
            .collect(),
        quality_label: worst.map(|q| q.to_string()),
    }
}

fn song_quality(song: &Song) -> Option<Quality> {
    let codec = Codec::from_extension(song.suffix.as_deref()?)?;
    Some(Quality {
        codec,
        bit_depth: song.bit_depth.filter(|_| codec.is_lossless()),
        sample_rate: song.sampling_rate.filter(|_| codec.is_lossless()),
        bitrate_kbps: if codec.is_lossless() { None } else { song.bit_rate },
        vbr: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, suffix: &str, depth: Option<u8>) -> Song {
        Song {
            id: title.into(),
            title: title.into(),
            album: None,
            artist: None,
            path: None,
            suffix: Some(suffix.into()),
            bit_rate: Some(320),
            bit_depth: depth,
            sampling_rate: Some(44_100),
            duration: None,
            track: Some(1),
            disc_number: Some(1),
        }
    }

    #[test]
    fn describes_the_library_copy_by_its_weakest_track() {
        let album = AlbumWithSongs {
            id: "a".into(),
            name: "Twoism".into(),
            artist: Some("Boards of Canada".into()),
            year: Some(1995),
            song: vec![song("Sixtyniner", "flac", Some(16)), song("Oirectine", "mp3", None)],
        };
        let described = describe(&album);
        assert_eq!(described.state, LibraryState::InLibrary);
        assert_eq!(described.tracks.len(), 2);
        assert_eq!(described.quality_label.as_deref(), Some("MP3 320"));
    }
}
