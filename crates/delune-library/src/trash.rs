//! Where files delune takes out of the library go instead of being deleted.
//!
//! `<library>/.delune-trash/<batch>/<path in the library>`: on the library's own
//! filesystem (so a move is instant and needs no space), hidden, and ignored by
//! Navidrome through an `.ndignore` file. A batch can be put back as it was.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DIR: &str = ".delune-trash";

/// In a batch: the files a fix moved within the library, one `from\tto` per line, so
/// undoing the batch can move them back.
const MOVES: &str = ".delune-moves";

/// A name for a new batch: sorts by time, unique within the process.
#[must_use]
pub fn batch_name() -> String {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{secs}-{n}")
}

fn safe(relative: &str) -> io::Result<&Path> {
    let path = Path::new(relative);
    if relative.is_empty()
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
        || path.components().next().is_some_and(|c| c.as_os_str() == DIR)
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("not a library path: {relative}")));
    }
    Ok(path)
}

fn prepare(root: &Path) -> io::Result<PathBuf> {
    let trash = root.join(DIR);
    fs::create_dir_all(&trash)?;
    let ignore = trash.join(".ndignore");
    if !ignore.exists() {
        // Navidrome skips a folder holding this file (and everything under it).
        fs::write(ignore, "*\n")?;
    }
    Ok(trash)
}

/// Move `relative` (a file in the library) into trash batch `batch`, with any files
/// beside it that share its name (lyrics, cue sheets). Returns what moved, as
/// library-relative paths.
///
/// # Errors
///
/// When the path isn't a plain file inside the library, or a move fails (anything
/// already moved stays in the trash batch and is reported by [`contents`]).
pub fn put(root: &Path, relative: &str, batch: &str) -> io::Result<Vec<String>> {
    let path = safe(relative)?;
    let source = root.join(path);
    let meta = fs::symlink_metadata(&source)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("not a file: {relative}")));
    }
    let target_dir = prepare(root)?.join(batch);
    let mut moved = Vec::new();
    let stem = source.file_stem().map(std::ffi::OsStr::to_os_string);
    let mut companions = Vec::new();
    if let (Some(parent), Some(stem)) = (source.parent(), stem) {
        let mut others_remain = false;
        for entry in fs::read_dir(parent)?.flatten() {
            let p = entry.path();
            if p == source || !p.is_file() || p.file_stem() != Some(stem.as_os_str()) {
                continue;
            }
            // Another copy of the track in another format isn't a companion, and it
            // keeps the lyrics they share.
            if crate::health::is_audio(&p) {
                others_remain = true;
            } else {
                companions.push(p);
            }
        }
        if others_remain {
            companions.clear();
        }
    }
    for file in std::iter::once(source.clone()).chain(companions) {
        let Ok(inside) = file.strip_prefix(root) else { continue };
        let destination = target_dir.join(inside);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&file, &destination)?;
        moved.push(inside.to_string_lossy().replace('\\', "/"));
    }
    Ok(moved)
}

/// Note that `from` was moved to `to` (both library-relative) as part of `batch`.
///
/// # Errors
///
/// When the note can't be written.
pub fn record_move(root: &Path, batch: &str, from: &str, to: &str) -> io::Result<()> {
    use std::io::Write as _;
    safe(from)?;
    safe(to)?;
    let dir = prepare(root)?.join(batch);
    fs::create_dir_all(&dir)?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(dir.join(MOVES))?;
    writeln!(file, "{from}\t{to}")
}

fn moves(root: &Path, batch: &str) -> Vec<(String, String)> {
    let text = fs::read_to_string(root.join(DIR).join(batch).join(MOVES)).unwrap_or_default();
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(from, to)| safe(from).is_ok() && safe(to).is_ok())
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

/// A batch's own notes (moves, tag changes) live beside its files as hidden files.
fn is_record(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(".delune-"))
}

/// Library-relative paths of everything in a batch.
#[must_use]
pub fn contents(root: &Path, batch: &str) -> Vec<String> {
    let base = root.join(DIR).join(batch);
    let mut out = Vec::new();
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if !(p.parent() == Some(base.as_path()) && is_record(&p))
                && let Ok(inside) = p.strip_prefix(&base)
            {
                out.push(inside.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// Put a batch back where it came from, and undo the moves it recorded. Files whose
/// place is taken again stay where they are; their paths are returned.
///
/// # Errors
///
/// When the batch name isn't a plain name.
pub fn restore(root: &Path, batch: &str) -> io::Result<Vec<String>> {
    safe(batch)?;
    let base = root.join(DIR).join(batch);
    let mut left = Vec::new();
    // Files moved within the library go back where they were, newest move first.
    for (from, to) in moves(root, batch).into_iter().rev() {
        let (was, now) = (root.join(&from), root.join(&to));
        if !now.exists() || was.exists() {
            left.push(to);
            continue;
        }
        if let Some(parent) = was.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(&now, &was).is_err() {
            left.push(to);
        } else if let Some(parent) = now.parent() {
            // A folder the fix filled and that's now empty again.
            let _ = fs::remove_dir(parent);
        }
    }
    for relative in contents(root, batch) {
        let from = base.join(&relative);
        let to = root.join(&relative);
        if to.exists() {
            left.push(relative);
            continue;
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(&from, &to).is_err() {
            left.push(relative);
        }
    }
    if left.is_empty() {
        let _ = fs::remove_dir_all(&base);
    }
    Ok(left)
}

/// What a batch did, in a sentence.
const ABOUT: &str = ".delune-about";

/// Say what batch `batch` is for, so the trash can show it.
pub fn describe(root: &Path, batch: &str, label: &str) {
    let Ok(dir) = prepare(root).map(|t| t.join(batch)) else { return };
    let _ = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join(ABOUT), label));
}

/// One thing a batch changed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Change {
    /// `removed` (in the trash), `moved` or `retagged`.
    pub kind: String,
    /// Library-relative.
    pub path: String,
    /// Where a moved file went.
    pub to: Option<String>,
}

/// A batch in the trash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Batch {
    pub id: String,
    /// Unix seconds.
    pub created_at: u64,
    pub label: Option<String>,
    pub changes: Vec<Change>,
}

fn changes(root: &Path, batch: &str) -> Vec<Change> {
    let base = root.join(DIR).join(batch);
    let mut out: Vec<Change> =
        contents(root, batch).into_iter().map(|path| Change { kind: "removed".into(), path, to: None }).collect();
    out.extend(moves(root, batch).into_iter().map(|(from, to)| Change {
        kind: "moved".into(),
        path: from,
        to: Some(to),
    }));
    let tags = fs::read_to_string(base.join(".delune-tags")).unwrap_or_default();
    let mut retagged: Vec<String> = tags
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("file").and_then(|f| f.as_str()).map(str::to_owned))
        .collect();
    retagged.dedup();
    out.extend(retagged.into_iter().map(|path| Change { kind: "retagged".into(), path, to: None }));
    out
}

/// The batches in the trash that changed anything, newest first.
#[must_use]
pub fn batches(root: &Path) -> Vec<Batch> {
    let Ok(entries) = fs::read_dir(root.join(DIR)) else { return Vec::new() };
    let mut out: Vec<Batch> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            let created_at = id.split('-').next()?.parse::<u64>().ok()?;
            let changes = changes(root, &id);
            if changes.is_empty() {
                return None;
            }
            let label = fs::read_to_string(e.path().join(ABOUT)).ok().map(|l| l.trim().to_owned());
            Some(Batch { id, created_at, label, changes })
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
    out
}

/// Remove batches older than `age`. Only ever touches the trash folder.
pub fn purge(root: &Path, age: Duration) {
    let Ok(entries) = fs::read_dir(root.join(DIR)) else { return };
    let cutoff = SystemTime::now().checked_sub(age).and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    let Some(cutoff) = cutoff else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(secs) = name.split('-').next().and_then(|s| s.parse::<u64>().ok()) else { continue };
        if secs < cutoff.as_secs() && entry.path().is_dir() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_files_and_their_companions_away_and_back() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Artist/Album");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("01 - Song.mp3"), "old").unwrap();
        fs::write(album.join("01 - Song.lrc"), "lyrics").unwrap();
        fs::write(album.join("02 - Other.flac"), "keep").unwrap();

        let batch = batch_name();
        let mut moved = put(root.path(), "Artist/Album/01 - Song.mp3", &batch).unwrap();
        moved.sort();
        assert_eq!(moved, ["Artist/Album/01 - Song.lrc", "Artist/Album/01 - Song.mp3"]);
        assert!(!album.join("01 - Song.mp3").exists() && album.join("02 - Other.flac").exists());
        assert!(root.path().join(DIR).join(".ndignore").exists());
        assert_eq!(contents(root.path(), &batch).len(), 2);

        assert!(put(root.path(), "../outside.mp3", &batch).is_err());
        assert!(put(root.path(), ".delune-trash/x", &batch).is_err());
        assert!(put(root.path(), "Artist/Album", &batch).is_err(), "folders aren't trashed");

        assert!(restore(root.path(), &batch).unwrap().is_empty());
        assert_eq!(fs::read_to_string(album.join("01 - Song.mp3")).unwrap(), "old");
        assert!(!root.path().join(DIR).join(&batch).exists());
    }

    #[test]
    fn purging_only_removes_old_batches() {
        let root = tempfile::tempdir().unwrap();
        let trash = root.path().join(DIR);
        fs::create_dir_all(trash.join("100-0")).unwrap();
        fs::create_dir_all(trash.join(batch_name())).unwrap();
        purge(root.path(), Duration::from_secs(3600));
        let left: Vec<_> = fs::read_dir(&trash).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 1);
    }
}
