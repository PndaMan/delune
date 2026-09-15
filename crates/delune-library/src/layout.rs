//! Working out how an existing library is laid out, as a naming template.
//!
//! People who already have a library want new albums to look like the old ones.
//! Rather than ask them to write a template, we read the tags of a sample of their
//! files and turn each path back into one: in
//! `Radiohead/1997 - OK Computer/02 - Paranoid Android.flac`, "Radiohead" is the
//! album artist, "1997" the year, and so on, giving
//! `{album_artist}/{year} - {album}/{track} - {title}`. The pattern most files agree
//! on wins, along with the track padding and disc numbering they use.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::inspect::{self, Tags};
use crate::naming::{MultiDisc, NamingOptions};

const AUDIO: &[&str] = &["flac", "alac", "wav", "aif", "aiff", "mp3", "m4a", "aac", "opus", "ogg", "oga"];
/// Files read from any one folder, so a sample spans many albums.
const PER_FOLDER: usize = 2;
/// Folders visited at most, so a huge or looping tree can't stall detection.
const MAX_FOLDERS: usize = 20_000;

/// One file from the library: its path relative to the library, with `/`
/// separators and no extension, and its tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sample {
    pub path: String,
    pub tags: Tags,
    /// Codec label, such as `FLAC`, for folders that name the format.
    pub codec: Option<String>,
}

/// The layout most of the sample follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedLayout {
    pub template: String,
    pub options: NamingOptions,
    /// Files whose path fits the template.
    pub matching: usize,
    /// Files whose tags were read.
    pub sampled: usize,
    /// A few real paths that fit, to show people what was recognised.
    pub examples: Vec<String>,
}

/// Read up to `limit` audio files from `root`, a couple per folder, breadth first so
/// the sample spans the library rather than one artist.
#[must_use]
pub fn sample(root: &Path, limit: usize) -> Vec<Sample> {
    let mut folders = VecDeque::from([root.to_path_buf()]);
    let mut out = Vec::new();
    let mut visited = 0;
    while let Some(folder) = folders.pop_front() {
        visited += 1;
        if out.len() >= limit || visited > MAX_FOLDERS {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort();
        let mut taken = 0;
        for path in entries {
            let hidden = path.file_name().and_then(|n| n.to_str()).is_none_or(|n| n.starts_with('.'));
            if hidden {
                continue;
            }
            if path.is_dir() {
                folders.push_back(path);
                continue;
            }
            let audio = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()));
            if !audio || taken >= PER_FOLDER || out.len() >= limit {
                continue;
            }
            let Some(relative) =
                path.strip_prefix(root).ok().and_then(|p| p.with_extension("").to_str().map(str::to_owned))
            else {
                continue;
            };
            let Ok(info) = inspect::inspect(&path) else { continue };
            out.push(Sample {
                path: relative.replace('\\', "/"),
                tags: info.tags,
                codec: Some(info.quality.codec.label().to_owned()),
            });
            taken += 1;
        }
    }
    out
}

/// The template most samples follow, or `None` if no path could be explained by
/// its tags (an untagged library, say).
#[must_use]
pub fn detect(samples: &[Sample]) -> Option<DetectedLayout> {
    let patterns: Vec<(usize, Pattern)> =
        samples.iter().enumerate().filter_map(|(i, s)| pattern(s).map(|p| (i, p))).collect();

    // A template with an optional year also covers the same paths without one.
    let supports = |template: &Pattern, candidate: &Pattern| {
        candidate.template == template.template
            || template.without_year.as_deref() == Some(candidate.plain.as_str())
            || candidate.without_year.as_deref() == Some(template.template.as_str())
    };
    let best = patterns
        .iter()
        .map(|(_, p)| p)
        .max_by_key(|p| (patterns.iter().filter(|(_, c)| supports(p, c)).count(), p.without_year.is_some()))?;
    let fitting: Vec<&(usize, Pattern)> = patterns.iter().filter(|(_, c)| supports(best, c)).collect();

    let mut digits: BTreeMap<usize, usize> = BTreeMap::new();
    for (_, p) in &fitting {
        if let Some(d) = p.track_digits {
            *digits.entry(d).or_default() += 1;
        }
    }
    let track_padding = digits.iter().max_by_key(|(d, n)| (**n, **d)).map_or(2, |(d, _)| *d);
    let multi_disc = if fitting.iter().any(|(_, p)| p.disc_prefix) {
        MultiDisc::DiscPrefix
    } else if fitting.iter().any(|(i, p)| p.disc_folder || samples[*i].tags.disc_total.is_some_and(|n| n > 1)) {
        MultiDisc::PerDisc
    } else {
        MultiDisc::DiscPrefix
    };

    Some(DetectedLayout {
        template: best.template.clone(),
        options: NamingOptions {
            track_padding: u8::try_from(track_padding.clamp(1, 4)).unwrap_or(2),
            multi_disc,
            ..NamingOptions::default()
        },
        matching: fitting.len(),
        sampled: samples.len(),
        examples: fitting.iter().take(3).map(|(i, _)| samples[*i].path.clone()).collect(),
    })
}

/// One path turned back into a template.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    /// With any year made optional.
    template: String,
    /// Exactly as the path was, without optional groups.
    plain: String,
    /// The template with its optional year group removed, when it has one.
    without_year: Option<String>,
    track_digits: Option<usize>,
    /// Tracks numbered like `1-02` on multi-disc releases.
    disc_prefix: bool,
    /// A folder per disc, such as `CD1`.
    disc_folder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Token(&'static str),
}

fn pattern(sample: &Sample) -> Option<Pattern> {
    let tags = &sample.tags;
    let components: Vec<&str> = sample.path.split('/').filter(|c| !c.is_empty()).collect();
    let last = components.len().checked_sub(1)?;
    let mut used: Vec<&'static str> = Vec::new();
    let mut result = Pattern {
        template: String::new(),
        plain: String::new(),
        without_year: None,
        track_digits: None,
        disc_prefix: false,
        disc_folder: false,
    };
    let (mut template, mut plain, mut without_year) = (Vec::new(), Vec::new(), Vec::new());

    for (i, component) in components.iter().enumerate() {
        let is_file = i == last;
        let mut pieces = vec![Piece::Text((*component).to_owned())];

        let order: [&'static str; 4] = if is_file {
            ["title", "album_artist", "artist", "album"]
        } else {
            ["album_artist", "artist", "album", "title"]
        };
        // Tokens not yet used anywhere in the path go first: in `Weezer/Weezer`, the
        // second folder is the album, not the artist again.
        let mut order: Vec<&'static str> = order.to_vec();
        order.sort_by_key(|name| used.contains(name));
        for name in order {
            let value = match name {
                "title" => tags.title.as_deref(),
                "album_artist" => tags.album_artist.as_deref(),
                "artist" => tags.artist.as_deref(),
                _ => tags.album.as_deref(),
            };
            // The track artist is only worth telling apart when it differs.
            if name == "artist"
                && tags.artist.as_deref().map(str::to_lowercase) == tags.album_artist.as_deref().map(str::to_lowercase)
            {
                continue;
            }
            let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else { continue };
            // Folders often drop bracketed extras from tags: "[1995] Twoism [vinyl rip]".
            let bare = without_brackets(value);
            if replace_words(&mut pieces, value, name)
                || (!bare.is_empty() && bare != value && replace_words(&mut pieces, &bare, name))
            {
                used.push(name);
            }
        }
        if let Some(year) = tags.year {
            replace_number(&mut pieces, u32::from(year), Some(4), "year");
        }
        if let Some(codec) = &sample.codec {
            replace_words(&mut pieces, codec, "codec");
        }
        if let Some(track) = tags.track {
            if is_file {
                let disc =
                    tags.disc.filter(|_| tags.disc_total.is_some_and(|n| n > 1) || tags.disc.is_some_and(|d| d > 1));
                if let Some(digits) = disc.and_then(|disc| replace_disc_track(&mut pieces, disc, track)) {
                    result.disc_prefix = true;
                    result.track_digits = Some(digits);
                } else if let Some(digits) = replace_number(&mut pieces, track, None, "track") {
                    result.track_digits = Some(digits);
                }
            } else if let Some(disc) = tags.disc {
                let lower = component.to_lowercase();
                if (lower.contains("cd") || lower.contains("disc") || lower.contains("disk"))
                    && replace_number(&mut pieces, disc, None, "disc").is_some()
                {
                    result.disc_folder = true;
                }
            }
        }

        let has = |token| pieces.contains(&Piece::Token(token));
        if is_file && !has("title") {
            return None;
        }
        plain.push(render(&pieces));
        let (with, without) = optional_year(&pieces).unwrap_or_else(|| (render(&pieces), render(&pieces)));
        template.push(with);
        without_year.push(without);
    }
    if !used.contains(&"album") {
        return None;
    }
    result.template = template.join("/");
    result.plain = plain.join("/");
    let without_year = without_year.join("/");
    result.without_year = (without_year != result.template).then_some(without_year);
    Some(result)
}

/// Replace the first whole-word, case-insensitive occurrence of `value` in the
/// text pieces with `token`. Characters naming replaces (`:` `/` `?` …) match `_`
/// and each other, since the library may already have had them replaced.
fn replace_words(pieces: &mut Vec<Piece>, value: &str, token: &'static str) -> bool {
    let needle: Vec<char> = value.chars().map(loose).collect();
    for index in 0..pieces.len() {
        let Piece::Text(text) = &pieces[index] else { continue };
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let loose_chars: Vec<char> = chars.iter().map(|(_, c)| loose(*c)).collect();
        let Some(start) = (0..=loose_chars.len().saturating_sub(needle.len())).find(|&at| {
            loose_chars.len() >= needle.len()
                && loose_chars[at..at + needle.len()] == needle[..]
                && (at == 0 || !chars[at - 1].1.is_alphanumeric())
                && chars.get(at + needle.len()).is_none_or(|(_, c)| !c.is_alphanumeric())
        }) else {
            continue;
        };
        let from = chars[start].0;
        let to = chars.get(start + needle.len()).map_or(text.len(), |(i, _)| *i);
        split_in(pieces, index, from, to, token);
        return true;
    }
    false
}

fn without_brackets(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0usize;
    for c in value.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn loose(c: char) -> char {
    if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '_') {
        '_'
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Replace the first standalone number equal to `value` (with exactly `width`
/// digits, if given). Returns how many digits it had.
fn replace_number(pieces: &mut Vec<Piece>, value: u32, width: Option<usize>, token: &'static str) -> Option<usize> {
    for index in 0..pieces.len() {
        let Piece::Text(text) = &pieces[index] else { continue };
        let found = digit_runs(text).into_iter().find(|&(from, to)| {
            width.is_none_or(|w| to - from == w) && to - from <= 4 && text[from..to].parse::<u32>().ok() == Some(value)
        });
        if let Some((from, to)) = found {
            split_in(pieces, index, from, to, token);
            return Some(to - from);
        }
    }
    None
}

/// Replace `disc-track` (or `disc.track`) numbering such as `1-02` with `{track}`.
/// Returns the track's digits.
fn replace_disc_track(pieces: &mut Vec<Piece>, disc: u32, track: u32) -> Option<usize> {
    for index in 0..pieces.len() {
        let Piece::Text(text) = &pieces[index] else { continue };
        let runs = digit_runs(text);
        let found = runs.windows(2).find(|pair| {
            let ((a_from, a_to), (b_from, b_to)) = (pair[0], pair[1]);
            b_from == a_to + 1
                && matches!(&text[a_to..b_from], "-" | ".")
                && text[a_from..a_to].parse::<u32>().ok() == Some(disc)
                && text[b_from..b_to].parse::<u32>().ok() == Some(track)
        });
        if let Some(pair) = found {
            let (from, digits) = (pair[0].0, pair[1].1 - pair[1].0);
            split_in(pieces, index, from, pair[1].1, "track");
            return Some(digits);
        }
    }
    None
}

/// Byte ranges of runs of ASCII digits.
fn digit_runs(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    runs
}

fn split_in(pieces: &mut Vec<Piece>, index: usize, from: usize, to: usize, token: &'static str) {
    let Piece::Text(text) = &pieces[index] else { return };
    let (before, after) = (text[..from].to_owned(), text[to..].to_owned());
    let mut replacement = Vec::with_capacity(3);
    if !before.is_empty() {
        replacement.push(Piece::Text(before));
    }
    replacement.push(Piece::Token(token));
    if !after.is_empty() {
        replacement.push(Piece::Text(after));
    }
    pieces.splice(index..=index, replacement);
}

fn escape(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}").replace('[', "[[").replace(']', "]]")
}

fn render(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => escape(text),
            Piece::Token(token) => format!("{{{token}}}"),
        })
        .collect()
}

/// The component with `{year}` and the separator beside it in an optional group,
/// and without them: `{year} - {album}` becomes `[{year} - ]{album}` and `{album}`.
fn optional_year(pieces: &[Piece]) -> Option<(String, String)> {
    let at = pieces.iter().position(|p| *p == Piece::Token("year"))?;
    // Text right before and after the year, if those pieces are text.
    let before_index = at.checked_sub(1).filter(|&i| matches!(pieces[i], Piece::Text(_)));
    let after_index = Some(at + 1).filter(|&i| matches!(pieces.get(i), Some(Piece::Text(_))));
    let text = |i: Option<usize>| match i.map(|i| &pieces[i]) {
        Some(Piece::Text(t)) => t.as_str(),
        _ => "",
    };
    let (before, after) = (text(before_index), text(after_index));
    let separator = |c: char| c.is_whitespace() || matches!(c, '-' | '.' | '_' | ',');

    // How much of `before` stays outside the group, and how much of `after` joins it.
    let bracketed = |open: char, close: char| before.ends_with(open) && after.starts_with(close);
    let (keep_before, group_after) = if bracketed('(', ')') || bracketed('[', ']') {
        // ` ({year})`: the brackets and the space before them.
        (before[..before.len() - 1].trim_end().len(), 1)
    } else if after.starts_with(separator) {
        // `{year} - {album}`
        (before.len(), after.len() - after.trim_start_matches(separator).len())
    } else {
        // `{album} - {year}`
        (before.trim_end_matches(separator).len(), 0)
    };

    let first = before_index.unwrap_or(at);
    let last = after_index.unwrap_or(at);
    let outside = render(&pieces[..first]);
    let rest = render(&pieces[last + 1..]);
    let with = format!(
        "{outside}{}[{}{{year}}{}]{}{rest}",
        escape(&before[..keep_before]),
        escape(&before[keep_before..]),
        escape(&after[..group_after]),
        escape(&after[group_after..]),
    );
    let without = format!("{outside}{}{}{rest}", escape(&before[..keep_before]), escape(&after[group_after..]));
    Some((with, without))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::{Template, TrackFields};

    fn sample(
        path: &str,
        artist: &str,
        album: &str,
        year: Option<u16>,
        disc: (u32, u32),
        track: u32,
        title: &str,
    ) -> Sample {
        Sample {
            path: path.into(),
            tags: Tags {
                title: Some(title.into()),
                artist: Some(artist.into()),
                album_artist: Some(artist.into()),
                album: Some(album.into()),
                year,
                track: Some(track),
                disc: Some(disc.0),
                disc_total: Some(disc.1),
                ..Tags::default()
            },
            codec: Some("FLAC".into()),
        }
    }

    #[test]
    fn recognises_a_common_layout_with_an_optional_year() {
        let samples = [
            sample(
                "Radiohead/1997 - OK Computer/02 - Paranoid Android",
                "Radiohead",
                "OK Computer",
                Some(1997),
                (1, 1),
                2,
                "Paranoid Android",
            ),
            sample("Burial/2007 - Untrue/03 - Near Dark", "Burial", "Untrue", Some(2007), (1, 1), 3, "Near Dark"),
            sample("Weezer/Weezer/01 - My Name Is Jonas", "Weezer", "Weezer", None, (1, 1), 1, "My Name Is Jonas"),
            sample(
                "AC_DC/1980 - Back in Black/06 - Back in Black",
                "AC/DC",
                "Back in Black",
                Some(1980),
                (1, 1),
                6,
                "Back in Black",
            ),
            sample(
                "Boards of Canada/1995 - Twoism/01 - Sixtyniner",
                "Boards of Canada",
                "[1995] Twoism [vinyl rip]",
                Some(1995),
                (1, 1),
                1,
                "Sixtyniner",
            ),
            sample("Unsorted/track1", "Someone", "Something", None, (1, 1), 1, "Else"),
        ];
        let layout = detect(&samples).unwrap();
        assert_eq!(layout.template, "{album_artist}/[{year} - ]{album}/{track} - {title}");
        assert_eq!((layout.matching, layout.sampled), (5, 6));
        assert_eq!(layout.options.track_padding, 2);
        Template::parse(&layout.template).unwrap();
    }

    #[test]
    fn recognises_disc_prefixes_brackets_and_codecs() {
        let samples = [
            sample(
                "Pink Floyd - The Wall (1979) [FLAC]/1-03 Another Brick in the Wall",
                "Pink Floyd",
                "The Wall",
                Some(1979),
                (1, 2),
                3,
                "Another Brick in the Wall",
            ),
            sample(
                "Pink Floyd - The Wall (1979) [FLAC]/2-01 Hey You",
                "Pink Floyd",
                "The Wall",
                Some(1979),
                (2, 2),
                1,
                "Hey You",
            ),
            sample(
                "Air - Moon Safari (1998) [FLAC]/04 Kelly Watch the Stars",
                "Air",
                "Moon Safari",
                Some(1998),
                (1, 1),
                4,
                "Kelly Watch the Stars",
            ),
        ];
        let layout = detect(&samples).unwrap();
        assert_eq!(layout.template, "{album_artist} - {album}[ ({year})] [[{codec}]]/{track} {title}");
        assert_eq!(layout.options.multi_disc, MultiDisc::DiscPrefix);
        assert_eq!(layout.matching, 3);

        // The detected template renders the original path back.
        let template = Template::parse(&layout.template).unwrap();
        let fields = TrackFields {
            title: "Hey You".into(),
            album_artist: "Pink Floyd".into(),
            album: "The Wall".into(),
            year: Some(1979),
            track: 1,
            disc: 2,
            disc_count: 2,
            codec: "FLAC".into(),
            ..TrackFields::default()
        };
        assert_eq!(template.render(&fields, &layout.options), "Pink Floyd - The Wall (1979) [FLAC]/2-01 Hey You");
    }

    #[test]
    fn recognises_disc_folders_and_padding() {
        let samples = [
            sample("Various/Now 100/CD2/005. Song", "Various", "Now 100", None, (2, 2), 5, "Song"),
            sample("Various/Now 99/CD1/012. Tune", "Various", "Now 99", None, (1, 2), 12, "Tune"),
        ];
        let layout = detect(&samples).unwrap();
        assert_eq!(layout.template, "{album_artist}/{album}/CD{disc}/{track}. {title}");
        assert_eq!(layout.options.multi_disc, MultiDisc::PerDisc);
        assert_eq!(layout.options.track_padding, 3);
    }

    #[test]
    fn untagged_libraries_have_no_layout() {
        let untagged = Sample { path: "a/b/c".into(), ..Sample::default() };
        assert_eq!(detect(&[untagged]), None);
        assert_eq!(detect(&[]), None);
    }
}
