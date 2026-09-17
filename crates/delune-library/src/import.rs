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
use crate::merge::{self, ExistingAlbum, ListedTrack, Renumber};
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
    /// The library's copy of this album, when these tracks fill gaps in it.
    pub existing: Option<ExistingAlbum>,
    /// The album's current tracklist, which numbers tracks added to an existing album.
    pub tracklist: Vec<ListedTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    pub source: PathBuf,
    /// Relative to the library root, `/`-separated.
    pub destination: String,
    pub fields: TrackFields,
    /// The library's copy of this track, in lower quality, which this one replaces.
    /// It goes to the library's trash, not away.
    #[serde(default)]
    pub replaces: Option<Replaced>,
    /// The library already has this track in the same or better quality, so it isn't
    /// imported.
    #[serde(default)]
    pub skip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replaced {
    /// Relative to the library root, `/`-separated.
    pub file: String,
    pub quality: Option<delune_core::Quality>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub tracks: Vec<PlannedFile>,
    /// Cover image copied next to the tracks as `cover.<ext>`.
    pub cover: Option<(PathBuf, String)>,
    /// Things a reviewer should know, in plain language.
    pub warnings: Vec<String>,
    /// Files already in the library that move to their new place in the tracklist.
    #[serde(default)]
    pub renumber: Vec<Renumber>,
    /// Write the planned album tags into the files, so they join the existing album.
    #[serde(default)]
    pub retag: bool,
}

/// Decide where every file goes.
#[must_use]
#[allow(clippy::too_many_lines, reason = "one pass that decides every field of every file")]
pub fn plan(
    tracks: &[StagedTrack],
    images: &[PathBuf],
    context: &ReleaseContext,
    template: &Template,
    options: &NamingOptions,
) -> Plan {
    let mut warnings = Vec::new();
    let existing = context.existing.as_ref();
    let tracklist = existing.filter(|e| merge::usable(&context.tracklist, e)).map(|_| context.tracklist.as_slice());

    let (album_artist, album) = identity(tracks, context, &mut warnings);

    if tracks.iter().any(|t| t.info.tags.title.is_none()) {
        warnings.push("Some tracks have no title tag; their titles come from the file names.".into());
    }

    // Disc numbers and per-disc track counts, for multi-disc naming.
    let discs: Vec<u32> = if tracklist.is_some() {
        vec![1; tracks.len()]
    } else {
        tracks.iter().map(|t| t.info.tags.disc.unwrap_or(1).max(1)).collect()
    };
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

    let matches = already_there(tracks, existing);

    let mut used = HashSet::new();
    let planned: Vec<PlannedFile> = tracks
        .iter()
        .zip(&discs)
        .enumerate()
        .map(|(index, (track, &disc))| {
            let tags = &track.info.tags;
            let (name_number, name_title) = parse_file_name(&track.path);
            let quality = track.info.quality;
            let title = tags.title.clone().unwrap_or(name_title);
            let listed = tracklist.and_then(|list| merge::position(list, &title));
            let fields = TrackFields {
                title,
                artist: tags.artist.clone().unwrap_or_else(|| album_artist.clone()),
                album_artist: album_artist.clone(),
                album: album.clone(),
                edition: None,
                year: existing.map_or(tags.year, |e| e.year.or(tags.year)),
                track: listed.or(tags.track).or(name_number).unwrap_or_else(|| u32::try_from(index + 1).unwrap_or(0)),
                disc,
                disc_count: if tracklist.is_some() { 1 } else { disc_count },
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
            let rendered = template.render(&fields, options);
            // Into the album's existing folder, whatever the template would make of it.
            let stem = match existing {
                Some(e) => format!("{}/{}", e.folder, rendered.rsplit('/').next().unwrap_or(&rendered)),
                None => rendered,
            };
            let mut destination = format!("{stem}.{extension}");
            let mut n = 2;
            while !used.insert(destination.to_lowercase()) {
                destination = format!("{stem} ({n}).{extension}");
                n += 1;
            }
            let (replaces, skip) = match (existing, matches[index]) {
                (Some(e), Some(i)) => {
                    let old = &e.tracks[i];
                    if old.quality.is_none_or(|q| quality.rank() > q.rank()) {
                        (Some(Replaced { file: format!("{}/{}", e.folder, old.file), quality: old.quality }), false)
                    } else {
                        (None, true)
                    }
                }
                _ => (None, false),
            };
            PlannedFile { source: track.path.clone(), destination, fields, replaces, skip }
        })
        .collect();

    let skipped = planned.iter().filter(|p| p.skip).count();
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} track(s) are already in your library in the same or better quality and won't be imported."
        ));
    }
    let replacing = planned.iter().filter(|p| p.replaces.is_some()).count();
    if replacing > 0 {
        warnings.push(format!(
            "{replacing} track(s) replace lower-quality copies in your library; those go to the library's trash."
        ));
    }

    if used.len() < planned.len() || planned.iter().any(|p| p.destination.contains(" (2).")) {
        warnings.push("Two tracks would get the same name; a number was added to keep both.".into());
    }

    let cover = pick_cover(images).and_then(|image| {
        let folder = planned.first()?.destination.rsplit_once('/').map(|(dir, _)| dir.to_owned())?;
        let extension = image.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_ascii_lowercase();
        Some((image, format!("{folder}/cover.{extension}")))
    });

    let leaving: HashSet<String> =
        planned.iter().filter_map(|p| p.replaces.as_ref()).map(|r| r.file.to_lowercase()).collect();
    let renumber = match (existing, tracklist) {
        (Some(e), Some(list)) => renumbered(e, list, template, options, &album_artist, &album)
            .into_iter()
            .filter(|r| !leaving.contains(&r.from.to_lowercase()))
            .collect(),
        _ => Vec::new(),
    };
    if !renumber.is_empty() {
        warnings.push(format!(
            "The album's track order has changed; {} track(s) already in your library get new numbers.",
            renumber.len()
        ));
    }

    Plan { tracks: planned, cover, warnings, renumber, retag: existing.is_some() }
}

/// For each staged track, the existing track it duplicates, if any. Each existing
/// track is matched once, exact titles first.
fn already_there(tracks: &[StagedTrack], existing: Option<&ExistingAlbum>) -> Vec<Option<usize>> {
    let mut claimed: HashSet<usize> = HashSet::new();
    tracks
        .iter()
        .map(|track| {
            let existing = existing?;
            let title = track.info.tags.title.clone().unwrap_or_else(|| parse_file_name(&track.path).1);
            let key = merge::title_key(&title);
            let candidates = || existing.tracks.iter().enumerate().filter(|(i, _)| !claimed.contains(i));
            let found = candidates()
                .find(|(_, t)| merge::title_key(&t.title) == key)
                .or_else(|| {
                    let mut loose = candidates().filter(|(_, t)| merge::same_song(&merge::title_key(&t.title), &key));
                    let first = loose.next()?;
                    loose.next().is_none().then_some(first)
                })
                .map(|(i, _)| i);
            if let Some(i) = found {
                claimed.insert(i);
            }
            found
        })
        .collect()
}

/// Existing tracks whose number no longer matches the tracklist, and their new names.
fn renumbered(
    existing: &ExistingAlbum,
    tracklist: &[ListedTrack],
    template: &Template,
    options: &NamingOptions,
    album_artist: &str,
    album: &str,
) -> Vec<Renumber> {
    existing
        .tracks
        .iter()
        .filter_map(|t| {
            let position = merge::position(tracklist, &t.title)?;
            if t.track == Some(position) {
                return None;
            }
            let quality = t.quality;
            let fields = TrackFields {
                title: t.title.clone(),
                artist: album_artist.to_owned(),
                album_artist: album_artist.to_owned(),
                album: album.to_owned(),
                year: existing.year,
                track: position,
                disc: 1,
                disc_count: 1,
                codec: quality.map(|q| q.codec.label().to_owned()).unwrap_or_default(),
                quality: quality.map(|q| q.to_string()).unwrap_or_default(),
                bit_depth: quality.and_then(|q| q.bit_depth),
                sample_rate: quality.and_then(|q| q.sample_rate),
                bitrate: quality.and_then(|q| q.bitrate_kbps),
                ..TrackFields::default()
            };
            let rendered = template.render(&fields, options);
            let name = rendered.rsplit('/').next().unwrap_or(&rendered);
            let extension = t.file.rsplit_once('.').map_or("", |(_, e)| e);
            Some(Renumber {
                from: format!("{}/{}", existing.folder, t.file),
                to: format!("{}/{name}.{}", existing.folder, extension.to_ascii_lowercase()),
                track: position,
                old_track: t.track,
            })
        })
        .collect()
}

/// The album artist and album name the tracks are filed under, noting any guesswork.
fn identity(tracks: &[StagedTrack], context: &ReleaseContext, warnings: &mut Vec<String>) -> (String, String) {
    let existing = context.existing.as_ref();
    let album_artist = existing
        .map(|e| e.album_artist.clone())
        .or_else(|| tracks.iter().find_map(|t| t.info.tags.album_artist.clone()))
        .or_else(|| most_common(tracks.iter().filter_map(|t| t.info.tags.artist.clone())))
        .or_else(|| context.artist.clone())
        .unwrap_or_else(|| {
            warnings.push("No artist in the tags or folder names; using “Unknown Artist”.".into());
            "Unknown Artist".into()
        });
    let raw_album =
        most_common(tracks.iter().filter_map(|t| t.info.tags.album.clone())).unwrap_or_else(|| context.album.clone());
    let album = existing.map_or_else(|| clean_album(&raw_album), |e| e.album.clone());
    if let Some(e) = existing {
        warnings.push(format!(
            "Your library already has {} of this album; these tracks join it in {}.",
            e.tracks.len(),
            e.folder
        ));
    } else if album != raw_album.trim() {
        warnings
            .push(format!("The album name “{raw_album}” looked like a folder name, so it was tidied to “{album}”."));
    }

    (album_artist, album)
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
pub(crate) fn parse_file_name(path: &Path) -> (Option<u32>, String) {
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

/// Files in `plan` whose destination already exists under `root`, not counting files
/// the plan moves out of the way first.
pub fn conflicts(plan: &Plan, root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let vacated: HashSet<String> = plan
        .renumber
        .iter()
        .map(|r| r.from.to_lowercase())
        .chain(plan.tracks.iter().filter_map(|t| t.replaces.as_ref()).map(|r| r.file.to_lowercase()))
        .collect();
    let taken: HashSet<String> = plan.renumber.iter().map(|r| r.to.to_lowercase()).collect();
    let mut existing = Vec::new();
    let renames = plan.renumber.iter().map(|r| (r.to.as_str(), true));
    let arriving = plan.tracks.iter().filter(|t| !t.skip).map(|t| (t.destination.as_str(), false));
    for (destination, renaming) in arriving.chain(renames) {
        let path = resolve(root, destination)?;
        let key = destination.to_lowercase();
        let freed = vacated.contains(&key) && (renaming || !taken.contains(&key));
        // A new track can't take a name an existing track is being renumbered to.
        if (path.exists() && !freed) || (!renaming && taken.contains(&key)) {
            existing.push(path);
        }
    }
    Ok(existing)
}

/// What the album tags of a track added to an existing album should say.
fn album_tags(fields: &TrackFields) -> crate::extras::AlbumTags {
    crate::extras::AlbumTags {
        album: Some(fields.album.clone()),
        album_artist: Some(fields.album_artist.clone()),
        year: fields.year,
        track: Some(fields.track),
        disc: Some(fields.disc),
    }
}

/// Move every planned file into `root`, renumbering existing tracks first when the
/// plan says so. Nothing moves if any destination is taken, and a failure part way
/// puts everything back. Returns the final paths of the new tracks.
#[allow(clippy::too_many_lines, reason = "each step pairs with its own undo")]
pub fn execute(plan: &Plan, root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let existing = conflicts(plan, root)?;
    if !existing.is_empty() {
        return Err(ImportError::Conflicts(existing));
    }

    // Renumber in two steps, through temporary names, so swapped numbers can't collide.
    let mut renamed: Vec<(&Renumber, PathBuf, PathBuf)> = Vec::new();
    let mut parked: Vec<(&Renumber, PathBuf, PathBuf)> = Vec::new();
    let undo_renames = |parked: &[(&Renumber, PathBuf, PathBuf)], renamed: &[(&Renumber, PathBuf, PathBuf)]| {
        for (r, from, to) in renamed {
            if let Some(old) = r.old_track {
                let _ = crate::extras::write_album_tags(
                    to,
                    &crate::extras::AlbumTags { track: Some(old), ..Default::default() },
                );
            }
            if let Err(undo) = move_file(to, from) {
                tracing::warn!(%undo, path = %to.display(), "couldn't undo renumbering");
            }
        }
        for (_, from, temporary) in parked {
            if let Err(undo) = move_file(temporary, from) {
                tracing::warn!(%undo, path = %temporary.display(), "couldn't undo renumbering");
            }
        }
    };
    for r in &plan.renumber {
        let step = resolve(root, &r.from).and_then(|from| {
            let temporary = from.with_extension(format!(
                "{}.delune-renumber",
                from.extension().and_then(|e| e.to_str()).unwrap_or("audio")
            ));
            move_file(&from, &temporary)?;
            Ok((from, temporary))
        });
        match step {
            Ok((from, temporary)) => parked.push((r, from, temporary)),
            Err(error) => {
                undo_renames(&parked, &[]);
                return Err(error);
            }
        }
    }
    while let Some((r, from, temporary)) = parked.pop() {
        let step = resolve(root, &r.to).and_then(|to| {
            move_file(&temporary, &to)?;
            Ok(to)
        });
        match step {
            Ok(to) => {
                let tags = crate::extras::AlbumTags { track: Some(r.track), ..Default::default() };
                if let Err(error) = crate::extras::write_album_tags(&to, &tags) {
                    tracing::warn!(%error, path = %to.display(), "couldn't write the new track number");
                }
                renamed.push((r, from, to));
            }
            Err(error) => {
                parked.push((r, from, temporary));
                undo_renames(&parked, &renamed);
                return Err(error);
            }
        }
    }

    // Lower-quality copies step aside into the trash before their replacements arrive.
    let batch = crate::trash::batch_name();
    let trashed = match trash_replaced(plan, root, &batch) {
        Ok(trashed) => trashed,
        Err(error) => {
            undo_renames(&[], &renamed);
            return Err(error);
        }
    };
    let undo_trash = |trashed: bool| {
        if trashed && let Err(undo) = crate::trash::restore(root, &batch) {
            tracing::warn!(%undo, "couldn't put replaced tracks back");
        }
    };

    let mut imported = Vec::with_capacity(plan.tracks.len());
    let mut arrived: Vec<(&PlannedFile, PathBuf)> = Vec::new();
    for track in plan.tracks.iter().filter(|t| !t.skip) {
        let moved = resolve(root, &track.destination).and_then(|destination| {
            move_file(&track.source, &destination)?;
            Ok(destination)
        });
        match moved {
            Ok(destination) => {
                if plan.retag
                    && let Err(error) = crate::extras::write_album_tags(&destination, &album_tags(&track.fields))
                {
                    tracing::warn!(%error, path = %destination.display(), "couldn't tag an added track");
                }
                imported.push(destination.clone());
                arrived.push((track, destination));
            }
            Err(error) => {
                // Half an album in the library would block importing it again; put back
                // what already moved so the next try starts clean.
                for (track, destination) in &arrived {
                    if let Err(undo) = move_file(destination, &track.source) {
                        tracing::warn!(%undo, path = %destination.display(), "couldn't undo part of an import");
                    }
                }
                undo_trash(trashed);
                undo_renames(&[], &renamed);
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

/// Move the files `plan` replaces into trash batch `batch`. Whether anything moved; on
/// failure, what moved is put back.
fn trash_replaced(plan: &Plan, root: &Path, batch: &str) -> Result<bool, ImportError> {
    let mut trashed = false;
    for replaced in plan.tracks.iter().filter(|t| !t.skip).filter_map(|t| t.replaces.as_ref()) {
        if !resolve(root, &replaced.file)?.exists() {
            continue;
        }
        trashed = true;
        if let Err(source) = crate::trash::put(root, &replaced.file, batch) {
            let _ = crate::trash::restore(root, batch);
            return Err(ImportError::Io { path: root.join(&replaced.file), source });
        }
    }
    Ok(trashed)
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
            ..ReleaseContext::default()
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
    fn gaps_fill_the_existing_album_and_follow_its_new_order() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let album_dir = root.path().join("Fred again..").join("USB");
        fs::create_dir_all(&album_dir).unwrap();
        // In the library: "Kyle" was 1 and "Jungle" 2; the artist has since put a new track first
        // and swapped those two.
        fs::write(album_dir.join("01 - Kyle.flac"), "kyle").unwrap();
        fs::write(album_dir.join("02 - Jungle.flac"), "jungle").unwrap();
        let existing = merge::find_existing(
            root.path(),
            &Template::parse("{album_artist}/{album}/{track} - {title}").unwrap(),
            &NamingOptions::default(),
            "Fred again..",
            "USB",
            None,
        )
        .unwrap();
        let tracklist = ["Lights Burn Dimmer", "Kyle", "Jungle"]
            .iter()
            .enumerate()
            .map(|(i, t)| ListedTrack { position: u32::try_from(i + 1).unwrap(), title: (*t).into() })
            .rev()
            .collect::<Vec<_>>();
        // Swap the listing so Jungle is 2 and Kyle 3.
        let tracklist: Vec<ListedTrack> = tracklist
            .into_iter()
            .map(|t| match t.title.as_str() {
                "Kyle" => ListedTrack { position: 3, ..t },
                "Jungle" => ListedTrack { position: 2, ..t },
                _ => t,
            })
            .collect();
        // The download is tagged differently: another album name, and its own numbering.
        let tracks = [track(
            staging.path(),
            "07 Lights Burn Dimmer.flac",
            tags("Fred again", "USB (2022)", "Lights Burn Dimmer", 7, None),
        )];
        let context = ReleaseContext {
            artist: Some("Fred again".into()),
            album: "USB (2022)".into(),
            source: "Soulseek".into(),
            existing: Some(existing),
            tracklist,
        };
        let t = Template::parse("{album_artist}/{album}/{track} - {title}").unwrap();
        let plan = plan(&tracks, &[], &context, &t, &NamingOptions::default());
        assert_eq!(plan.tracks[0].destination, "Fred again../USB/01 - Lights Burn Dimmer.flac");
        assert_eq!(plan.tracks[0].fields.album, "USB");
        assert_eq!(plan.tracks[0].fields.album_artist, "Fred again..");
        assert!(plan.retag);
        let moves: Vec<(&str, &str)> = plan.renumber.iter().map(|r| (r.from.as_str(), r.to.as_str())).collect();
        assert_eq!(
            moves,
            [("Fred again../USB/01 - Kyle.flac", "Fred again../USB/03 - Kyle.flac")],
            "Jungle keeps number 2; only Kyle moves"
        );

        // The new track takes 01, which Kyle is leaving.
        assert!(conflicts(&plan, root.path()).unwrap().is_empty());
        execute(&plan, root.path()).unwrap();
        let mut names: Vec<String> =
            fs::read_dir(&album_dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["01 - Lights Burn Dimmer.flac", "02 - Jungle.flac", "03 - Kyle.flac"]);
        assert_eq!(fs::read_to_string(album_dir.join("03 - Kyle.flac")).unwrap(), "kyle");
    }

    #[test]
    fn swapped_numbers_rename_cleanly_and_failures_put_everything_back() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("A/B");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("01 - One.flac"), "one").unwrap();
        fs::write(dir.join("02 - Two.flac"), "two").unwrap();
        let swap = |from: &str, to: &str, track| Renumber {
            from: format!("A/B/{from}"),
            to: format!("A/B/{to}"),
            track,
            old_track: None,
        };
        let mut plan = Plan {
            tracks: vec![],
            cover: None,
            warnings: vec![],
            renumber: vec![swap("01 - One.flac", "02 - One.flac", 2), swap("02 - Two.flac", "01 - Two.flac", 1)],
            retag: true,
        };
        assert!(conflicts(&plan, root.path()).unwrap().is_empty(), "swaps don't conflict with themselves");
        execute(&plan, root.path()).unwrap();
        assert_eq!(fs::read_to_string(dir.join("02 - One.flac")).unwrap(), "one");
        assert_eq!(fs::read_to_string(dir.join("01 - Two.flac")).unwrap(), "two");

        // A new track whose source is missing fails the import; the renames are undone.
        plan.renumber = vec![swap("02 - One.flac", "03 - One.flac", 3)];
        plan.tracks = vec![PlannedFile {
            source: root.path().join("missing.flac"),
            destination: "A/B/02 - New.flac".into(),
            fields: TrackFields::default(),
            replaces: None,
            skip: false,
        }];
        assert!(execute(&plan, root.path()).is_err());
        assert_eq!(fs::read_to_string(dir.join("02 - One.flac")).unwrap(), "one", "put back");
        assert!(!dir.join("03 - One.flac").exists());

        // Taking a name another track is moving to is a conflict.
        plan.tracks[0].destination = "A/B/03 - One.flac".into();
        assert_eq!(conflicts(&plan, root.path()).unwrap().len(), 1);
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
            replaces: None,
            skip: false,
        };
        let plan = Plan {
            tracks: vec![
                file(first.clone(), "A/B/01.flac"),
                // Vanished from staging: this move fails after the first succeeded.
                file(staging.path().join("02.flac"), "A/B/02.flac"),
            ],
            cover: None,
            warnings: vec![],
            renumber: vec![],
            retag: false,
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
                replaces: None,
                skip: false,
            }],
            cover: None,
            warnings: vec![],
            renumber: vec![],
            retag: false,
        };
        assert!(matches!(execute(&evil, library.path()), Err(ImportError::UnsafeDestination(_))));
        let absolute =
            Plan { tracks: vec![PlannedFile { destination: "/etc/passwd".into(), ..evil.tracks[0].clone() }], ..evil };
        assert!(matches!(execute(&absolute, library.path()), Err(ImportError::UnsafeDestination(_))));
    }

    #[test]
    fn better_copies_replace_old_ones_and_others_stay_out() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let album_dir = root.path().join("Fred again..").join("USB");
        fs::create_dir_all(&album_dir).unwrap();
        fs::write(album_dir.join("01 - Kyle.mp3"), "old kyle").unwrap();
        fs::write(album_dir.join("01 - Kyle.lrc"), "lyrics").unwrap();
        fs::write(album_dir.join("02 - Jungle.flac"), "good jungle").unwrap();
        let template = Template::parse("{album_artist}/{album}/{track} - {title}").unwrap();
        let mut existing =
            merge::find_existing(root.path(), &template, &NamingOptions::default(), "Fred again..", "USB", None)
                .unwrap();
        // inspect() can't read these stand-in files; give them their qualities.
        for t in &mut existing.tracks {
            t.quality = Some(if Path::new(&t.file).extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3")) {
                Quality::lossy(Codec::Mp3, 320)
            } else {
                Quality::lossless(Codec::Flac, 24, 96_000)
            });
        }
        let tracks = [
            track(staging.path(), "01 Kyle.flac", tags("Fred again..", "USB", "Kyle", 1, None)),
            track(staging.path(), "02 Jungle.flac", tags("Fred again..", "USB", "Jungle", 2, None)),
        ];
        let context = ReleaseContext {
            artist: Some("Fred again..".into()),
            album: "USB".into(),
            source: "Soulseek".into(),
            existing: Some(existing),
            tracklist: Vec::new(),
        };
        let plan = plan(&tracks, &[], &context, &template, &NamingOptions::default());
        assert_eq!(plan.tracks[0].replaces.as_ref().map(|r| r.file.as_str()), Some("Fred again../USB/01 - Kyle.mp3"));
        assert!(!plan.tracks[0].skip);
        assert!(plan.tracks[1].skip, "the library's copy is as good");
        assert!(
            conflicts(&plan, root.path()).unwrap().is_empty(),
            "the skipped track's name is taken, but it doesn't matter"
        );

        let imported = execute(&plan, root.path()).unwrap();
        assert_eq!(imported, [root.path().join("Fred again../USB/01 - Kyle.flac")]);
        assert!(!album_dir.join("01 - Kyle.mp3").exists() && !album_dir.join("01 - Kyle.lrc").exists());
        assert_eq!(fs::read_to_string(album_dir.join("02 - Jungle.flac")).unwrap(), "good jungle");
        let trash = root.path().join(crate::trash::DIR);
        let batch = fs::read_dir(&trash).unwrap().flatten().find(|e| e.path().is_dir()).unwrap().file_name();
        let mut kept = crate::trash::contents(root.path(), &batch.to_string_lossy());
        kept.sort();
        assert_eq!(kept, ["Fred again../USB/01 - Kyle.lrc", "Fred again../USB/01 - Kyle.mp3"]);
        assert!(tracks[1].path.exists(), "a skipped file stays in staging");
    }
}
