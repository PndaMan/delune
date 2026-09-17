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
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::Query;
use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::Codec;
use delune_core::api::{ApiError, SharingSettings, SharingStatus, TransferHistory, Upload, UploadRecord, UploadStatus};
use delune_soulseek::peer::SharedFile;
use delune_soulseek::{IndexedFile, ShareIndex, UploadLimits, UploadState};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

const SCHEDULE_CHECK_EVERY: Duration = Duration::from_secs(30);
const RESCAN_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Non-audio files worth sharing alongside the music.
const EXTRAS: &[&str] = &["jpg", "jpeg", "png", "webp", "cue", "log", "m3u", "m3u8", "txt", "nfo", "pdf"];

#[derive(Debug, Default)]
pub struct Sharing {
    store: Option<Arc<Database>>,
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
    scheduled: bool,
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
    /// Saved settings from `db`; the index cache stays a file in `data_dir`, since
    /// it's large and can always be rebuilt.
    pub fn open(db: &Arc<Database>, data_dir: &Path) -> Self {
        let settings = db.load("sharing").unwrap_or_default();
        Self {
            store: Some(db.clone()),
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

    pub(crate) fn status(&self, library_dir: Option<&Path>) -> SharingStatus {
        let inner = self.lock();
        SharingStatus {
            settings: inner.settings.clone(),
            library_dir: library_dir.map(|p| p.display().to_string()),
            scanning: inner.scanning,
            files: inner.files,
            folders: inner.folders,
            last_scan: inner.last_scan,
            error: inner.error.clone(),
            scheduled: inner.scheduled,
        }
    }

    fn save_settings(&self, settings: &SharingSettings) {
        if let Some(db) = &self.store {
            db.save("sharing", settings);
        }
    }
}

/// Whether the speed schedule is in force right now.
fn schedule_active(settings: &SharingSettings) -> bool {
    let Some(schedule) = &settings.schedule else { return false };
    let zone = jiff::tz::TimeZone::get(&schedule.time_zone).unwrap_or_else(|_| jiff::tz::TimeZone::system());
    let time = jiff::Timestamp::now().to_zoned(zone).time();
    let minute = u16::from(time.hour().unsigned_abs()) * 60 + u16::from(time.minute().unsigned_abs());
    schedule.covers(minute)
}

/// Upload and download caps in bytes per second, from the schedule when it's in force.
fn speed_caps(settings: &SharingSettings, scheduled: bool) -> (Option<u64>, Option<u64>) {
    let (upload, download) = match (&settings.schedule, scheduled) {
        (Some(schedule), true) => (schedule.upload_limit_kib, schedule.download_limit_kib),
        _ => (settings.speed_limit_kib, settings.download_limit_kib),
    };
    let bytes = |kib: Option<u32>| kib.filter(|&k| k > 0).map(|k| u64::from(k) * 1024);
    (bytes(upload), bytes(download))
}

fn limits(settings: &SharingSettings, upload_cap: Option<u64>) -> UploadLimits {
    UploadLimits {
        slots: usize::try_from(settings.slots.clamp(1, 20)).unwrap_or(3),
        queue_per_user: usize::try_from(settings.queue_per_user.clamp(1, 10_000)).unwrap_or(200),
        bytes_per_second: upload_cap,
        refuse_leechers: settings.refuse_leechers,
    }
}

/// The profile description other users see.
fn profile_text(settings: &SharingSettings) -> &str {
    settings.description.as_deref().map(str::trim).filter(|d| !d.is_empty()).unwrap_or("Sharing with delune")
}

/// Apply the speed limits in force now, and the limit on downloads at once.
fn apply_speeds(app: &AppState, settings: &SharingSettings) {
    app.downloads.set_slots(settings.downloads_at_once);
    let scheduled = schedule_active(settings);
    app.sharing.lock().scheduled = scheduled;
    let Some(client) = &app.soulseek else { return };
    let (upload, download) = speed_caps(settings, scheduled);
    client.set_description(profile_text(settings));
    client.set_upload_limits(limits(settings, upload));
    client.set_download_limit(download);
}

/// Apply settings to the Soulseek client and (re)index if sharing is on. Call at
/// startup, after settings change, and after imports.
pub fn refresh(app: &AppState) {
    let settings = app.sharing.settings();
    apply_speeds(app, &settings);
    let Some(client) = app.soulseek.clone() else { return };
    client.set_banned(settings.banned.iter().cloned().collect::<HashSet<_>>());
    // Relaying only makes sense while sharing: without shares delune doesn't join the tree.
    let children = if settings.enabled { settings.distributed_children.min(50) } else { 0 };
    client.set_distributed_children(usize::try_from(children).unwrap_or(0));

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
    let schedule_app = app.clone();
    tokio::spawn(async move {
        // Switch speed limits when the schedule's window opens or closes.
        let mut every = tokio::time::interval(SCHEDULE_CHECK_EVERY);
        loop {
            every.tick().await;
            let settings = schedule_app.sharing.settings();
            if settings.schedule.is_some() && schedule_active(&settings) != schedule_app.sharing.lock().scheduled {
                tracing::info!(scheduled = !schedule_app.sharing.lock().scheduled, "speed schedule changed limits");
                apply_speeds(&schedule_app, &settings);
            }
        }
    });
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
#[utoipa::path(
    get,
    operation_id = "sharing_status",
    path = "/api/v1/sharing",
    tag = "sharing",
    responses(
        (status = 200, description = "OK", body = delune_core::api::SharingStatus),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn status(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage sharing") {
        return denied;
    }
    Json(app.sharing.status(app.library.library_dir.as_deref())).into_response()
}

/// `PUT /api/v1/sharing`
#[utoipa::path(
    put,
    operation_id = "sharing_update",
    path = "/api/v1/sharing",
    tag = "sharing",
    request_body = delune_core::api::SharingSettings,
    responses(
        (status = 200, description = "OK", body = delune_core::api::SharingStatus),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 400, description = "Bad settings", body = delune_core::api::ApiError),
        (status = 409, description = "Can't right now", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
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
    settings.downloads_at_once = settings.downloads_at_once.filter(|&n| n > 0).map(|n| n.min(100));
    settings.distributed_children = settings.distributed_children.min(50);
    settings.banned = settings.banned.into_iter().map(|b| b.trim().to_owned()).filter(|b| !b.is_empty()).collect();
    if let Some(schedule) = &settings.schedule {
        if schedule.start_minute >= 24 * 60
            || schedule.end_minute >= 24 * 60
            || schedule.start_minute == schedule.end_minute
        {
            return error(
                StatusCode::BAD_REQUEST,
                "bad-schedule",
                "Choose two different times of day for the schedule.",
            );
        }
        if jiff::tz::TimeZone::get(&schedule.time_zone).is_err() {
            return error(StatusCode::BAD_REQUEST, "bad-time-zone", "The schedule's time zone isn't one delune knows.");
        }
    }
    settings.banned.sort();
    settings.banned.dedup();

    tracing::info!(by = %user.username, enabled = settings.enabled, "sharing settings changed");
    app.sharing.save_settings(&settings);
    app.sharing.lock().settings = settings;
    app.nat_wake.send_modify(|n| *n += 1);
    crate::events::changed(&app, crate::events::Topic::Sharing);
    refresh(&app);
    Json(app.sharing.status(app.library.library_dir.as_deref())).into_response()
}

/// `POST /api/v1/sharing/rescan`
#[utoipa::path(
    post,
    operation_id = "sharing_rescan",
    path = "/api/v1/sharing/rescan",
    tag = "sharing",
    responses(
        (status = 202, description = "Rescanning", body = delune_core::api::SharingStatus),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn rescan(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "manage sharing") {
        return denied;
    }
    refresh(&app);
    StatusCode::ACCEPTED.into_response()
}

/// `GET /api/v1/soulseek/uploads`
#[utoipa::path(
    get,
    operation_id = "sharing_uploads",
    path = "/api/v1/soulseek/uploads",
    tag = "uploads",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::Upload>),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn uploads(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "see uploads") {
        return denied;
    }
    let Some(client) = &app.soulseek else { return Json(Vec::<Upload>::new()).into_response() };
    let mut list: Vec<Upload> = client
        .uploads()
        .into_iter()
        .map(|u| {
            let (status, bytes, reason) = status_of(u.state);
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
    store: Option<Arc<Database>>,
    saved: Mutex<SavedTotals>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
struct SavedTotals {
    downloaded_bytes: u64,
    uploaded_bytes: u64,
}

impl Totals {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let saved = db.load("stats").unwrap_or_default();
        Self { store: Some(db.clone()), saved: Mutex::new(saved) }
    }

    fn current(&self, client: Option<&delune_soulseek::Client>) -> (u64, u64) {
        let saved = *self.saved.lock().unwrap_or_else(PoisonError::into_inner);
        let (down, up) = client.map_or((0, 0), delune_soulseek::Client::transferred);
        (saved.downloaded_bytes + down, saved.uploaded_bytes + up)
    }

    /// Save periodically; the saved base plus the live counters is always the total.
    /// Also keeps the hourly history and the record of finished uploads.
    pub fn start(app: &AppState) {
        if let Some(client) = app.soulseek.clone() {
            let db = app.db.clone();
            tokio::spawn(async move {
                let mut finished = client.finished_uploads();
                loop {
                    match finished.recv().await {
                        Ok(upload) => db.record_upload(&record(upload)),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                            tracing::warn!(missed, "upload history fell behind");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        let app = app.clone();
        tokio::spawn(async move {
            let base = *app.totals.saved.lock().unwrap_or_else(PoisonError::into_inner);
            let mut every = tokio::time::interval(Duration::from_secs(60));
            let mut last = (0, 0);
            loop {
                every.tick().await;
                let (down, up) = app.soulseek.as_ref().map_or((0, 0), delune_soulseek::Client::transferred);
                app.db.add_transfer(unix_now(), up.saturating_sub(last.1), down.saturating_sub(last.0));
                last = (down, up);
                let totals = SavedTotals {
                    downloaded_bytes: base.downloaded_bytes + down,
                    uploaded_bytes: base.uploaded_bytes + up,
                };
                if let Some(db) = &app.totals.store {
                    db.save("stats", &totals);
                }
            }
        });
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn status_of(state: UploadState) -> (UploadStatus, u64, Option<String>) {
    match state {
        UploadState::Queued => (UploadStatus::Queued, 0, None),
        UploadState::Connecting => (UploadStatus::Connecting, 0, None),
        UploadState::Transferring { bytes } => (UploadStatus::Transferring, bytes, None),
        UploadState::Completed { bytes } => (UploadStatus::Completed, bytes, None),
        UploadState::Failed { reason } => (UploadStatus::Failed, 0, Some(reason)),
        UploadState::Cancelled => (UploadStatus::Cancelled, 0, None),
    }
}

fn record(upload: delune_soulseek::UploadInfo) -> UploadRecord {
    let (status, bytes, reason) = status_of(upload.state);
    UploadRecord {
        username: upload.username,
        filename: upload.filename,
        size: upload.size,
        bytes,
        status,
        reason,
        speed: upload.speed,
        finished_at: unix_now(),
    }
}

#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    /// `7d`, `30d` (the default) or `all`.
    #[serde(default)]
    period: Option<String>,
}

/// `GET /api/v1/soulseek/uploads/history`
#[utoipa::path(
    get,
    operation_id = "sharing_history",
    path = "/api/v1/soulseek/uploads/history",
    tag = "uploads",
    params(("period" = Option<String>, Query, description = "`7d`, `30d` (the default) or `all`")),
    responses(
        (status = 200, description = "OK", body = delune_core::api::TransferHistory),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn history(State(app): State<AppState>, user: CurrentUser, Query(params): Query<HistoryParams>) -> Response {
    const DAY: u64 = 86_400;
    if let Some(denied) = user.refuse_unless(|p| p.manage, "see who downloads from you") {
        return denied;
    }
    let now = unix_now();
    let since = match params.period.as_deref() {
        Some("7d") => Some(now - 7 * DAY),
        Some("all") => None,
        _ => Some(now - 30 * DAY),
    };
    let from = since.unwrap_or(0);
    let db = app.db.clone();
    let (all_down, all_up) = app.totals.current(app.soulseek.as_ref());
    let history = tokio::task::spawn_blocking(move || {
        let hours = db.transfer_hours(since.unwrap_or(now - 90 * DAY));
        let (uploaded_bytes, downloaded_bytes) = if since.is_some() {
            hours.iter().fold((0, 0), |(u, d), h| (u + h.uploaded_bytes, d + h.downloaded_bytes))
        } else {
            (all_up, all_down)
        };
        let (files_sent, people) = db.uploads_sent(from);
        let top_albums = db
            .upload_folders(from, 12)
            .into_iter()
            .map(|(folder, mut album)| {
                let (title, parent) = crate::search::display_names(&folder);
                album.title = title;
                album.parent = parent;
                album
            })
            .collect();
        TransferHistory {
            since,
            uploaded_bytes,
            downloaded_bytes,
            all_time_uploaded_bytes: all_up,
            all_time_downloaded_bytes: all_down,
            files_sent,
            people,
            hours,
            top_people: db.upload_people(from, 12),
            top_albums,
            recent: db.recent_uploads(from, 80),
        }
    })
    .await;
    match history {
        Ok(history) => Json(history).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "history-failed", "Couldn't read the upload history."),
    }
}

/// `GET /api/v1/soulseek/stats`
#[utoipa::path(
    get,
    operation_id = "sharing_stats",
    path = "/api/v1/soulseek/stats",
    tag = "soulseek",
    responses(
        (status = 200, description = "OK", body = delune_core::api::SoulseekStats),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
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
        distributed_children: app
            .soulseek
            .as_ref()
            .map_or(0, |c| u32::try_from(c.distributed_children()).unwrap_or(u32::MAX)),
    })
    .into_response()
}

/// `DELETE /api/v1/soulseek/uploads/{id}`
#[utoipa::path(
    delete,
    operation_id = "sharing_cancel_upload",
    path = "/api/v1/soulseek/uploads/{id}",
    tag = "uploads",
    params(
        ("id" = u64, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
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
#[utoipa::path(
    post,
    operation_id = "sharing_clear_uploads",
    path = "/api/v1/soulseek/uploads/clear",
    tag = "uploads",
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
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
    fn scheduled_limits_replace_the_usual_ones_only_in_their_window() {
        let mut settings =
            SharingSettings { speed_limit_kib: Some(1000), download_limit_kib: None, ..SharingSettings::default() };
        assert_eq!(speed_caps(&settings, true), (Some(1_024_000), None));
        settings.schedule = Some(delune_core::api::SpeedSchedule {
            start_minute: 0,
            end_minute: 1,
            upload_limit_kib: None,
            download_limit_kib: Some(10),
            time_zone: "Europe/London".into(),
        });
        assert_eq!(speed_caps(&settings, false), (Some(1_024_000), None));
        assert_eq!(speed_caps(&settings, true), (None, Some(10_240)));
    }

    #[test]
    fn cleans_segments() {
        assert_eq!(clean_segment(r"AC\DC").as_deref(), Some("AC_DC"));
        assert_eq!(clean_segment(".."), None);
        assert_eq!(clean_segment("  "), None);
    }
}
