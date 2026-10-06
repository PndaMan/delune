//! What we share: the files, where they really are, and how to find them quickly.
//!
//! Other people see a *virtual* path for every file (`Music\Artist\Album\01.flac`),
//! never where it lives on disk. [`ShareIndex`] maps one to the other, so uploads can
//! only ever read files that were deliberately indexed, and answers searches with a
//! word index: the network sends many searches a second, far too many to scan a
//! library for each one.
//!
//! Matching follows what Soulseek clients do: every query word must appear as a word
//! in the file's path, words starting with `-` exclude files, and a leading `*`
//! matches the end of a word ("*phones" finds "headphones").

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::peer::SharedFile;
use crate::shares::{SharedDirectory, SharedFileList};

/// Most files one search answer carries.
pub const MAX_RESULTS: usize = 500;
/// Queries shorter than this (after punctuation) aren't answered; they match everything.
const MIN_WORD_LEN: usize = 2;

/// One shared file and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub file: SharedFile,
    pub disk_path: PathBuf,
}

/// The files we share, ready to browse, search and upload from.
#[derive(Debug, Default)]
pub struct ShareIndex {
    list: Arc<SharedFileList>,
    files: Vec<IndexedFile>,
    by_path: HashMap<String, usize>,
    words: HashMap<String, Vec<u32>>,
}

fn words(path: &str) -> impl Iterator<Item = String> + '_ {
    path.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase)
}

impl ShareIndex {
    /// Build an index. `files` carry full virtual paths.
    #[must_use]
    pub fn new(mut files: Vec<IndexedFile>) -> Self {
        files.sort_by(|a, b| a.file.path.cmp(&b.file.path));
        let mut by_path = HashMap::with_capacity(files.len());
        let mut index: HashMap<String, Vec<u32>> = HashMap::new();
        let mut directories: Vec<SharedDirectory> = Vec::new();

        for (i, entry) in files.iter().enumerate() {
            let id = u32::try_from(i).unwrap_or(u32::MAX);
            by_path.insert(entry.file.path.clone(), i);
            let unique: HashSet<String> = words(&entry.file.path).collect();
            for word in unique {
                index.entry(word).or_default().push(id);
            }
            let folder = entry.file.folder();
            match directories.last_mut() {
                Some(last) if last.path == folder => last.files.push(entry.file.clone()),
                _ => directories.push(SharedDirectory { path: folder.to_owned(), files: vec![entry.file.clone()] }),
            }
        }
        let list = Arc::new(SharedFileList { directories, private_directories: vec![] });
        Self { list, files, by_path, words: index }
    }

    /// Everything, as sent to people browsing us.
    #[must_use]
    pub fn list(&self) -> Arc<SharedFileList> {
        self.list.clone()
    }

    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn folder_count(&self) -> usize {
        self.list.directories.len()
    }

    /// The indexed file behind a virtual path, if we share it.
    #[must_use]
    pub fn lookup(&self, virtual_path: &str) -> Option<&IndexedFile> {
        self.by_path.get(virtual_path).map(|&i| &self.files[i])
    }

    /// Files matching a Soulseek query, best effort, at most [`MAX_RESULTS`].
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<SharedFile> {
        let mut include: Vec<String> = Vec::new();
        let mut suffixes: Vec<String> = Vec::new();
        let mut exclude: Vec<String> = Vec::new();
        for raw in query.split_whitespace() {
            let (negated, term) = raw.strip_prefix('-').map_or((false, raw), |t| (true, t));
            let (wildcard, term) = term.strip_prefix('*').map_or((false, term), |t| (true, t));
            for word in words(term) {
                match (negated, wildcard) {
                    (true, _) => exclude.push(word),
                    (false, true) => suffixes.push(word),
                    (false, false) => include.push(word),
                }
            }
        }
        if include.iter().chain(&suffixes).all(|w| w.chars().count() < MIN_WORD_LEN) {
            return Vec::new();
        }

        // Start from the rarest exact word, then check the rest against each candidate.
        let mut candidates: Option<Vec<u32>> = None;
        let mut postings: Vec<&Vec<u32>> = Vec::with_capacity(include.len());
        for word in &include {
            match self.words.get(word) {
                Some(list) => postings.push(list),
                None => return Vec::new(),
            }
        }
        postings.sort_by_key(|p| p.len());
        if let Some((first, rest)) = postings.split_first() {
            let mut ids: Vec<u32> = (*first).clone();
            for other in rest {
                let set: HashSet<u32> = other.iter().copied().collect();
                ids.retain(|id| set.contains(id));
            }
            candidates = Some(ids);
        }

        let matches = |entry: &IndexedFile| {
            let path_words: Vec<String> = words(&entry.file.path).collect();
            suffixes.iter().all(|s| path_words.iter().any(|w| w.ends_with(s.as_str())))
                && !exclude.iter().any(|e| path_words.contains(e))
        };
        let ids: Box<dyn Iterator<Item = usize>> = match candidates {
            Some(ids) => Box::new(ids.into_iter().map(|id| id as usize)),
            None => Box::new(0..self.files.len()),
        };
        ids.map(|i| &self.files[i]).filter(|e| matches(e)).take(MAX_RESULTS).map(|e| e.file.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> IndexedFile {
        IndexedFile {
            file: SharedFile {
                path: path.into(),
                size: 1,
                extension: "flac".into(),
                bitrate_kbps: None,
                duration_secs: None,
                vbr: false,
                sample_rate: None,
                bit_depth: None,
            },
            disk_path: PathBuf::from("/music").join(path.replace('\\', "/")),
        }
    }

    fn index() -> ShareIndex {
        ShareIndex::new(vec![
            entry(r"Music\Boards of Canada\Twoism\01 Sixtyniner.flac"),
            entry(r"Music\Boards of Canada\Twoism\02 Oirectine.flac"),
            entry(r"Music\Boards of Canada\Geogaddi\03 Music Is Math.flac"),
            entry(r"Music\Radiohead\OK Computer\02 Paranoid Android.flac"),
            entry(r"Music\Various\Headphones Only\01 Intro.flac"),
        ])
    }

    fn found(index: &ShareIndex, query: &str) -> Vec<String> {
        index.search(query).into_iter().map(|f| f.file_name().to_owned()).collect()
    }

    #[test]
    fn groups_files_into_folders() {
        let index = index();
        assert_eq!(index.file_count(), 5);
        assert_eq!(index.folder_count(), 4);
        assert!(index.lookup(r"Music\Radiohead\OK Computer\02 Paranoid Android.flac").is_some());
        assert!(index.lookup(r"Music\Radiohead\..\..\etc\passwd").is_none());
    }

    #[test]
    fn every_word_must_match() {
        let index = index();
        assert_eq!(found(&index, "boards twoism"), ["01 Sixtyniner.flac", "02 Oirectine.flac"]);
        assert_eq!(found(&index, "BOARDS of canada math"), ["03 Music Is Math.flac"]);
        assert!(found(&index, "boards radiohead").is_empty());
        assert!(found(&index, "nothing").is_empty());
    }

    #[test]
    fn exclusions_and_wildcards() {
        let index = index();
        assert_eq!(found(&index, "boards -twoism"), ["03 Music Is Math.flac"]);
        assert_eq!(found(&index, "*phones"), ["01 Intro.flac"]);
    }

    #[test]
    fn ignores_queries_that_match_everything() {
        let index = index();
        assert!(found(&index, "a").is_empty());
        assert!(found(&index, "-boards").is_empty());
    }
}
