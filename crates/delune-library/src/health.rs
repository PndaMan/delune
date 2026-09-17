//! Checking the library's shape: albums split over several folders, and tracks that
//! are in an album twice. Fixes never delete: what goes away moves to the library's
//! trash (see [`crate::trash`]), and a whole fix can be undone.
//!
//! A scan only reads folder listings (not tags), so it stays quick on a network share.
//! Each finding carries an id made from the files it's about (names, sizes and change
//! times); a fix scans those folders again and refuses if the id no longer matches,
//! so it never acts on a library that changed since you looked.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::merge::{album_key, artist_key, title_key};
use crate::trash;

const AUDIO: &[&str] = &["flac", "alac", "wav", "aif", "aiff", "mp3", "m4a", "aac", "opus", "ogg", "oga", "wv", "ape"];

/// The most files one fix may move. A fix bigger than this is almost certainly a
/// mistake in what was found, not a real album.
pub const MAX_FIX_FILES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// One album in several folders (often under different spellings of the artist).
    SplitAlbum,
    /// The same track more than once in one folder.
    DuplicateTracks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    /// Stays the same while the same folders are involved, whatever happens to the
    /// files in them, so a finding can be ignored for good.
    pub key: String,
    pub kind: Kind,
    /// Library-relative folders involved; for a split album, the one kept comes first.
    pub folders: Vec<String>,
    /// For duplicate tracks: the copies, grouped by track.
    pub duplicates: Vec<Vec<String>>,
    /// How many audio files a fix would move or put in the trash.
    pub files: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub albums: usize,
    pub tracks: usize,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fixed {
    /// Files moved into the kept folder.
    pub moved: usize,
    /// Files put in the trash.
    pub trashed: usize,
    /// The trash batch that undoes this fix.
    pub batch: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FixError {
    #[error("The library changed since it was checked. Check again first.")]
    Changed,
    #[error("That would move {0} files, more than one fix is allowed to.")]
    TooBig(usize),
    #[error("{0}")]
    Io(#[from] io::Error),
}

pub(crate) fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
}

fn hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn audio_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_audio(p))
        .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(hidden))
        .collect();
    files.sort();
    files
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn track_key(path: &Path) -> String {
    title_key(&crate::import::parse_file_name(path).1)
}

/// Album folders: `<artist>/<album>` directories holding audio, skipping hidden ones.
fn album_dirs(root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(artists) = fs::read_dir(root) else { return out };
    for artist in artists.flatten() {
        let artist_name = artist.file_name().to_string_lossy().into_owned();
        if hidden(&artist_name) || !artist.path().is_dir() {
            continue;
        }
        let Ok(albums) = fs::read_dir(artist.path()) else { continue };
        for album in albums.flatten() {
            let album_name = album.file_name().to_string_lossy().into_owned();
            if hidden(&album_name) || !album.path().is_dir() {
                continue;
            }
            out.push((artist_name.clone(), album_name, album.path()));
        }
    }
    out.sort_by(|a, b| a.2.cmp(&b.2));
    out
}

/// An id for a set of files: changes when any of them is added, removed or modified.
fn fingerprint(kind: Kind, folders: &[PathBuf]) -> String {
    let mut hash = Sha256::new();
    hash.update(format!("{kind:?}"));
    for dir in folders {
        hash.update(dir.to_string_lossy().as_bytes());
        for file in audio_files(dir) {
            let meta = fs::metadata(&file).ok();
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            hash.update(file.to_string_lossy().as_bytes());
            hash.update(meta.map_or(0, |m| m.len()).to_le_bytes());
            hash.update(modified.to_le_bytes());
        }
    }
    hex::encode(&hash.finalize()[..12])
}

fn key_of(kind: Kind, folders: &[String]) -> String {
    let mut sorted = folders.to_vec();
    sorted.sort();
    format!("{kind:?}:{}", sorted.join("\u{1f}"))
}

/// Groups of copies of the same track in one folder.
fn duplicates_in(dir: &Path) -> Vec<Vec<PathBuf>> {
    let mut by_key: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for file in audio_files(dir) {
        let key = track_key(&file);
        if key.len() >= 2 {
            by_key.entry(key).or_default().push(file);
        }
    }
    by_key.into_values().filter(|copies| copies.len() > 1).collect()
}

/// Look the library over.
#[must_use]
pub fn scan(root: &Path) -> Scan {
    let dirs = album_dirs(root);
    let mut result = Scan::default();
    let mut groups: HashMap<(String, String), Vec<PathBuf>> = HashMap::new();
    for (artist, album, dir) in &dirs {
        let files = audio_files(dir);
        if files.is_empty() {
            continue;
        }
        result.albums += 1;
        result.tracks += files.len();
        let key = (artist_key(artist), album_key(album));
        if !key.0.is_empty() && !key.1.is_empty() {
            groups.entry(key).or_default().push(dir.clone());
        }
        let duplicates = duplicates_in(dir);
        if !duplicates.is_empty() {
            let folders = vec![dir.clone()];
            let relative_folders = vec![relative(root, dir)];
            result.findings.push(Finding {
                id: fingerprint(Kind::DuplicateTracks, &folders),
                key: key_of(Kind::DuplicateTracks, &relative_folders),
                kind: Kind::DuplicateTracks,
                folders: relative_folders,
                files: duplicates.iter().map(|d| d.len() - 1).sum(),
                duplicates: duplicates.iter().map(|d| d.iter().map(|p| relative(root, p)).collect()).collect(),
            });
        }
    }
    let mut splits: Vec<Vec<PathBuf>> = groups.into_values().filter(|g| g.len() > 1).collect();
    for group in &mut splits {
        order_split(group);
    }
    splits.sort();
    for group in splits {
        let folders: Vec<String> = group.iter().map(|d| relative(root, d)).collect();
        result.findings.push(Finding {
            id: fingerprint(Kind::SplitAlbum, &group),
            key: key_of(Kind::SplitAlbum, &folders),
            kind: Kind::SplitAlbum,
            folders,
            duplicates: Vec::new(),
            files: group.iter().skip(1).map(|d| audio_files(d).len()).sum(),
        });
    }
    result
}

/// The folder to keep first: the fullest, then the first by path.
fn order_split(group: &mut [PathBuf]) {
    group.sort_by(|a, b| audio_files(b).len().cmp(&audio_files(a).len()).then_with(|| a.cmp(b)));
}

fn rank(path: &Path) -> (u32, u64) {
    let quality = crate::inspect::inspect(path).map_or(0, |i| i.quality.rank());
    (quality, fs::metadata(path).map_or(0, |m| m.len()))
}

/// Fix finding `id`, found by an earlier [`scan`] of `root`.
///
/// # Errors
///
/// [`FixError::Changed`] when the files differ from what the scan saw (or the finding
/// is gone), [`FixError::TooBig`] past [`MAX_FIX_FILES`], or an I/O error part way,
/// after which the batch is undone.
pub fn fix(root: &Path, finding: &Finding) -> Result<Fixed, FixError> {
    let folders: Vec<PathBuf> = finding.folders.iter().map(|f| root.join(f)).collect();
    if folders.iter().any(|f| !f.starts_with(root) || f.components().any(|c| c.as_os_str() == "..")) {
        return Err(FixError::Changed);
    }
    let mut ordered = folders.clone();
    if finding.kind == Kind::SplitAlbum {
        order_split(&mut ordered);
    }
    if ordered != folders || fingerprint(finding.kind, &folders) != finding.id {
        return Err(FixError::Changed);
    }
    if finding.files > MAX_FIX_FILES {
        return Err(FixError::TooBig(finding.files));
    }
    let batch = trash::batch_name();
    let result = match finding.kind {
        Kind::DuplicateTracks => {
            keep_best(root, &folders[0], &batch).map(|trashed| Fixed { moved: 0, trashed, batch: batch.clone() })
        }
        Kind::SplitAlbum => merge_into(root, &folders, &batch),
    };
    if result.is_err() {
        let _ = trash::restore(root, &batch);
    }
    result.map_err(FixError::from)
}

/// In `dir`, keep the best copy of each track and trash the rest.
fn keep_best(root: &Path, dir: &Path, batch: &str) -> io::Result<usize> {
    let mut trashed = 0;
    for mut copies in duplicates_in(dir) {
        copies.sort_by_key(|p| std::cmp::Reverse(rank(p)));
        for extra in &copies[1..] {
            trash::put(root, &relative(root, extra), batch)?;
            trashed += 1;
        }
    }
    Ok(trashed)
}

/// Move everything from `folders[1..]` into `folders[0]`: a track the kept folder
/// already has stays in whichever copy is better, the other goes to the trash.
fn merge_into(root: &Path, folders: &[PathBuf], batch: &str) -> io::Result<Fixed> {
    let target = &folders[0];
    let mut fixed = Fixed { batch: batch.to_owned(), ..Fixed::default() };
    let album_tags = audio_files(target).first().and_then(|f| crate::inspect::inspect(f).ok()).map(|i| i.tags);
    for source in &folders[1..] {
        for file in audio_files(source) {
            let key = track_key(&file);
            let existing = audio_files(target).into_iter().find(|t| key.len() >= 2 && track_key(t) == key);
            if let Some(existing) = existing {
                let (worse, better) =
                    if rank(&file) > rank(&existing) { (existing, file.clone()) } else { (file.clone(), existing) };
                trash::put(root, &relative(root, &worse), batch)?;
                fixed.trashed += 1;
                if better == file {
                    move_in(root, &file, target, batch, album_tags.as_ref())?;
                    fixed.moved += 1;
                }
            } else {
                move_in(root, &file, target, batch, album_tags.as_ref())?;
                fixed.moved += 1;
            }
        }
        // What's left beside the music (covers, playlists, cue sheets) goes to the
        // trash too, so the emptied folder can go.
        if let Ok(entries) = fs::read_dir(source) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    trash::put(root, &relative(root, &path), batch)?;
                }
            }
        }
        let _ = fs::remove_dir(source);
        if let Some(artist) = source.parent() {
            let _ = fs::remove_dir(artist);
        }
    }
    Ok(fixed)
}

/// Move `file` (and files sharing its name, like lyrics) into `target` without
/// replacing anything, tagging it as the kept album's.
fn move_in(
    root: &Path,
    file: &Path,
    target: &Path,
    batch: &str,
    tags: Option<&crate::inspect::Tags>,
) -> io::Result<()> {
    let stem = file.file_stem().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    let parent = file.parent().unwrap_or(root);
    let mut companions: Vec<PathBuf> = fs::read_dir(parent)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.file_stem() == Some(stem.as_os_str()) && (p.as_path() == file || !is_audio(p)))
        .collect();
    companions.sort();
    for path in companions {
        let name = path.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
        let mut destination = target.join(&name);
        let mut n = 2;
        while destination.exists() {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            destination = target.join(format!("{stem} ({n}).{ext}"));
            n += 1;
        }
        fs::rename(&path, &destination)?;
        trash::record_move(root, batch, &relative(root, &path), &relative(root, &destination))?;
        if is_audio(&destination)
            && let Some(tags) = tags
        {
            let album = crate::extras::AlbumTags {
                album: tags.album.clone(),
                album_artist: tags.album_artist.clone().or_else(|| tags.artist.clone()),
                ..Default::default()
            };
            if album.album.is_some()
                && let Err(error) = crate::extras::write_album_tags(&destination, &album)
            {
                tracing::warn!(%error, path = %destination.display(), "couldn't tag a merged track");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(root: &Path, dir: &str, files: &[(&str, &str)]) {
        let dir = root.join(dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            fs::write(dir.join(name), body).unwrap();
        }
    }

    #[test]
    fn finds_and_merges_a_split_album_and_can_undo_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        album(
            root,
            "Fred again._/USB (2025)",
            &[("01 - Kyle.flac", "k"), ("02 - scared.flac", "s-small"), ("03 - Jungle.flac", "j"), ("cover.jpg", "c")],
        );
        album(
            root,
            "Fred again/USB (2026)",
            &[("02 - scared.flac", "s-bigger"), ("05 - ICEY.flac", "i"), ("05 - ICEY.lrc", "l")],
        );
        album(root, "Fred again._/USB002 REMIXES", &[("01 - solo.flac", "x")]);
        album(root, ".delune-trash/1-0/Old/Album", &[("01 - x.flac", "x")]);

        let scan = scan(root);
        assert_eq!((scan.albums, scan.tracks), (3, 6));
        assert_eq!(scan.findings.len(), 1, "{:?}", scan.findings);
        let finding = &scan.findings[0];
        assert_eq!(finding.kind, Kind::SplitAlbum);
        assert_eq!(finding.folders, ["Fred again._/USB (2025)", "Fred again/USB (2026)"]);
        assert_eq!(finding.files, 2);

        let fixed = fix(root, finding).unwrap();
        // "scared": the two copies can't be read as audio here, so the bigger one wins.
        assert_eq!((fixed.moved, fixed.trashed), (2, 1));
        let kept = root.join("Fred again._/USB (2025)");
        assert_eq!(fs::read_to_string(kept.join("02 - scared.flac")).unwrap(), "s-bigger");
        assert!(kept.join("05 - ICEY.flac").exists() && kept.join("05 - ICEY.lrc").exists());
        assert!(!root.join("Fred again").exists(), "emptied folders go");
        assert!(scan_of(root).findings.is_empty());

        // The same finding can't be applied twice.
        assert!(matches!(fix(root, finding), Err(FixError::Changed)));

        assert!(trash::restore(root, &fixed.batch).unwrap().is_empty());
        assert_eq!(fs::read_to_string(root.join("Fred again/USB (2026)/02 - scared.flac")).unwrap(), "s-bigger");
        assert_eq!(fs::read_to_string(kept.join("02 - scared.flac")).unwrap(), "s-small");
        assert!(root.join("Fred again/USB (2026)/05 - ICEY.lrc").exists());
        assert!(!kept.join("05 - ICEY.flac").exists());
    }

    fn scan_of(root: &Path) -> Scan {
        scan(root)
    }

    #[test]
    fn keeps_one_copy_of_a_doubled_track() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        album(root, "A/B", &[("01 - Song.mp3", "small"), ("01 - Song.flac", "much bigger"), ("02 - Other.flac", "o")]);
        let scan = scan(root);
        assert_eq!(scan.findings.len(), 1);
        let finding = &scan.findings[0];
        assert_eq!(finding.kind, Kind::DuplicateTracks);
        assert_eq!(finding.duplicates, [["A/B/01 - Song.flac", "A/B/01 - Song.mp3"]]);

        // Something changed since the scan: refused.
        fs::write(root.join("A/B/03 - New.flac"), "n").unwrap();
        assert!(matches!(fix(root, finding), Err(FixError::Changed)));
        let finding = scan_of(root).findings.remove(0);
        let fixed = fix(root, &finding).unwrap();
        assert_eq!(fixed.trashed, 1);
        assert!(root.join("A/B/01 - Song.flac").exists() && !root.join("A/B/01 - Song.mp3").exists());
    }

    #[test]
    fn refuses_findings_that_point_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let finding = Finding {
            id: String::new(),
            key: String::new(),
            kind: Kind::DuplicateTracks,
            folders: vec!["../elsewhere".into()],
            duplicates: Vec::new(),
            files: 1,
        };
        assert!(matches!(fix(tmp.path(), &finding), Err(FixError::Changed)));
    }
}
