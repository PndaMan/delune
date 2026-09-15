//! Where music comes from, and in what order we ask.
//!
//! delune's central product rule lives here: **Soulseek is always searched first,
//! and streaming services are only used when the user has explicitly enabled them.**
//! Keeping that rule in one small, tested function means no other part of the
//! codebase can accidentally reorder or silently add sources.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A service delune knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Soulseek,
    Qobuz,
    Tidal,
    Deezer,
    YoutubeMusic,
    SoundCloud,
    Bandcamp,
    Spotify,
    AppleMusic,
    MusicBrainz,
}

/// What a provider can be used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum ProviderRole {
    /// Peer-to-peer. Always on, always first.
    PeerToPeer,
    /// Can deliver audio, but only when the user opts in.
    Streaming,
    /// Links and metadata only — never a download source.
    MetadataOnly,
}

impl Provider {
    pub const ALL: [Self; 10] = [
        Self::Soulseek,
        Self::Qobuz,
        Self::Tidal,
        Self::Deezer,
        Self::YoutubeMusic,
        Self::SoundCloud,
        Self::Bandcamp,
        Self::Spotify,
        Self::AppleMusic,
        Self::MusicBrainz,
    ];

    #[must_use]
    pub const fn role(self) -> ProviderRole {
        match self {
            Self::Soulseek => ProviderRole::PeerToPeer,
            Self::Qobuz | Self::Tidal | Self::Deezer | Self::YoutubeMusic | Self::SoundCloud | Self::Bandcamp => {
                ProviderRole::Streaming
            }
            Self::Spotify | Self::AppleMusic | Self::MusicBrainz => ProviderRole::MetadataOnly,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Soulseek => "Soulseek",
            Self::Qobuz => "Qobuz",
            Self::Tidal => "Tidal",
            Self::Deezer => "Deezer",
            Self::YoutubeMusic => "YouTube Music",
            Self::SoundCloud => "SoundCloud",
            Self::Bandcamp => "Bandcamp",
            Self::Spotify => "Spotify",
            Self::AppleMusic => "Apple Music",
            Self::MusicBrainz => "MusicBrainz",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The user's source preferences, as stored in settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SourcePolicy {
    /// Streaming providers the user has switched on, in their preferred order.
    /// Empty by default: a fresh install only ever uses Soulseek.
    pub enabled_streaming: Vec<Provider>,
}

impl SourcePolicy {
    /// The order in which download sources are queried.
    ///
    /// Guarantees, enforced here and covered by tests:
    /// - Soulseek is always present and always first.
    /// - Only streaming providers the user enabled appear, in the user's order.
    /// - Metadata-only providers never appear, even if misconfigured.
    /// - No provider appears twice.
    #[must_use]
    pub fn search_order(&self) -> Vec<Provider> {
        let mut order = vec![Provider::Soulseek];
        for &p in &self.enabled_streaming {
            if p.role() == ProviderRole::Streaming && !order.contains(&p) {
                order.push(p);
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_soulseek_only() {
        assert_eq!(SourcePolicy::default().search_order(), vec![Provider::Soulseek]);
    }

    #[test]
    fn soulseek_first_even_if_user_lists_it_later() {
        let policy = SourcePolicy { enabled_streaming: vec![Provider::Qobuz, Provider::Soulseek, Provider::Tidal] };
        assert_eq!(policy.search_order(), vec![Provider::Soulseek, Provider::Qobuz, Provider::Tidal]);
    }

    #[test]
    fn metadata_only_and_duplicates_are_dropped() {
        let policy = SourcePolicy {
            enabled_streaming: vec![Provider::Spotify, Provider::Deezer, Provider::Deezer, Provider::MusicBrainz],
        };
        assert_eq!(policy.search_order(), vec![Provider::Soulseek, Provider::Deezer]);
    }

    #[test]
    fn exactly_one_peer_to_peer_provider() {
        assert_eq!(Provider::ALL.iter().filter(|p| p.role() == ProviderRole::PeerToPeer).count(), 1);
    }
}
