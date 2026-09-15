//! What a downloaded file claims to be: audio properties and embedded tags.
//!
//! Reads with lofty, which understands FLAC, MP3, MP4/ALAC/AAC, Ogg, Opus, WAV and
//! AIFF tags. Nothing here decodes audio; see [`crate::verify`] for that.

use std::path::Path;

use delune_core::{Codec, Quality};
use lofty::prelude::*;
use serde::{Deserialize, Serialize};

/// Tags that feed naming templates. Everything is optional: files from Soulseek
/// are often tagged partially or not at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<u16>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub disc_total: Option<u32>,
    pub genre: Option<String>,
    pub label: Option<String>,
    pub catalog: Option<String>,
    pub isrc: Option<String>,
    pub musicbrainz_release: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub quality: Quality,
    pub duration_secs: u32,
    pub channels: Option<u8>,
    pub tags: Tags,
}

#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    #[error("not an audio file delune understands")]
    Unsupported,
    #[error("couldn't read the file: {0}")]
    Read(String),
}

/// Just the audio properties, skipping tags and artwork: fast enough to index a
/// whole library for sharing.
///
/// # Errors
///
/// When the file isn't audio delune understands, or can't be read.
pub fn properties(path: &Path) -> Result<(Quality, u32), InspectError> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let mut codec = Codec::from_extension(extension).ok_or(InspectError::Unsupported)?;
    let options = lofty::config::ParseOptions::new().read_tags(false).read_cover_art(false);
    let file = lofty::probe::Probe::open(path)
        .and_then(|probe| probe.options(options).read())
        .map_err(|e| InspectError::Read(e.to_string()))?;
    let props = file.properties();
    if codec == Codec::Aac && props.bit_depth().is_some() {
        codec = Codec::Alac;
    }
    let quality = Quality {
        codec,
        bit_depth: props.bit_depth().filter(|_| codec.is_lossless()),
        sample_rate: props.sample_rate(),
        bitrate_kbps: if codec.is_lossless() { None } else { props.audio_bitrate() },
        vbr: false,
    };
    Ok((quality, u32::try_from(props.duration().as_secs()).unwrap_or(u32::MAX)))
}

/// Read properties and tags from an audio file.
pub fn inspect(path: &Path) -> Result<AudioInfo, InspectError> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let mut codec = Codec::from_extension(extension).ok_or(InspectError::Unsupported)?;
    let file = lofty::read_from_path(path).map_err(|e| InspectError::Read(e.to_string()))?;
    let props = file.properties();

    // `.m4a` may hold ALAC; lofty reports a bit depth only for lossless streams.
    if codec == Codec::Aac && props.bit_depth().is_some() {
        codec = Codec::Alac;
    }
    let quality = Quality {
        codec,
        bit_depth: props.bit_depth().filter(|_| codec.is_lossless()),
        sample_rate: props.sample_rate(),
        bitrate_kbps: if codec.is_lossless() { None } else { props.audio_bitrate() },
        vbr: false,
    };

    let tags = file.primary_tag().or_else(|| file.first_tag()).map(read_tags).unwrap_or_default();

    Ok(AudioInfo {
        quality,
        duration_secs: u32::try_from(props.duration().as_secs()).unwrap_or(u32::MAX),
        channels: props.channels(),
        tags,
    })
}

fn read_tags(tag: &lofty::tag::Tag) -> Tags {
    let text = |key: ItemKey| tag.get_string(key).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let year = tag
        .date()
        .map(|d| d.year)
        .or_else(|| text(ItemKey::Year).and_then(|y| y.get(..4).and_then(|y| y.parse().ok())));
    Tags {
        title: tag.title().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
        artist: tag.artist().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
        album_artist: text(ItemKey::AlbumArtist),
        album: tag.album().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
        year: year.filter(|y| *y > 0),
        track: tag.track(),
        track_total: tag.track_total(),
        disc: tag.disk(),
        disc_total: tag.disk_total(),
        genre: tag.genre().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
        label: text(ItemKey::Label),
        catalog: text(ItemKey::CatalogNumber),
        isrc: text(ItemKey::Isrc),
        musicbrainz_release: text(ItemKey::MusicBrainzReleaseId),
    }
}
