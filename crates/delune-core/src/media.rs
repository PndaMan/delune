//! Releases and tracks, and the identifiers that tie one release together across
//! every service that carries it.
//!
//! A Spotify link, a Deezer link and a Soulseek folder for the same album are three
//! views of one [`Release`]. [`ExternalIds`] is how we know that: MusicBrainz IDs are
//! the canonical anchor, UPC (albums) and ISRC (tracks) bridge between stores.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::Provider;

/// Identifiers for one entity across services. All optional; more is better.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIds {
    /// MusicBrainz release group ("the album" regardless of edition).
    pub mb_release_group: Option<String>,
    /// MusicBrainz release (one specific edition and tracklist).
    pub mb_release: Option<String>,
    /// MusicBrainz recording (tracks only).
    pub mb_recording: Option<String>,
    /// Universal Product Code / EAN barcode (releases only).
    pub upc: Option<String>,
    /// International Standard Recording Code (tracks only).
    pub isrc: Option<String>,
    /// Provider-native IDs, e.g. `Deezer -> "302127"`.
    pub provider: BTreeMap<Provider, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseKind {
    Album,
    Ep,
    Single,
    Compilation,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub title: String,
    pub artists: Vec<String>,
    /// 1-based position on its disc.
    pub number: u32,
    /// 1-based disc number.
    pub disc: u32,
    pub duration_ms: Option<u32>,
    pub explicit: bool,
    pub ids: ExternalIds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub title: String,
    pub album_artist: String,
    pub kind: ReleaseKind,
    pub year: Option<u16>,
    pub label: Option<String>,
    pub tracks: Vec<Track>,
    pub ids: ExternalIds,
}

impl Release {
    #[must_use]
    pub fn disc_count(&self) -> u32 {
        self.tracks.iter().map(|t| t.disc).max().unwrap_or(1)
    }

    /// Total runtime, if every track has a known duration.
    #[must_use]
    pub fn duration_ms(&self) -> Option<u64> {
        self.tracks.iter().map(|t| t.duration_ms.map(u64::from)).sum()
    }
}
