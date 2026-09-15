//! # delune-core
//!
//! The shared vocabulary of delune. No I/O lives here — only types and pure logic
//! that every other crate (Soulseek client, resolver, library, server, TUI) agrees on.
//!
//! - [`quality`] — codecs, bit depth/sample rate, and the single ranking key.
//! - [`provider`] — known services and the Soulseek-first [`SourcePolicy`].
//! - [`media`] — releases, tracks and the identifiers that link them across services.
//! - [`api`] — request/response types shared by the server and its clients.

pub mod api;
pub mod media;
pub mod provider;
pub mod quality;

pub use media::{ExternalIds, Release, ReleaseKind, Track};
pub use provider::{Provider, ProviderRole, SourcePolicy};
pub use quality::{Codec, Quality};
