//! File and folder naming templates.
//!
//! A template like
//!
//! ```text
//! {album_artist}/{year} - {album}[ ({edition})]/{track} - {title}
//! ```
//!
//! is parsed once (so the settings UI can show errors as you type) and rendered for
//! every track. The syntax is small on purpose:
//!
//! | Syntax      | Meaning                                                              |
//! |-------------|----------------------------------------------------------------------|
//! | `{token}`   | Insert a value. Unknown tokens are a parse error, not silent blanks. |
//! | `[ ... ]`   | Optional group: dropped entirely if any token inside is empty.       |
//! | `/`         | Folder separator. Each folder name is sanitised on its own.          |
//! | `{{` `}}` `[[` `]]` | Literal braces/brackets.                                     |
//!
//! Rendering never produces a path that escapes the library root: separators and
//! illegal characters inside *values* are replaced, `.`/`..` components are
//! neutralised, and trailing dots/spaces (which break SMB and Windows clients) are
//! trimmed.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Every token a template may use. Kept as one list so the settings UI can offer
/// them as clickable chips and the docs can be generated from it.
pub const TOKENS: &[(&str, &str)] = &[
    ("title", "Track title"),
    ("artist", "Track artist(s)"),
    ("album_artist", "Album artist"),
    ("album", "Album title"),
    ("edition", "Edition or version, e.g. Deluxe"),
    ("year", "Original release year"),
    ("track", "Track number (respects padding and multi-disc settings)"),
    ("disc", "Disc number"),
    ("disc_count", "Number of discs"),
    ("genre", "Primary genre"),
    ("composer", "Composer"),
    ("label", "Record label"),
    ("catalog", "Catalogue number"),
    ("isrc", "ISRC"),
    ("codec", "Codec, e.g. FLAC"),
    ("quality", "Quality label, e.g. FLAC 24/96"),
    ("bit_depth", "Bit depth, e.g. 24"),
    ("sample_rate", "Sample rate in kHz, e.g. 96"),
    ("bitrate", "Bitrate in kbps"),
    ("source", "Where it came from, e.g. Soulseek"),
    ("mbid", "MusicBrainz release ID"),
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MultiDisc {
    /// `1-01`, `2-01` on multi-disc releases; plain `01` otherwise.
    #[default]
    DiscPrefix,
    /// Numbering continues across discs: disc 2 starts where disc 1 ended.
    Continuous,
    /// Always the per-disc number. Pair with `{disc}` in a folder name.
    PerDisc,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Whitespace {
    #[default]
    Preserve,
    /// Collapse runs of whitespace into a single space.
    Collapse,
    /// Replace whitespace with underscores.
    Underscore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NamingOptions {
    /// Minimum digits for track numbers (`2` → `01`).
    pub track_padding: u8,
    pub multi_disc: MultiDisc,
    /// Replacement for characters that aren't allowed in file names.
    pub illegal_replacement: String,
    pub whitespace: Whitespace,
    /// Longest allowed file or folder name, in bytes. Most filesystems cap at 255.
    pub max_component_bytes: usize,
}

impl Default for NamingOptions {
    fn default() -> Self {
        Self {
            track_padding: 2,
            multi_disc: MultiDisc::default(),
            illegal_replacement: "_".into(),
            whitespace: Whitespace::default(),
            max_component_bytes: 200,
        }
    }
}

/// The values available to a template for one track.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackFields {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub edition: Option<String>,
    pub year: Option<u16>,
    pub track: u32,
    pub disc: u32,
    pub disc_count: u32,
    /// Number of tracks on all discs *before* this one (for continuous numbering).
    pub tracks_before_disc: u32,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub label: Option<String>,
    pub catalog: Option<String>,
    pub isrc: Option<String>,
    pub codec: String,
    pub quality: String,
    pub bit_depth: Option<u8>,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
    pub source: String,
    pub mbid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (at character {position})")]
pub struct TemplateError {
    /// 0-based character offset, for highlighting in the editor.
    pub position: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Token(&'static str),
    Optional(Vec<Part>),
}

/// A parsed, validated naming template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    source: String,
    parts: Vec<Part>,
}

impl Template {
    pub fn parse(source: &str) -> Result<Self, TemplateError> {
        let chars: Vec<char> = source.chars().collect();
        let mut pos = 0;
        let parts = parse_parts(&chars, &mut pos, false)?;
        if parts.is_empty() {
            return Err(TemplateError { position: 0, message: "template is empty".into() });
        }
        Ok(Self { source: source.to_owned(), parts })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Render to a relative path, without extension, using `/` as separator.
    #[must_use]
    pub fn render(&self, fields: &TrackFields, opts: &NamingOptions) -> String {
        let mut raw = String::new();
        render_parts(&self.parts, fields, opts, &mut raw);
        raw.split('/')
            .map(|component| sanitize_component(component, opts))
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn parse_parts(chars: &[char], pos: &mut usize, in_group: bool) -> Result<Vec<Part>, TemplateError> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let group_start = pos.saturating_sub(1);

    while *pos < chars.len() {
        let c = chars[*pos];
        let next = chars.get(*pos + 1).copied();
        match (c, next) {
            ('{', Some('{')) | ('}', Some('}')) | ('[', Some('[')) | (']', Some(']')) => {
                literal.push(c);
                *pos += 2;
            }
            ('{', _) => {
                let start = *pos;
                let end = chars[start..]
                    .iter()
                    .position(|&ch| ch == '}')
                    .map(|off| start + off)
                    .ok_or_else(|| TemplateError { position: start, message: "unclosed `{`".into() })?;
                let name: String = chars[start + 1..end].iter().collect();
                let token = TOKENS
                    .iter()
                    .find(|(t, _)| *t == name.trim())
                    .map(|(t, _)| *t)
                    .ok_or_else(|| TemplateError { position: start, message: format!("unknown token `{{{name}}}`") })?;
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Token(token));
                *pos = end + 1;
            }
            ('}', _) => return Err(TemplateError { position: *pos, message: "unmatched `}`".into() }),
            ('[', _) => {
                if in_group {
                    return Err(TemplateError { position: *pos, message: "optional groups can't be nested".into() });
                }
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                *pos += 1;
                let inner = parse_parts(chars, pos, true)?;
                parts.push(Part::Optional(inner));
            }
            (']', _) if in_group => {
                *pos += 1;
                if !literal.is_empty() {
                    parts.push(Part::Literal(literal));
                }
                return Ok(parts);
            }
            (']', _) => return Err(TemplateError { position: *pos, message: "unmatched `]`".into() }),
            _ => {
                literal.push(c);
                *pos += 1;
            }
        }
    }

    if in_group {
        return Err(TemplateError { position: group_start, message: "unclosed `[`".into() });
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
}

fn render_parts(parts: &[Part], fields: &TrackFields, opts: &NamingOptions, out: &mut String) -> bool {
    let mut all_present = true;
    for part in parts {
        match part {
            Part::Literal(s) => out.push_str(s),
            Part::Token(t) => {
                let value = token_value(t, fields, opts);
                all_present &= !value.is_empty();
                // Values may not introduce folders: a `/` in "AC/DC" is not a directory.
                out.push_str(&value.replace(['/', '\\'], &opts.illegal_replacement));
            }
            Part::Optional(inner) => {
                let mut buf = String::new();
                if render_parts(inner, fields, opts, &mut buf) {
                    out.push_str(&buf);
                }
            }
        }
    }
    all_present
}

fn token_value(token: &str, f: &TrackFields, opts: &NamingOptions) -> String {
    let opt = |v: &Option<String>| v.clone().unwrap_or_default();
    let num = |v: Option<u32>| v.map(|n| n.to_string()).unwrap_or_default();
    match token {
        "title" => f.title.clone(),
        "artist" => f.artist.clone(),
        "album_artist" => f.album_artist.clone(),
        "album" => f.album.clone(),
        "edition" => opt(&f.edition),
        "year" => num(f.year.map(u32::from)),
        "track" => track_number(f, opts),
        "disc" => f.disc.to_string(),
        "disc_count" => f.disc_count.to_string(),
        "genre" => opt(&f.genre),
        "composer" => opt(&f.composer),
        "label" => opt(&f.label),
        "catalog" => opt(&f.catalog),
        "isrc" => opt(&f.isrc),
        "codec" => f.codec.clone(),
        "quality" => f.quality.clone(),
        "bit_depth" => num(f.bit_depth.map(u32::from)),
        "sample_rate" => f.sample_rate.map(format_khz).unwrap_or_default(),
        "bitrate" => num(f.bitrate),
        "source" => f.source.clone(),
        "mbid" => opt(&f.mbid),
        _ => unreachable!("tokens are validated at parse time"),
    }
}

fn track_number(f: &TrackFields, opts: &NamingOptions) -> String {
    let width = usize::from(opts.track_padding);
    match opts.multi_disc {
        MultiDisc::DiscPrefix if f.disc_count > 1 => format!("{}-{:0width$}", f.disc, f.track),
        MultiDisc::Continuous => format!("{:0width$}", f.tracks_before_disc + f.track),
        _ => format!("{:0width$}", f.track),
    }
}

fn format_khz(hz: u32) -> String {
    let mut s = String::new();
    if hz.is_multiple_of(1000) {
        let _ = write!(s, "{}", hz / 1000);
    } else {
        let _ = write!(s, "{:.1}", f64::from(hz) / 1000.0);
    }
    s
}

/// Make one path component safe on every filesystem Navidrome is likely to sit on.
fn sanitize_component(component: &str, opts: &NamingOptions) -> String {
    let mut s: String = component
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                opts.illegal_replacement.clone()
            } else {
                c.to_string()
            }
        })
        .collect();

    s = match opts.whitespace {
        Whitespace::Preserve => s,
        Whitespace::Collapse => s.split_whitespace().collect::<Vec<_>>().join(" "),
        Whitespace::Underscore => s.split_whitespace().collect::<Vec<_>>().join("_"),
    };

    // Truncate on a character boundary.
    if s.len() > opts.max_component_bytes {
        let mut cut = opts.max_component_bytes;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }

    let trimmed = s.trim();
    // `.` and `..` would navigate instead of naming; check before trimming dots away.
    if !trimmed.is_empty() && trimmed.chars().all(|c| c == '.') {
        return opts.illegal_replacement.repeat(trimmed.len());
    }
    trimmed.trim_end_matches(['.', ' ']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> TrackFields {
        TrackFields {
            title: "Paranoid Android".into(),
            artist: "Radiohead".into(),
            album_artist: "Radiohead".into(),
            album: "OK Computer".into(),
            year: Some(1997),
            track: 2,
            disc: 1,
            disc_count: 1,
            codec: "FLAC".into(),
            quality: "FLAC 24/96".into(),
            sample_rate: Some(96_000),
            ..TrackFields::default()
        }
    }

    fn render(template: &str, f: &TrackFields) -> String {
        Template::parse(template).unwrap().render(f, &NamingOptions::default())
    }

    #[test]
    fn antra_style_default() {
        assert_eq!(
            render("{album_artist}/{year} - {album}/{track} - {title}", &fields()),
            "Radiohead/1997 - OK Computer/02 - Paranoid Android"
        );
    }

    #[test]
    fn optional_groups_vanish_when_empty() {
        let t = "{album}[ ({edition})] [[{quality}]]";
        assert_eq!(render(t, &fields()), "OK Computer [FLAC 24_96]");
        let deluxe = TrackFields { edition: Some("OKNOTOK".into()), ..fields() };
        assert_eq!(render(t, &deluxe), "OK Computer (OKNOTOK) [FLAC 24_96]");
    }

    #[test]
    fn multi_disc_modes() {
        let f = TrackFields { disc: 2, disc_count: 2, track: 3, tracks_before_disc: 12, ..fields() };
        let t = Template::parse("{track}").unwrap();
        let with = |multi_disc| t.render(&f, &NamingOptions { multi_disc, ..NamingOptions::default() });
        assert_eq!(with(MultiDisc::DiscPrefix), "2-03");
        assert_eq!(with(MultiDisc::Continuous), "15");
        assert_eq!(with(MultiDisc::PerDisc), "03");
        // Single-disc releases never get a disc prefix.
        assert_eq!(render("{track}", &fields()), "02");
    }

    #[test]
    fn values_cannot_create_folders_or_escape() {
        let f = TrackFields {
            album_artist: "AC/DC".into(),
            album: "..".into(),
            title: "What?: \"Yes\"".into(),
            ..fields()
        };
        assert_eq!(render("{album_artist}/{album}/{title}", &f), "AC_DC/__/What__ _Yes_");
    }

    #[test]
    fn trailing_dots_and_spaces_trimmed() {
        let f = TrackFields { album: "Hail to the Thief...".into(), ..fields() };
        assert_eq!(render("{album} ", &f), "Hail to the Thief");
    }

    #[test]
    fn whitespace_and_length() {
        let opts =
            NamingOptions { whitespace: Whitespace::Underscore, max_component_bytes: 10, ..NamingOptions::default() };
        let out = Template::parse("{album}").unwrap().render(&fields(), &opts);
        assert_eq!(out, "OK_Compute");
        let unicode = TrackFields { album: "ééééééé".into(), ..fields() };
        let out = Template::parse("{album}")
            .unwrap()
            .render(&unicode, &NamingOptions { max_component_bytes: 5, ..NamingOptions::default() });
        assert_eq!(out, "éé");
    }

    #[test]
    fn parse_errors_point_at_the_problem() {
        assert_eq!(Template::parse("{album} - {titel}").unwrap_err().position, 10);
        assert!(Template::parse("{album").unwrap_err().message.contains("unclosed"));
        assert!(Template::parse("[{a}[{b}]]").is_err());
        assert!(Template::parse("[{album}").unwrap_err().message.contains("unclosed `[`"));
        assert!(Template::parse("").is_err());
    }

    #[test]
    fn escapes() {
        assert_eq!(render("{{{album}}} [[x]]", &fields()), "{OK Computer} [x]");
    }
}
