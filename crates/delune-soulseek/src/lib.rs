//! # delune-soulseek
//!
//! A native Soulseek client, written to be a *good citizen* of the network: it
//! rate-limits searches, backs off when disconnected, refuses to fight another client
//! for the same account, and caps its peer connections.
//!
//! Layers, bottom-up:
//!
//! - [`wire`] — primitives: little-endian integers, length-prefixed strings.
//! - [`frame`] — turning TCP byte streams into length-prefixed frames, with size limits.
//! - [`server`] — messages to and from the central server (login, search, peer lookup).
//! - [`peer`] — peer-to-peer messages: connection init and compressed search responses.
//! - [`limiter`] — the search rate limiter.
//! - [`client`] — the running client: session supervision, peer connections, searches.
//!
//! Still to come: file transfers, share indexing and answering other people's
//! searches, and the distributed search network.
//!
//! ```no_run
//! # async fn demo() -> Result<(), delune_soulseek::client::Error> {
//! use delune_soulseek::client::{Client, Config};
//!
//! let client = Client::start(Config::new("username", "password"));
//! let mut state = client.state();
//! state.wait_for(|s| matches!(s, delune_soulseek::client::SessionState::Online { .. })).await.ok();
//!
//! let mut search = client.search("boards of canada geogaddi").await?;
//! while let Some(response) = search.next().await {
//!     println!("{}: {} files", response.username, response.files.len());
//! }
//! # Ok(()) }
//! ```
//!
//! See `docs/adr/0002-native-soulseek.md` for why delune speaks the protocol itself
//! instead of driving slskd.

pub mod client;
mod connection;
pub mod frame;
pub mod limiter;
pub mod peer;
pub mod server;
pub mod wire;

pub use client::{Client, Config, Search, SessionState, StopReason};

/// Default Soulseek server address.
pub const DEFAULT_SERVER: &str = "server.slsknet.org:2242";
