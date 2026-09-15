//! Types shared by the HTTP API and its clients (TUI, web via generated TypeScript).
//!
//! Anything here is part of the public API contract: renaming a field is a
//! breaking change for every client.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::Quality;

/// `GET /api/v1/health`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub name: String,
    pub version: String,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Degraded,
}

/// `GET /api/v1/soulseek`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulseekStatus {
    pub state: SoulseekState,
    /// The account delune logs in with, when configured.
    pub username: Option<String>,
    /// Human-readable detail for anything other than `online`.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SoulseekState {
    /// No Soulseek account configured.
    NotConfigured,
    Connecting,
    Online,
    Reconnecting,
    /// Won't reconnect without a restart (bad password, logged in elsewhere).
    Stopped,
}

/// Error body returned by every failing API route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// Stable machine-readable code, e.g. `soulseek-not-configured`.
    pub code: String,
    /// What happened and what to do about it, written for the end user.
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

/// One file inside a [`Candidate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateFile {
    pub name: String,
    pub size: u64,
    pub audio: bool,
    pub quality: Option<Quality>,
    pub quality_label: Option<String>,
    pub duration_secs: Option<u32>,
}

/// A folder shared by one peer that looks like a release: the unit users choose
/// between in search results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable within a search: `username` + folder path.
    pub id: String,
    pub username: String,
    /// Full folder path as the peer shares it.
    pub folder: String,
    /// Display title, usually the album folder name.
    pub title: String,
    /// The folder above, usually the artist.
    pub parent: Option<String>,
    pub files: Vec<CandidateFile>,
    pub audio_files: u32,
    pub total_bytes: u64,
    /// Sum of audio durations, when every audio file reports one.
    pub duration_secs: Option<u32>,
    /// The *lowest* quality among the audio files: what you're guaranteed to get.
    pub quality: Option<Quality>,
    pub quality_label: Option<String>,
    /// [`Quality::rank`] of `quality`; 0 when unknown.
    pub quality_rank: u32,
    /// Audio files differ in codec, depth or rate.
    pub mixed_quality: bool,
    pub has_cover: bool,
    pub free_slot: bool,
    /// Bytes per second.
    pub avg_speed: u32,
    pub queue_length: u32,
}

impl Candidate {
    /// Default result ordering: best quality first, then whoever can start
    /// sending soonest and fastest.
    #[must_use]
    pub fn compare(a: &Self, b: &Self) -> Ordering {
        b.quality_rank
            .cmp(&a.quality_rank)
            .then(b.mixed_quality.cmp(&a.mixed_quality).reverse())
            .then(b.free_slot.cmp(&a.free_slot))
            .then(a.queue_length.cmp(&b.queue_length))
            .then(b.avg_speed.cmp(&a.avg_speed))
            .then(a.id.cmp(&b.id))
    }
}

/// Events streamed by `GET /api/v1/search` (Server-Sent Events, JSON data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SearchEvent {
    Started {
        query: String,
        timeout_secs: u32,
    },
    /// New candidates from one peer. Clients merge and sort.
    Candidates {
        items: Vec<Candidate>,
    },
    Finished {
        peers: u32,
        candidates: u32,
    },
    Failed {
        error: ApiError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Codec;

    fn candidate(id: &str, quality: Option<Quality>) -> Candidate {
        Candidate {
            id: id.into(),
            username: "u".into(),
            folder: String::new(),
            title: String::new(),
            parent: None,
            files: vec![],
            audio_files: 1,
            total_bytes: 0,
            duration_secs: None,
            quality,
            quality_label: None,
            quality_rank: quality.map_or(0, |q| q.rank()),
            mixed_quality: false,
            has_cover: false,
            free_slot: true,
            avg_speed: 0,
            queue_length: 0,
        }
    }

    #[test]
    fn ordering_prefers_quality_then_availability() {
        let hires = candidate("a", Some(Quality::lossless(Codec::Flac, 24, 96_000)));
        let cd_busy = Candidate {
            free_slot: false,
            queue_length: 50,
            ..candidate("b", Some(Quality::lossless(Codec::Flac, 16, 44_100)))
        };
        let cd_free =
            Candidate { avg_speed: 5_000_000, ..candidate("c", Some(Quality::lossless(Codec::Flac, 16, 44_100))) };
        let cd_mixed = Candidate { mixed_quality: true, ..cd_free.clone() };
        let mp3 = candidate("d", Some(Quality::lossy(Codec::Mp3, 320)));
        let unknown = candidate("e", None);

        let mut list = [unknown, mp3, cd_busy, cd_mixed, hires, cd_free];
        list.sort_by(Candidate::compare);
        let ids: Vec<_> = list.iter().map(|c| (c.id.as_str(), c.mixed_quality)).collect();
        // A consistent-quality folder beats a mixed one even if it's busier (ADR 0004).
        assert_eq!(ids, [("a", false), ("c", false), ("b", false), ("c", true), ("d", false), ("e", false)]);
    }

    #[test]
    fn search_events_are_tagged() {
        let json = serde_json::to_string(&SearchEvent::Finished { peers: 3, candidates: 5 }).unwrap();
        assert_eq!(json, r#"{"type":"finished","peers":3,"candidates":5}"#);
    }
}
