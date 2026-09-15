//! # delune-resolve
//!
//! From "whatever the user pasted" to "the same release on every service".
//!
//! - [`link`] — offline parsing of provider URLs into `(provider, kind, id)`.
//! - Resolution (MusicBrainz URL lookup → UPC/ISRC bridge → provider metadata) lands
//!   next; see `docs/adr/0003-link-resolution.md` for why we don't use Odesli.

pub mod link;

pub use link::{EntityKind, Link, ParseError, Parsed, parse};

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
