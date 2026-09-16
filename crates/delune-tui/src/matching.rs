//! Telling whether a shared folder is something you already have or are already getting.
//!
//! The same rules as the web app (`web/src/lib/library.ts`, `track-name.ts`,
//! `downloads.ts`), so both clients mark the same releases.

use delune_core::api::{Candidate, DownloadJob, JobStatus, LibraryMatch, LibraryState};

/// Folder names that say where music is kept, not who made it.
const GENERIC_FOLDERS: &[&str] = &[
    "music",
    "musik",
    "musique",
    "mp3",
    "mp3s",
    "flac",
    "flacs",
    "lossless",
    "album",
    "albums",
    "download",
    "downloads",
    "complete",
    "share",
    "shared",
    "soulseek",
    "slsk",
    "new",
    "misc",
    "various",
    "va",
    "library",
    "media",
    "audio",
    "collection",
];

/// The artist a shared folder's parent names, unless it's a storage folder like "Music".
#[must_use]
pub fn artist_from_folder(parent: Option<&str>) -> Option<String> {
    let name = parent?.trim();
    let lower = name.to_lowercase();
    if name.is_empty() || name.starts_with("@@") || GENERIC_FOLDERS.contains(&lower.as_str()) {
        return None;
    }
    Some(name.to_owned())
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        other => other,
    }
}

fn without_brackets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// A comparison key for titles: case, accents, punctuation and bracketed extras don't count.
#[must_use]
pub fn title_key(title: &str) -> String {
    let lower: String = without_brackets(&title.to_lowercase()).chars().map(fold_accent).collect();
    lower.replace('&', "and").replace("colour", "color").chars().filter(char::is_ascii_alphanumeric).collect()
}

/// Whether two title keys name the same song, allowing for extra words on one of them.
#[must_use]
pub fn same_title(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (b.len() >= 4 && a.contains(b)) || (a.len() >= 4 && b.contains(a))
}

fn is_audio_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "flac"
            | "alac"
            | "wav"
            | "aif"
            | "aiff"
            | "mp3"
            | "m4a"
            | "aac"
            | "opus"
            | "ogg"
            | "oga"
            | "wv"
            | "ape"
            | "dsf"
            | "dff"
    )
}

/// Whether a file name is a song.
#[must_use]
pub fn is_audio(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| is_audio_extension(ext))
}

/// The song title in a shared file name: "01. Speak to Me.flac" is "Speak to Me".
#[must_use]
pub fn track_title(file_name: &str) -> String {
    let Some((stem, ext)) = file_name.rsplit_once('.') else { return file_name.to_owned() };
    if !is_audio_extension(ext) {
        return file_name.to_owned();
    }
    let stem = stem.replace('_', " ");
    let stem = stem.trim();
    let parts: Vec<&str> = stem.split(" - ").collect();
    let is_number = |p: &str| {
        let p = p.trim();
        !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit())
    };
    if let Some(i) = parts.iter().position(|p| is_number(p))
        && i + 1 < parts.len()
    {
        return parts[i + 1..].join(" - ");
    }
    // A leading position: "03 ", "2-04 ", "A1. ".
    let bytes = stem.as_bytes();
    let mut end = 0;
    if bytes.first().is_some_and(|b| (b'A'..=b'H').contains(b)) && bytes.get(1).is_some_and(u8::is_ascii_digit) {
        end = 1;
    }
    while end < bytes.len() && (bytes[end].is_ascii_digit() || (end > 0 && bytes[end] == b'-' && end < 3)) {
        end += 1;
    }
    if end > 0 && end <= 4 && end < bytes.len() && matches!(bytes[end], b' ' | b'.' | b'-' | b')') {
        let rest = stem[end..].trim_start_matches([' ', '.', '-', ')']);
        if !rest.is_empty() {
            return rest.to_owned();
        }
    }
    parts.last().map_or_else(|| stem.to_owned(), |p| (*p).to_owned())
}

/// How much of a shared folder the library already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ownership {
    pub owned: usize,
    pub total: usize,
}

impl Ownership {
    #[must_use]
    pub const fn complete(self) -> bool {
        self.total > 0 && self.owned == self.total
    }
}

/// Compare a folder with the library's copy, track by track. `None` when the album
/// isn't in the library at all.
#[must_use]
pub fn ownership(candidate: &Candidate, library: &LibraryMatch) -> Option<Ownership> {
    if library.state != LibraryState::InLibrary {
        return None;
    }
    let titles: Vec<String> = library.tracks.iter().map(|t| title_key(&t.title)).filter(|t| !t.is_empty()).collect();
    let mut result = Ownership { owned: 0, total: 0 };
    for file in candidate.files.iter().filter(|f| f.audio) {
        result.total += 1;
        if owns(&titles, &file.name) {
            result.owned += 1;
        }
    }
    Some(result)
}

/// Whether the library titles (already keyed) include this file's song.
#[must_use]
pub fn owns(library_titles: &[String], file_name: &str) -> bool {
    let key = title_key(&track_title(file_name));
    library_titles.iter().any(|t| same_title(&key, t))
}

/// This exact folder's download, unless it was stopped.
#[must_use]
pub fn job_for_candidate<'a>(jobs: &'a [DownloadJob], candidate: &Candidate) -> Option<&'a DownloadJob> {
    jobs.iter().find(|job| {
        job.username == candidate.username && job.folder == candidate.folder && job.status != JobStatus::Cancelled
    })
}

fn loose(text: &str) -> String {
    title_key(text)
}

/// A download of the same album from anyone, matched loosely by title.
#[must_use]
pub fn job_for_title<'a>(jobs: &'a [DownloadJob], title: &str) -> Option<&'a DownloadJob> {
    let wanted = loose(title);
    if wanted.len() < 3 {
        return None;
    }
    jobs.iter().find(|job| job.status != JobStatus::Cancelled && loose(&job.title).contains(&wanted))
}

/// Whether a job is still on its way.
#[must_use]
pub const fn in_flight(job: &DownloadJob) -> bool {
    matches!(job.status, JobStatus::Queued | JobStatus::Downloading)
}

/// How far a job has got, from 0 to 1.
#[must_use]
pub fn progress(job: &DownloadJob) -> f64 {
    if job.total_bytes == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // display only
    let ratio = job.bytes as f64 / job.total_bytes as f64;
    ratio.clamp(0.0, 1.0)
}

/// What a release is to you: already yours, on its way, or waiting for a look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    InLibrary,
    /// Some of its songs are in the library.
    Partial(Ownership),
    Downloading(f64),
    Queued,
    /// Another copy of the same album is downloading.
    OtherCopy,
    Review,
    Imported,
    Failed,
}

impl Mark {
    /// Whether the release (or this copy of it) is already handled.
    #[must_use]
    pub const fn is_yours(self) -> bool {
        matches!(self, Self::InLibrary | Self::Imported)
    }
}

/// The mark for a search result.
#[must_use]
pub fn mark(candidate: &Candidate, jobs: &[DownloadJob], library: Option<&LibraryMatch>) -> Option<Mark> {
    if let Some(job) = job_for_candidate(jobs, candidate) {
        return Some(match job.status {
            JobStatus::Queued => Mark::Queued,
            JobStatus::Downloading => Mark::Downloading(progress(job)),
            JobStatus::Ready => Mark::Review,
            JobStatus::Imported => Mark::Imported,
            JobStatus::Failed | JobStatus::Cancelled => Mark::Failed,
        });
    }
    if let Some(owned) = library.and_then(|l| ownership(candidate, l)) {
        if owned.complete() {
            return Some(Mark::InLibrary);
        }
        if owned.owned > 0 {
            return Some(Mark::Partial(owned));
        }
        // The album is there, but none of this folder's names matched: still the album.
        if owned.total == 0 {
            return Some(Mark::InLibrary);
        }
    }
    if job_for_title(jobs, &candidate.title).is_some_and(in_flight) {
        return Some(Mark::OtherCopy);
    }
    None
}

/// The mark for an album known by title (artist and album pages).
#[must_use]
pub fn album_mark(title: &str, in_library: bool, jobs: &[DownloadJob]) -> Option<Mark> {
    if let Some(job) = job_for_title(jobs, title) {
        match job.status {
            JobStatus::Queued => return Some(Mark::Queued),
            JobStatus::Downloading => return Some(Mark::Downloading(progress(job))),
            JobStatus::Ready => return Some(Mark::Review),
            JobStatus::Imported if !in_library => return Some(Mark::Imported),
            _ => {}
        }
    }
    in_library.then_some(Mark::InLibrary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use delune_core::api::{CandidateFile, LibraryTrack};

    #[test]
    fn finds_titles_in_file_names() {
        assert_eq!(track_title("Pink Floyd - The Dark Side of the Moon - 03 - Time.flac"), "Time");
        assert_eq!(track_title("01. Speak to Me.flac"), "Speak to Me");
        assert_eq!(track_title("2-04 Lift.flac"), "Lift");
        assert_eq!(track_title("A1. Sixtyniner.flac"), "Sixtyniner");
        assert_eq!(track_title("01_Roygbiv.mp3"), "Roygbiv");
        assert_eq!(track_title("cover.jpg"), "cover.jpg");
        assert_eq!(track_title("Aquarius.flac"), "Aquarius");
    }

    #[test]
    fn titles_compare_loosely() {
        assert_eq!(title_key("Café del Mar (Remastered)"), "cafedelmar");
        assert!(same_title(&title_key("Time - 2011 Remaster"), &title_key("Time")));
        assert!(!same_title("", "x"));
        assert_eq!(artist_from_folder(Some("FLAC")), None);
        assert_eq!(artist_from_folder(Some("@@abcde")), None);
        assert_eq!(artist_from_folder(Some("Boards of Canada")).as_deref(), Some("Boards of Canada"));
    }

    fn file(name: &str) -> CandidateFile {
        CandidateFile {
            path: name.into(),
            name: name.into(),
            size: 1,
            audio: is_audio(name),
            quality: None,
            quality_label: None,
            duration_secs: None,
        }
    }

    #[test]
    fn counts_what_the_library_has() {
        let candidate = Candidate {
            id: "c".into(),
            username: "peer".into(),
            folder: "f".into(),
            title: "Twoism".into(),
            parent: None,
            files: vec![file("01 Sixtyniner.flac"), file("02 Oirectine.flac"), file("cover.jpg")],
            audio_files: 2,
            total_bytes: 2,
            duration_secs: None,
            quality: None,
            quality_label: None,
            quality_rank: 0,
            mixed_quality: false,
            has_cover: true,
            free_slot: true,
            avg_speed: 0,
            queue_length: 0,
            peer: None,
        };
        let library = LibraryMatch {
            state: LibraryState::InLibrary,
            album: Some("Twoism".into()),
            artist: None,
            year: None,
            tracks: vec![LibraryTrack { title: "Sixtyniner".into(), track: Some(1), disc: None }],
            quality_label: None,
        };
        assert_eq!(ownership(&candidate, &library), Some(Ownership { owned: 1, total: 2 }));
        assert!(matches!(mark(&candidate, &[], Some(&library)), Some(Mark::Partial(_))));
        assert_eq!(mark(&candidate, &[], None), None);
    }
}
