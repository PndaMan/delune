//! # delune-resolve
//!
//! From "whatever the user pasted" to "the same release on every service".
//!
//! - [`link`]: offline parsing of provider URLs into `(provider, kind, id)`.
//! - [`resolve`]: what a link points at (title, artist, tracklist), fetched from
//!   each service's public metadata. See `docs/adr/0003-link-resolution.md`.
//! - [`query`]: the Soulseek search a resolved release turns into.

mod html;
pub mod link;
pub mod query;
pub mod resolve;

pub use link::{EntityKind, Link, ParseError, Parsed, parse};
pub use resolve::{ResolveError, Resolver};

/// What the search bar should do with some input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// A link we understood (or a short link to expand).
    Link(Parsed),
    /// Free text to search for.
    Text(String),
}

/// Classify search-bar input. Anything that isn't a recognisable link is a text
/// search — the user should never see a "not a valid URL" error for typing words.
///
/// ```
/// use delune_resolve::{classify, Query};
/// assert!(matches!(classify("  radiohead ok computer "), Query::Text(t) if t == "radiohead ok computer"));
/// assert!(matches!(classify("https://www.deezer.com/album/302127"), Query::Link(_)));
/// ```
#[must_use]
pub fn classify(input: &str) -> Query {
    let trimmed = input.trim();
    match parse(trimmed) {
        Ok(parsed) => Query::Link(parsed),
        Err(_) => Query::Text(trimmed.to_owned()),
    }
}
