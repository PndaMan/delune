//! # delune-library
//!
//! Everything that touches the music library on disk.
//!
//! - [`naming`] — file/folder naming templates with live-preview-friendly errors.
//! - Coming in v0.1: layout detection (learn the naming scheme of an existing
//!   library), tagging (`lofty`), artwork and lyrics embedding, verification, and the
//!   atomic move from staging into the library.

pub mod naming;

pub use naming::{MultiDisc, NamingOptions, Template, TemplateError, TrackFields, Whitespace};
