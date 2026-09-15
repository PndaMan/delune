//! Sharing the library on Soulseek, and the uploads that follow.
//!
//! Off until an admin turns it on. When on, delune walks the library folder, reads
//! each audio file's properties (cached by size and modification time, so rescans
//! only read what changed), and hands the Soulseek client a [`ShareIndex`] under a
//! virtual top folder like `Music\`. Other people never see real paths, and only
//! indexed files can be uploaded. Symlinks aren't followed and hidden files are
//! skipped, so nothing outside the library is reachable.
//!
//! The library is rescanned after every import and every six hours.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::Codec;
use delune_core::api::{ApiError, SharingSettings, SharingStatus, Upload, UploadStatus};
use delune_soulseek::peer::SharedFile;
use delune_soulseek::{IndexedFile, ShareIndex, UploadLimits, UploadState};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::accounts::CurrentUser;

const RESCAN_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Non-audio files worth sharing alongside the music.
const EXTRAS: &[&str] = &["jpg", "jpeg", "png", "webp", "cue", "log", "m3u", "m3u8", "txt", "nfo", "pdf"];

#[derive(Debug, Default)]
pub struct Sharing {
    settings_path: Option<PathBuf>,
    cache_path: Option<PathBuf>,
    state: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    settings: SharingSettings,
    scanning: bool,
    rescan_again: bool,
    files: u32,
    folders: u32,
    last_scan: Option<u64>,
    error: Option<String>,
}

/// Audio properties remembered between scans.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    size: u64,
    modified: u64,
    bitrate_kbps: Option<u32>,
    duration_secs: Option<u32>,
    sample_rate: Option<u32>,
    bit_depth: Option<u32>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Sharing {
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        let settings_path = data_dir.join("sharing.json");
        let settings = std::fs::read(&settings_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            settings_path: Some(settings_path),
            cache_path: Some(data_dir.join("share-cache.json")),
            state: Mutex::new(Inner { settings, ..Inner::default() }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[must_use]
    pub fn settings(&self) -> SharingSettings {
        self.lock().settings.clone()
    }

    fn status(&self, library_dir: Option<&Path>) -> SharingStatus {
        let inner = self.lock();
        SharingStatus {
            settings: inner.settings.clone(),
            library_dir: library_dir.map(|p| p.display().to_string()),
            scanning: inner.scanning,
            files: inner.files,
            folders: inner.folders,
            last_scan: inner.last_scan,
            error: inner.error.clone(),
        }
    }

    fn save_settings(&self, settings: &SharingSettings) {
        let Some(path) = &self.settings_path else { return };
        if let Ok(json) = serde_json::to_vec_pretty(settings)
            && let Err(error) = std::fs::write(path, json)
        {
            tracing::warn!(%error, "couldn't save sharing settings");
        }
    }
}

fn limits(settings: &SharingSettings) -> UploadLimits {
    UploadLimits {
        slots: usize::try_from(settings.slots.clamp(1, 20)).unwrap_or(3),
        queue_per_user: usize::try_from(settings.queue_per_user.clamp(1, 10_000)).unwrap_or(200),
        bytes_per_second: settings.speed_limit_kib.filter(|&k| k > 0).map(|k| u64::from(k) * 1024),
        refuse_leechers: settings.refuse_leechers,
    }
}

/// Apply settings to the Soulseek client and (re)index if sharing is on. Call at
/// startup, after settings change, and after imports.
pub fn refresh(app: &AppState) {
    let Some(client) = app.soulseek.clone() else { return };
    let settings = app.sharing.settings();
    client.set_upload_limits(limits(&settings));
    client.set_download_limit(settings.download_limit_kib.filter(|&k| k > 0).map(|k| u64::from(k) * 1024));
    client.set_banned(settings.banned.iter().cloned().collect::<HashSet<_>>());

    let library = app.library.library_dir.clone();
    let (Some(library), true) = (library, settings.enabled) else {
        client.set_share_index(ShareIndex::default());
        let mut inner = app.sharing.lock();
        inner.files = 0;
        inner.folders = 0;
        return;
    };

    {
        let mut inner = app.sharing.lock();
        if inner.scanning {
            inner.rescan_again = true;
            return;
        }
        inner.scanning = true;
    }
    let sharing = app.sharing.clone();
    tokio::spawn(async move {
        loop {
            let cache_path = sharing.cache_path.clone();
            let (share_name, root) = (settings.share_name.clone(), library.clone());
            let scanned = tokio::task::spawn_blocking(move || scan(&root, &share_name, cache_path.as_deref())).await;
            let mut inner = sharing.lock();
            match scanned {
                Ok(Ok(files)) => {
                    let index = ShareIndex::new(files);
                    inner.files = u32::try_from(index.file_count()).unwrap_or(u32::MAX);
                    inner.folders = u32::try_from(index.folder_count()).unwrap_or(u32::MAX);
                    inner.error = None;
                    inner.last_scan = Some(now());
                    tracing::info!(files = inner.files, folders = inner.folders, "library indexed for sharing");
                    client.set_share_index(index);
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "couldn't index the library for sharing");
                    inner.error = Some(error.to_string());
                }
                Err(_) => inner.error = Some("indexing stopped unexpectedly".into()),
            }
            if inner.rescan_again {
                inner.rescan_again = false;
                continue;
            }
            inner.scanning = false;
            break;
        }
    });
}

/// Rescan periodically while sharing is on. Call once at startup.
pub fn start(app: &AppState) {
    refresh(app);
    let app = app.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval_at(tokio::time::Instant::now() + RESCAN_EVERY, RESCAN_EVERY);
        loop {
            every.tick().await;
            if app.sharing.settings().enabled {
                refresh(&app);
            }
        }
    });
}

/// Walk the library and describe every shareable file.
fn scan(root: &Path, share_name: &str, cache_path: Option<&Path>) -> std::io::Result<Vec<IndexedFile>> {
    let old: HashMap<String, Cached> = cache_path
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut cache: HashMap<String, Cached> = HashMap::with_capacity(old.len());
    let mut files = Vec::new();
    let top = clean_segment(share_name).unwrap_or_else(|| "Music".into());

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if dir == root => return Err(error),
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            // `file_type` doesn't follow symlinks, so links are skipped rather than followed.
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
            let audio = Codec::from_extension(&extension).is_some();
            if !audio && !EXTRAS.contains(&extension.as_str()) {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else { continue };
            let segments: Option<Vec<String>> =
                relative.components().map(|c| clean_segment(&c.as_os_str().to_string_lossy())).collect();
            let Some(segments) = segments else { continue };
            let virtual_path = format!("{top}\\{}", segments.join("\\"));

            let Ok(meta) = entry.metadata() else { continue };
            let modified =
                meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            let key = relative.to_string_lossy().into_owned();
            let cached = match old.get(&key) {
                Some(c) if c.size == meta.len() && c.modified == modified => c.clone(),
                _ => {
                    let mut c = Cached {
                        size: meta.len(),
                        modified,
                        bitrate_kbps: None,
                        duration_secs: None,
                        sample_rate: None,
                        bit_depth: None,
                    };
                    if audio && let Ok((quality, duration)) = delune_library::inspect::properties(&path) {
                        c.bitrate_kbps = quality.bitrate_kbps;
                        c.duration_secs = Some(duration);
                        c.sample_rate = quality.sample_rate;
                        c.bit_depth = quality.bit_depth.map(u32::from);
                    }
                    c
                }
            };
            files.push(IndexedFile {
                file: SharedFile {
                    path: virtual_path,
                    size: cached.size,
                    extension,
                    bitrate_kbps: cached.bitrate_kbps,
                    duration_secs: cached.duration_secs,
                    vbr: false,
                    sample_rate: cached.sample_rate,
                    bit_depth: cached.bit_depth,
                },
                disk_path: path,
            });
            cache.insert(key, cached);
        }
    }

    if let Some(path) = cache_path
        && let Ok(json) = serde_json::to_vec(&cache)
    {
        let _ = std::fs::write(path, json);
    }
    Ok(files)
}

/// A path segment as other people see it: backslashes would split it, so they go.
fn clean_segment(segment: &str) -> Option<String> {
    let cleaned = segment.replace('\\', "_");
    let cleaned = cleaned.trim();
    (!cleaned.is_empty() && cleaned != "." && cleaned != "..").then(|| cleaned.to_owned())
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// `GET /api/v1/sharing`
pub async fn status(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage sharing") {
        return denied;
    }
    Json(app.sharing.status(app.library.library_dir.as_deref())).into_response()
}

/// `PUT /api/v1/sharing`
pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(mut settings): Json<SharingSettings>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage sharing") {
        return denied;
    }
    settings.share_name = match clean_segment(&settings.share_name) {
        Some(name) if name.chars().count() <= 40 => name,
        _ => return error(StatusCode::BAD_REQUEST, "bad-share-name", "Give the shared folder a short name."),
    };
    if settings.enabled && app.library.library_dir.is_none() {
        return error(StatusCode::CONFLICT, "no-library", "Set a library folder before sharing it.");
    }
    settings.slots = settings.slots.clamp(1, 20);
    settings.queue_per_user = settings.queue_per_user.clamp(1, 10_000);
    settings.banned = settings.banned.into_iter().map(|b| b.trim().to_owned()).filter(|b| !b.is_empty()).collect();
    settings.banned.sort();
    settings.banned.dedup();

    tracing::info!(by = %user.username, enabled = settings.enabled, "sharing settings changed");
    app.sharing.save_settings(&settings);
    app.sharing.lock().settings = settings;
    refresh(&app);
    Json(app.sharing.status(app.library.library_dir.as_deref())).into_response()
}

/// `POST /api/v1/sharing/rescan`
pub async fn rescan(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage sharing") {
        return denied;
    }
    refresh(&app);
    StatusCode::ACCEPTED.into_response()
}

/// `GET /api/v1/soulseek/uploads`
pub async fn uploads(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "see uploads") {
        return denied;
    }
    let Some(client) = &app.soulseek else { return Json(Vec::<Upload>::new()).into_response() };
    let mut list: Vec<Upload> = client
        .uploads()
        .into_iter()
        .map(|u| {
            let (status, bytes, reason) = match u.state {
                UploadState::Queued => (UploadStatus::Queued, 0, None),
                UploadState::Connecting => (UploadStatus::Connecting, 0, None),
                UploadState::Transferring { bytes } => (UploadStatus::Transferring, bytes, None),
                UploadState::Completed { bytes } => (UploadStatus::Completed, bytes, None),
                UploadState::Failed { reason } => (UploadStatus::Failed, 0, Some(reason)),
                UploadState::Cancelled => (UploadStatus::Cancelled, 0, None),
            };
            Upload {
                id: u.id,
                username: u.username,
                filename: u.filename,
                size: u.size,
                bytes,
                status,
                reason,
                queued_at: u.queued_at,
                speed: u.speed,
            }
        })
        .collect();
    list.sort_by_key(|u| std::cmp::Reverse(u.id));
    Json(list).into_response()
}

/// Transfer totals that survive restarts: what was saved, plus this run's counters.
#[derive(Debug, Default)]
pub struct Totals {
    path: Option<PathBuf>,
    saved: Mutex<SavedTotals>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
struct SavedTotals {
    downloaded_bytes: u64,
    uploaded_bytes: u64,
}

impl Totals {
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("stats.json");
        let saved = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path: Some(path), saved: Mutex::new(saved) }
    }

    fn current(&self, client: Option<&delune_soulseek::Client>) -> (u64, u64) {
        let saved = *self.saved.lock().unwrap_or_else(PoisonError::into_inner);
        let (down, up) = client.map_or((0, 0), delune_soulseek::Client::transferred);
        (saved.downloaded_bytes + down, saved.uploaded_bytes + up)
    }

    /// Save periodically; the saved base plus the live counters is always the total.
    pub fn start(app: &AppState) {
        let app = app.clone();
        tokio::spawn(async move {
            let base = *app.totals.saved.lock().unwrap_or_else(PoisonError::into_inner);
            let mut every = tokio::time::interval(Duration::from_secs(60));
            loop {
                every.tick().await;
                let (down, up) = app.soulseek.as_ref().map_or((0, 0), delune_soulseek::Client::transferred);
                let totals = SavedTotals {
                    downloaded_bytes: base.downloaded_bytes + down,
                    uploaded_bytes: base.uploaded_bytes + up,
                };
                if let Some(path) = &app.totals.path
                    && let Ok(json) = serde_json::to_vec(&totals)
                {
                    let _ = std::fs::write(path, json);
                }
            }
        });
    }
}

/// `GET /api/v1/soulseek/stats`
pub async fn stats(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.search, "see Soulseek stats") {
        return denied;
    }
    let uploads = app.soulseek.as_ref().map(delune_soulseek::Client::uploads).unwrap_or_default();
    let count =
        |f: fn(&UploadState) -> bool| u32::try_from(uploads.iter().filter(|u| f(&u.state)).count()).unwrap_or(u32::MAX);
    let (downloaded_bytes, uploaded_bytes) = app.totals.current(app.soulseek.as_ref());
    let downloads_running = app
        .downloads
        .list()
        .iter()
        .filter(|j| matches!(j.status, delune_core::api::JobStatus::Queued | delune_core::api::JobStatus::Downloading))
        .count();
    let (files, folders) = {
        let inner = app.sharing.lock();
        (inner.files, inner.folders)
    };
    Json(delune_core::api::SoulseekStats {
        shared_files: files,
        shared_folders: folders,
        uploads_running: count(|s| matches!(s, UploadState::Connecting | UploadState::Transferring { .. })),
        uploads_waiting: count(|s| *s == UploadState::Queued),
        downloads_running: u32::try_from(downloads_running).unwrap_or(u32::MAX),
        downloaded_bytes,
        uploaded_bytes,
        uploads_completed: count(|s| matches!(s, UploadState::Completed { .. })),
    })
    .into_response()
}

/// `DELETE /api/v1/soulseek/uploads/{id}`
pub async fn cancel_upload(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<u64>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage uploads") {
        return denied;
    }
    match &app.soulseek {
        Some(client) if client.cancel_upload(id) => StatusCode::NO_CONTENT.into_response(),
        _ => error(StatusCode::NOT_FOUND, "no-such-upload", "That upload has already finished."),
    }
}

/// `POST /api/v1/soulseek/uploads/clear`: forget finished uploads.
pub async fn clear_uploads(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage uploads") {
        return denied;
    }
    if let Some(client) = &app.soulseek {
        client.clear_finished_uploads();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_music_under_a_virtual_folder_without_leaving_the_library() {
        let root = std::env::temp_dir().join(format!("delune-share-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let album = root.join("Talk Talk").join("Spirit of Eden");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01 The Rainbow.flac"), b"not really flac").unwrap();
        std::fs::write(album.join("cover.jpg"), b"jpg").unwrap();
        std::fs::write(album.join("notes.exe"), b"no").unwrap();
        std::fs::write(album.join(".hidden.flac"), b"no").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", root.join("outside")).unwrap();

        let cache = root.with_extension("cache.json");
        let mut paths: Vec<String> =
            scan(&root, "Shared", Some(&cache)).unwrap().into_iter().map(|f| f.file.path).collect();
        paths.sort();
        assert_eq!(
            paths,
            [r"Shared\Talk Talk\Spirit of Eden\01 The Rainbow.flac", r"Shared\Talk Talk\Spirit of Eden\cover.jpg"]
        );
        assert!(cache.exists(), "properties are cached for the next scan");
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_file(cache).unwrap();
    }

    #[test]
    fn cleans_segments() {
        assert_eq!(clean_segment(r"AC\DC").as_deref(), Some("AC_DC"));
        assert_eq!(clean_segment(".."), None);
        assert_eq!(clean_segment("  "), None);
    }
}
