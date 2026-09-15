//! # delune-soulseek
//!
//! A native Soulseek client, written to be a *good citizen* of the network: it
//! shares, it rate-limits searches, and it respects upload slots and queues.
//!
//! Layers, bottom-up:
//!
//! - [`wire`] — primitives: little-endian integers, length-prefixed strings, framing.
//! - [`server`] — messages to and from the central server (login, search, peer lookup).
//! - [`peer`] — peer-to-peer messages, starting with compressed search responses.
//! - Coming next: the connection state machine (server session, peer connections,
//!   firewall piercing), the search rate limiter, transfers, and share indexing.
//!
//! See `docs/adr/0002-native-soulseek.md` for why delune speaks the protocol itself
//! instead of driving slskd.

pub mod peer;
pub mod server;
pub mod wire;

/// Default Soulseek server address.
pub const DEFAULT_SERVER: &str = "server.slsknet.org:2242";
