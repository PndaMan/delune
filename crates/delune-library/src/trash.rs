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
        for entry in fs::read_dir(parent)?.flatten() {
            let p = entry.path();
            if p != source && p.is_file() && p.file_stem() == Some(stem.as_os_str()) {
                companions.push(p);
            }
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
            } else if let Ok(inside) = p.strip_prefix(&base) {
                out.push(inside.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// Put a batch back where it came from. Files whose place is taken again stay in the
/// trash; their paths are returned.
///
/// # Errors
///
/// When the batch name isn't a plain name.
pub fn restore(root: &Path, batch: &str) -> io::Result<Vec<String>> {
    safe(batch)?;
    let base = root.join(DIR).join(batch);
    let mut left = Vec::new();
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
