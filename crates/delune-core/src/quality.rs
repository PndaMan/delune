//! Audio quality: what a file *is*, and how two files compare.
//!
//! Every candidate delune considers — a Soulseek folder, a Qobuz stream, a file
//! already sitting in Navidrome — is reduced to a [`Quality`]. Ranking then works
//! on a single ordered [`Quality::rank`] key, so "highest quality" means the same
//! thing in the resolver, the review screen and the upgrade checker.
//!
//! The ordering is deliberately simple and documented in `docs/adr/0004-quality-ranking.md`:
//!
//! 1. Lossless always beats lossy.
//! 2. Among lossless: higher bit depth, then higher sample rate.
//! 3. Among lossy: higher effective bitrate, with a small bonus for efficient codecs.
//!
//! Anything unknown ranks *below* the same thing known, so a peer that reports
//! `24/96` wins over one that reports nothing.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Container/codec family of an audio file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    Flac,
    Alac,
    Wav,
    Aiff,
    Mp3,
    Aac,
    Opus,
    Vorbis,
}

impl Codec {
    /// Guess the codec from a file extension (case-insensitive, without the dot).
    ///
    /// `.m4a` is ambiguous (ALAC or AAC); we return AAC because that is by far the
    /// more common case on Soulseek. Verification after download corrects it.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "flac" => Self::Flac,
            "alac" => Self::Alac,
            "wav" => Self::Wav,
            "aif" | "aiff" => Self::Aiff,
            "mp3" => Self::Mp3,
            "m4a" | "aac" | "mp4" => Self::Aac,
            "opus" => Self::Opus,
            "ogg" | "oga" => Self::Vorbis,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn is_lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Alac | Self::Wav | Self::Aiff)
    }

    /// Lossy codecs are not equally efficient: 256 kbps AAC sounds at least as good
    /// as 320 kbps MP3. This multiplier converts a bitrate into an "MP3-equivalent".
    const fn efficiency_percent(self) -> u32 {
        match self {
            Self::Opus => 150,
            Self::Aac | Self::Vorbis => 125,
            _ => 100,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Flac => "FLAC",
            Self::Alac => "ALAC",
            Self::Wav => "WAV",
            Self::Aiff => "AIFF",
            Self::Mp3 => "MP3",
            Self::Aac => "AAC",
            Self::Opus => "Opus",
            Self::Vorbis => "Vorbis",
        }
    }
}

/// Everything we know about the quality of one audio stream.
///
/// Fields are optional because sources are unreliable: Soulseek peers often omit
/// attributes, and streaming APIs report quality per album, not per file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Quality {
    pub codec: Codec,
    /// Bits per sample (lossless only), e.g. 16 or 24.
    pub bit_depth: Option<u8>,
    /// Sample rate in Hz, e.g. 44100 or 96000.
    pub sample_rate: Option<u32>,
    /// Average bitrate in kbps.
    pub bitrate_kbps: Option<u32>,
    /// Variable bitrate (lossy only).
    pub vbr: bool,
}

impl Quality {
    #[must_use]
    pub const fn lossless(codec: Codec, bit_depth: u8, sample_rate: u32) -> Self {
        Self { codec, bit_depth: Some(bit_depth), sample_rate: Some(sample_rate), bitrate_kbps: None, vbr: false }
    }

    #[must_use]
    pub const fn lossy(codec: Codec, bitrate_kbps: u32) -> Self {
        Self { codec, bit_depth: None, sample_rate: None, bitrate_kbps: Some(bitrate_kbps), vbr: false }
    }

    /// A single comparable key. Higher is better.
    ///
    /// Layout (most significant bits first):
    /// `lossless flag (bit 30) | bit depth (bits 18–29) | sample rate in 100 Hz (bits 0–17)`
    /// for lossless, and effective bitrate for lossy. It fits in a `u32` so it can be
    /// sent to JavaScript clients without losing precision.
    #[must_use]
    pub fn rank(&self) -> u32 {
        if self.codec.is_lossless() {
            // Unknown depth/rate on a lossless file: assume CD quality but rank it
            // one step below a file that *says* it is CD quality.
            let depth = u32::from(self.bit_depth.unwrap_or(16).min(64)) * 2 + u32::from(self.bit_depth.is_some());
            let rate =
                (self.sample_rate.unwrap_or(44_100).min(1_536_000) / 100) * 2 + u32::from(self.sample_rate.is_some());
            (1 << 30) | (depth << 18) | rate
        } else {
            let kbps = self.bitrate_kbps.unwrap_or(0).min(10_000);
            kbps * self.codec.efficiency_percent() / 100
        }
    }

    /// Is `self` strictly better than `other`?
    #[must_use]
    pub fn beats(&self, other: &Self) -> bool {
        self.rank() > other.rank()
    }

    /// Estimate bitrate from file size and duration when a source doesn't report it.
    ///
    /// Used for Soulseek results with missing attributes. Returns kbps.
    #[must_use]
    pub fn estimate_bitrate_kbps(size_bytes: u64, duration_secs: u32) -> Option<u32> {
        if duration_secs == 0 {
            return None;
        }
        u32::try_from(size_bytes * 8 / 1000 / u64::from(duration_secs)).ok()
    }
}

impl fmt::Display for Quality {
    /// `FLAC 24/96`, `FLAC 16/44.1`, `MP3 320`, `MP3 V0`, `AAC 256`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.codec.label())?;
        if self.codec.is_lossless() {
            if let (Some(depth), Some(rate)) = (self.bit_depth, self.sample_rate) {
                let khz = f64::from(rate) / 1000.0;
                if rate.is_multiple_of(1000) {
                    write!(f, " {depth}/{khz:.0}")?;
                } else {
                    write!(f, " {depth}/{khz:.1}")?;
                }
            }
        } else if let Some(kbps) = self.bitrate_kbps {
            if self.vbr && self.codec == Codec::Mp3 && (220..=260).contains(&kbps) {
                f.write_str(" V0")?;
            } else {
                write!(f, " {kbps}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_beats_any_lossy() {
        let cd = Quality::lossless(Codec::Flac, 16, 44_100);
        let opus = Quality::lossy(Codec::Opus, 510);
        assert!(cd.beats(&opus));
    }

    #[test]
    fn hires_ordering() {
        let ladder = [
            Quality::lossless(Codec::Flac, 24, 192_000),
            Quality::lossless(Codec::Flac, 24, 96_000),
            Quality::lossless(Codec::Flac, 24, 48_000),
            Quality::lossless(Codec::Flac, 24, 44_100),
            Quality::lossless(Codec::Flac, 16, 44_100),
            Quality::lossy(Codec::Mp3, 320),
            Quality::lossy(Codec::Mp3, 245),
            Quality::lossy(Codec::Mp3, 128),
        ];
        for pair in ladder.windows(2) {
            assert!(pair[0].beats(&pair[1]), "{} should beat {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn known_beats_unknown() {
        let known = Quality::lossless(Codec::Flac, 16, 44_100);
        let unknown = Quality { bit_depth: None, sample_rate: None, ..known };
        assert!(known.beats(&unknown));
    }

    #[test]
    fn efficient_codecs_get_credit() {
        assert!(Quality::lossy(Codec::Aac, 256).rank() >= Quality::lossy(Codec::Mp3, 320).rank());
    }

    #[test]
    fn display_labels() {
        assert_eq!(Quality::lossless(Codec::Flac, 24, 96_000).to_string(), "FLAC 24/96");
        assert_eq!(Quality::lossless(Codec::Flac, 16, 44_100).to_string(), "FLAC 16/44.1");
        assert_eq!(Quality::lossy(Codec::Mp3, 320).to_string(), "MP3 320");
        assert_eq!(Quality { vbr: true, ..Quality::lossy(Codec::Mp3, 245) }.to_string(), "MP3 V0");
    }

    #[test]
    fn extension_guessing() {
        assert_eq!(Codec::from_extension("FLAC"), Some(Codec::Flac));
        assert_eq!(Codec::from_extension("m4a"), Some(Codec::Aac));
        assert_eq!(Codec::from_extension("jpg"), None);
    }

    #[test]
    fn bitrate_estimate() {
        // 3 minutes at 320 kbps ≈ 7.2 MB
        assert_eq!(Quality::estimate_bitrate_kbps(7_200_000, 180), Some(320));
        assert_eq!(Quality::estimate_bitrate_kbps(1, 0), None);
    }
}
