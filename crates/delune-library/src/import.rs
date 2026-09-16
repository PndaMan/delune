//! Planning and performing the move from staging into the library.
//!
//! [`plan`] is pure: given what each staged file is (from [`crate::inspect`]), the
//! release it came from, and the naming template, it decides every destination
//! path. The review screen shows that plan. [`execute`] then checks the library
//! for conflicts *before* moving anything, and moves files into place.
//!
//! Fields come from tags first, then from what delune knows about the release (the
//! folder the files came from), then from file names, so an untagged rip still
//! lands somewhere sensible.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::inspect::AudioInfo;
use crate::naming::{NamingOptions, Template, TrackFields};

/// A staged audio file and what's known about it.
#[derive(Debug, Clone)]
pub struct StagedTrack {
    pub path: PathBuf,
    pub info: AudioInfo,
}

/// What delune knows about the release beyond the files' own tags.
#[derive(Debug, Clone, Default)]
pub struct ReleaseContext {
    pub artist: Option<String>,
    pub album: String,
    /// Where it came from, for the `{source}` token.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    pub source: PathBuf,
    /// Relative to the library root, `/`-separated.
    pub destination: String,
    pub fields: TrackFields,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub tracks: Vec<PlannedFile>,
    /// Cover image copied next to the tracks as `cover.<ext>`.
    pub cover: Option<(PathBuf, String)>,
    /// Things a reviewer should know, in plain language.
    pub warnings: Vec<String>,
}

/// Decide where every file goes.
#[must_use]
pub fn plan(
    tracks: &[StagedTrack],
    images: &[PathBuf],
    context: &ReleaseContext,
    template: &Template,
    options: &NamingOptions,
) -> Plan {
    let mut warnings = Vec::new();

    let album_artist = tracks
        .iter()
        .find_map(|t| t.info.tags.album_artist.clone())
        .or_else(|| most_common(tracks.iter().filter_map(|t| t.info.tags.artist.clone())))
        .or_else(|| context.artist.clone())
        .unwrap_or_else(|| {
            warnings.push("No artist in the tags or folder names; using “Unknown Artist”.".into());
            "Unknown Artist".into()
        });
    let raw_album =
        most_common(tracks.iter().filter_map(|t| t.info.tags.album.clone())).unwrap_or_else(|| context.album.clone());
    let album = clean_album(&raw_album);
    if album != raw_album.trim() {
        warnings
            .push(format!("The album name “{raw_album}” looked like a folder name, so it was tidied to “{album}”."));
    }

    if tracks.iter().any(|t| t.info.tags.title.is_none()) {
        warnings.push("Some tracks have no title tag; their titles come from the file names.".into());
    }

    // Disc numbers and per-disc track counts, for multi-disc naming.
    let discs: Vec<u32> = tracks.iter().map(|t| t.info.tags.disc.unwrap_or(1).max(1)).collect();
    let disc_count = discs
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(tracks.iter().filter_map(|t| t.info.tags.disc_total).max().unwrap_or(1));
    let mut per_disc: BTreeMap<u32, u32> = BTreeMap::new();
    for disc in &discs {
        *per_disc.entry(*disc).or_default() += 1;
    }

    let mut used = HashSet::new();
    let planned: Vec<PlannedFile> = tracks
        .iter()
        .zip(&discs)
        .enumerate()
        .map(|(index, (track, &disc))| {
            let tags = &track.info.tags;
            let (name_number, name_title) = parse_file_name(&track.path);
            let quality = track.info.quality;
            let fields = TrackFields {
                title: tags.title.clone().unwrap_or(name_title),
                artist: tags.artist.clone().unwrap_or_else(|| album_artist.clone()),
                album_artist: album_artist.clone(),
                album: album.clone(),
                edition: None,
                year: tags.year,
                track: tags.track.or(name_number).unwrap_or_else(|| u32::try_from(index + 1).unwrap_or(0)),
                disc,
                disc_count,
                tracks_before_disc: per_disc.range(..disc).map(|(_, count)| count).sum(),
                genre: tags.genre.clone(),
                composer: None,
                label: tags.label.clone(),
                catalog: tags.catalog.clone(),
                isrc: tags.isrc.clone(),
                codec: quality.codec.label().to_owned(),
                quality: quality.to_string(),
                bit_depth: quality.bit_depth,
                sample_rate: quality.sample_rate,
                bitrate: quality.bitrate_kbps,
                source: context.source.clone(),
                mbid: tags.musicbrainz_release.clone(),
            };
            let extension = track.path.extension().and_then(|e| e.to_str()).unwrap_or("audio").to_ascii_lowercase();
            let stem = template.render(&fields, options);
            let mut destination = format!("{stem}.{extension}");
            let mut n = 2;
            while !used.insert(destination.to_lowercase()) {
                destination = format!("{stem} ({n}).{extension}");
                n += 1;
            }
            PlannedFile { source: track.path.clone(), destination, fields }
        })
        .collect();

    if used.len() < planned.len() || planned.iter().any(|p| p.destination.contains(" (2).")) {
        warnings.push("Two tracks would get the same name; a number was added to keep both.".into());
    }

    let cover = pick_cover(images).and_then(|image| {
        let folder = planned.first()?.destination.rsplit_once('/').map(|(dir, _)| dir.to_owned())?;
        let extension = image.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_ascii_lowercase();
        Some((image, format!("{folder}/cover.{extension}")))
    });

    Plan { tracks: planned, cover, warnings }
}

/// Words that mark a bracketed group as rip details rather than part of the title.
const RIP_WORDS: &[&str] = &[
    "flac", "mp3", "khz", "kbps", "bit", "24-96", "24-192", "16-44", "vinyl", "rip", "web", "cd", "lossless", "hi-res",
    "hires", "dr", "log", "cue", "remaster",
];

/// Strip rip details from an album name: `[1995] Twoism [96khz vinyl rip]` → `Twoism`.
/// Brackets that look like part of the real title (`(Deluxe Edition)`) are kept.
fn clean_album(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name.trim();
    while let Some(start) = rest.find(['[', '(', '{']) {
        let close = match rest.as_bytes()[start] {
            b'[' => ']',
            b'(' => ')',
            _ => '}',
        };
        let Some(end) = rest[start..].find(close).map(|e| start + e) else { break };
        let inner = rest[start + 1..end].to_ascii_lowercase();
        let is_year = inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit());
        let is_rip = RIP_WORDS.iter().any(|w| inner.contains(w));
        out.push_str(&rest[..start]);
        if !is_year && !is_rip {
            out.push_str(&rest[start..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    let cleaned = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches(|c: char| c == '-' || c.is_whitespace()).to_owned();
    if cleaned.is_empty() { name.trim().to_owned() } else { cleaned }
}

fn most_common(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, count)| *count).map(|(value, _)| value)
}

/// "03 - Time.flac" → (Some(3), "Time"); "A2. Oirectine.flac" → (None, "Oirectine").
fn parse_file_name(path: &Path) -> (Option<u32>, String) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().replace('_', " ");
    let trimmed = stem.trim();
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    let (number, rest) = if !digits.is_empty() && digits.len() <= 3 {
        (digits.parse().ok(), &trimmed[digits.len()..])
    } else if trimmed.len() > 2 && trimmed.as_bytes()[0].is_ascii_uppercase() && trimmed.as_bytes()[1].is_ascii_digit()
    {
        // Vinyl sides: "A1", "B12"
        let side_len = 1 + trimmed[1..].chars().take_while(char::is_ascii_digit).count();
        (None, &trimmed[side_len..])
    } else {
        (None, trimmed)
    };
    let title = rest.trim_start_matches([' ', '.', '-', ')', '_']).trim();
    (number, if title.is_empty() { trimmed.to_owned() } else { title.to_owned() })
}

fn pick_cover(images: &[PathBuf]) -> Option<PathBuf> {
    let named = |p: &&PathBuf| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| ["cover", "folder", "front", "album"].contains(&s.to_ascii_lowercase().as_str()))
    };
    images.iter().find(named).or_else(|| images.iter().max_by_key(|p| fs::metadata(p).map_or(0, |m| m.len()))).cloned()
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{} file(s) already exist in the library, e.g. {}", .0.len(), .0.first().map(|p| p.display().to_string()).unwrap_or_default())]
    Conflicts(Vec<PathBuf>),
    #[error("refusing an unsafe destination: {0}")]
    UnsafeDestination(String),
    #[error("couldn't move {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

fn resolve(root: &Path, relative: &str) -> Result<PathBuf, ImportError> {
    let path = Path::new(relative);
    if relative.is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(ImportError::UnsafeDestination(relative.to_owned()));
    }
    Ok(root.join(path))
}

/// Files in `plan` whose destination already exists under `root`.
pub fn conflicts(plan: &Plan, root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut existing = Vec::new();
    for destination in plan.tracks.iter().map(|t| t.destination.as_str()) {
        let path = resolve(root, destination)?;
        if path.exists() {
            existing.push(path);
        }
    }
    Ok(existing)
}

/// Move every planned file into `root`. Nothing moves if any track destination
/// already exists. Returns the final paths of the tracks.
pub fn execute(plan: &Plan, root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let existing = conflicts(plan, root)?;
    if !existing.is_empty() {
        return Err(ImportError::Conflicts(existing));
    }
    let mut imported = Vec::with_capacity(plan.tracks.len());
    for track in &plan.tracks {
        let moved = resolve(root, &track.destination).and_then(|destination| {
            move_file(&track.source, &destination)?;
            Ok(destination)
        });
        match moved {
            Ok(destination) => imported.push(destination),
            Err(error) => {
                // Half an album in the library would block importing it again; put back
                // what already moved so the next try starts clean.
                for (track, destination) in plan.tracks.iter().zip(&imported) {
                    if let Err(undo) = move_file(destination, &track.source) {
                        tracing::warn!(%undo, path = %destination.display(), "couldn't undo part of an import");
                    }
                }
                return Err(error);
            }
        }
    }
    if let Some((image, relative)) = &plan.cover {
        let destination = resolve(root, relative)?;
        if !destination.exists() {
            move_file(image, &destination)?;
        }
    }
    Ok(imported)
}

fn move_file(from: &Path, to: &Path) -> Result<(), ImportError> {
    let io_err = |source| ImportError::Io { path: from.to_owned(), source };
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        // Staging and library on different filesystems (a NAS mount): copy, sync, delete.
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            if let Err(e) = fs::copy(from, to).and_then(|_| fs::File::open(to)?.sync_all()) {
                // Don't leave a partial copy behind in the library.
                let _ = fs::remove_file(to);
                return Err(io_err(e));
            }
            fs::remove_file(from).map_err(io_err)
        }
        Err(e) => Err(io_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::Tags;
    use delune_core::{Codec, Quality};

    fn track(dir: &Path, name: &str, tags: Tags) -> StagedTrack {
        let path = dir.join(name);
        fs::write(&path, name).unwrap();
        StagedTrack {
            path,
            info: AudioInfo {
                quality: Quality::lossless(Codec::Flac, 24, 96_000),
                duration_secs: 300,
                channels: Some(2),
                tags,
            },
        }
    }

    fn tags(artist: &str, album: &str, title: &str, track: u32, disc: Option<u32>) -> Tags {
        Tags {
            artist: Some(artist.into()),
            album: Some(album.into()),
            title: Some(title.into()),
            track: Some(track),
            disc,
            year: Some(1997),
            ..Tags::default()
        }
    }

    fn template() -> Template {
        Template::parse("{album_artist}/{year} - {album}/{track} - {title}").unwrap()
    }

    #[test]
    fn plans_tagged_tracks_and_cover() {
        let staging = tempfile::tempdir().unwrap();
        let tracks = [
            track(staging.path(), "01.flac", tags("Radiohead", "OK Computer", "Airbag", 1, None)),
            track(staging.path(), "02.flac", tags("Radiohead", "OK Computer", "Paranoid Android", 2, None)),
        ];
        let cover = staging.path().join("folder.jpg");
        fs::write(&cover, "jpg").unwrap();
        let plan = plan(
            &tracks,
            std::slice::from_ref(&cover),
            &ReleaseContext::default(),
            &template(),
            &NamingOptions::default(),
        );
        let destinations: Vec<_> = plan.tracks.iter().map(|t| t.destination.as_str()).collect();
        assert_eq!(
            destinations,
            [
                "Radiohead/1997 - OK Computer/01 - Airbag.flac",
                "Radiohead/1997 - OK Computer/02 - Paranoid Android.flac"
            ]
        );
        assert_eq!(plan.cover, Some((cover, "Radiohead/1997 - OK Computer/cover.jpg".into())));
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn untagged_files_fall_back_to_context_and_names() {
        let staging = tempfile::tempdir().unwrap();
        let tracks = [
            track(staging.path(), "A1. Sixtyniner.flac", Tags::default()),
            track(staging.path(), "03 - Iced Cooly.flac", Tags::default()),
        ];
        let context = ReleaseContext {
            artist: Some("Boards of Canada".into()),
            album: "Twoism".into(),
            source: "Soulseek".into(),
        };
        let t = Template::parse("{album_artist}/{album}/{track} - {title}").unwrap();
        let plan = plan(&tracks, &[], &context, &t, &NamingOptions::default());
        let destinations: Vec<_> = plan.tracks.iter().map(|t| t.destination.as_str()).collect();
        assert_eq!(
            destinations,
            ["Boards of Canada/Twoism/01 - Sixtyniner.flac", "Boards of Canada/Twoism/03 - Iced Cooly.flac"]
        );
        assert_eq!(plan.warnings.len(), 1, "missing titles are reported");
    }

    #[test]
    fn album_names_lose_rip_details() {
        assert_eq!(clean_album("[1995] Twoism [96khz vinyl rip]"), "Twoism");
        assert_eq!(clean_album("OK Computer (Collector's Edition)"), "OK Computer (Collector's Edition)");
        assert_eq!(clean_album("Kid A (2000) [FLAC 24-96]"), "Kid A");
        assert_eq!(clean_album("[FLAC]"), "[FLAC]");
    }

    #[test]
    fn multi_disc_numbering() {
        let staging = tempfile::tempdir().unwrap();
        let tracks = [
            track(staging.path(), "1.flac", tags("A", "B", "One", 1, Some(1))),
            track(staging.path(), "2.flac", tags("A", "B", "Two", 2, Some(1))),
            track(staging.path(), "3.flac", tags("A", "B", "Three", 1, Some(2))),
        ];
        let t = Template::parse("{track}").unwrap();
        let continuous = NamingOptions { multi_disc: crate::naming::MultiDisc::Continuous, ..NamingOptions::default() };
        let names = |options: &NamingOptions| {
            plan(&tracks, &[], &ReleaseContext::default(), &t, options)
                .tracks
                .into_iter()
                .map(|p| p.destination)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&NamingOptions::default()), ["1-01.flac", "1-02.flac", "2-01.flac"]);
        assert_eq!(names(&continuous), ["01.flac", "02.flac", "03.flac"]);
    }

    #[test]
    fn duplicate_names_are_kept_apart() {
        let staging = tempfile::tempdir().unwrap();
        let tracks = [
            track(staging.path(), "a.flac", tags("A", "B", "Same", 1, None)),
            track(staging.path(), "b.flac", tags("A", "B", "Same", 1, None)),
        ];
        let plan = plan(
            &tracks,
            &[],
            &ReleaseContext::default(),
            &Template::parse("{title}").unwrap(),
            &NamingOptions::default(),
        );
        assert_eq!(plan.tracks[1].destination, "Same (2).flac");
        assert!(plan.warnings.iter().any(|w| w.contains("same name")));
    }

    #[test]
    fn execute_moves_files_and_refuses_conflicts() {
        let staging = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let tracks = [track(staging.path(), "01.flac", tags("A", "B", "One", 1, None))];
        let plan = plan(&tracks, &[], &ReleaseContext::default(), &template(), &NamingOptions::default());

        let imported = execute(&plan, library.path()).unwrap();
        assert_eq!(imported, [library.path().join("A/1997 - B/01 - One.flac")]);
        assert_eq!(fs::read_to_string(&imported[0]).unwrap(), "01.flac");
        assert!(!tracks[0].path.exists(), "moved, not copied");

        // Same plan again: the destination exists now, so nothing may move.
        fs::write(&tracks[0].path, "again").unwrap();
        assert!(matches!(execute(&plan, library.path()), Err(ImportError::Conflicts(c)) if c.len() == 1));
        assert!(tracks[0].path.exists());
    }

    #[test]
    fn a_failed_import_puts_back_what_moved() {
        let staging = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let first = staging.path().join("01.flac");
        fs::write(&first, "one").unwrap();
        let file = |source: PathBuf, destination: &str| PlannedFile {
            source,
            destination: destination.into(),
            fields: TrackFields::default(),
        };
        let plan = Plan {
            tracks: vec![
                file(first.clone(), "A/B/01.flac"),
                // Vanished from staging: this move fails after the first succeeded.
                file(staging.path().join("02.flac"), "A/B/02.flac"),
            ],
            cover: None,
            warnings: vec![],
        };
        assert!(matches!(execute(&plan, library.path()), Err(ImportError::Io { .. })));
        assert_eq!(fs::read_to_string(&first).unwrap(), "one", "the first track is back in staging");
        assert!(!library.path().join("A/B/01.flac").exists());
    }

    #[test]
    fn refuses_destinations_outside_the_library() {
        let library = tempfile::tempdir().unwrap();
        let evil = Plan {
            tracks: vec![PlannedFile {
                source: PathBuf::from("x"),
                destination: "../escape.flac".into(),
                fields: TrackFields::default(),
            }],
            cover: None,
            warnings: vec![],
        };
        assert!(matches!(execute(&evil, library.path()), Err(ImportError::UnsafeDestination(_))));
        let absolute =
            Plan { tracks: vec![PlannedFile { destination: "/etc/passwd".into(), ..evil.tracks[0].clone() }], ..evil };
        assert!(matches!(execute(&absolute, library.path()), Err(ImportError::UnsafeDestination(_))));
    }
}
