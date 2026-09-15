//! # delune-library
//!
//! Everything that touches the music library on disk.
//!
//! - [`naming`] — file/folder naming templates with live-preview-friendly errors.
//! - [`inspect`] — audio properties and tags of downloaded files.
//! - [`verify`] — full decode and spectral transcode detection.
//! - [`import`] — planning destinations and moving files into the library.
//! - [`layout`] — working out an existing library's naming template.
//! - [`extras`] — embedded artwork and lyrics.

pub mod extras;
pub mod import;
pub mod inspect;
pub mod layout;
pub mod naming;
pub mod verify;

pub use naming::{MultiDisc, NamingOptions, Template, TemplateError, TrackFields, Whitespace};
