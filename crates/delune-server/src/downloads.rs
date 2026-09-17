//! Download jobs.
//!
//! A job is one folder from one peer: the release someone picked from search
//! results. All of its files are requested at once (the peer queues them and sends
//! as its upload slots allow) into a staging folder of its own,
//! `<data dir>/staging/<job id>/`.
//! Nothing touches the music library here; a finished job waits for review.
//!
//! When only a few jobs may download at once, the rest wait for a slot in order
//! of priority, then age.
//!
//! Jobs are saved to `<data dir>/jobs.json` whenever they change, and unfinished
//! jobs resume on startup from whatever is already on disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, DownloadJob, DownloadJobRequest, FileStatus, JobFile, JobStatus, ReviewState};
use delune_library::import::ReleaseContext;
use delune_soulseek::{DownloadRequest, DownloadState};
use tokio::sync::{Notify, broadcast, watch};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::review::{self, Checked, LibrarySettings};
use crate::store::Database;

/// All jobs plus the cancel switches of the ones still running.
#[derive(Debug)]
pub struct Downloads {
    jobs: Mutex<Vec<Entry>>,
    counter: AtomicU32,
    /// Where jobs are saved; `None` keeps them in memory only (tests).
    store: Option<Arc<Database>>,
    dirty: AtomicBool,
    /// Jobs downloading at once; 0 means no limit.
    slots: AtomicUsize,
    /// Wakes jobs waiting for a slot when one frees up or the order changes.
    slot_freed: Notify,
    /// Each job whose status changed, with the status it had before.
    status_changes: broadcast::Sender<(DownloadJob, JobStatus)>,
}

impl Default for Downloads {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
            counter: AtomicU32::new(0),
            store: None,
            dirty: AtomicBool::new(false),
            slots: AtomicUsize::new(0),
            slot_freed: Notify::new(),
            status_changes: broadcast::channel(64).0,
        }
    }
}

#[derive(Debug)]
struct Entry {
    job: DownloadJob,
    cancel: watch::Sender<bool>,
    checked: Option<Checked>,
    slot: Slot,
}

/// Where a job stands with the limit on downloads at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Slot {
    #[default]
    None,
    Waiting,
    Holding,
}

impl Downloads {
    /// Load saved jobs. Jobs that were mid-download come back as queued, and reviews
    /// that were in progress will be redone.
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let jobs: Vec<DownloadJob> = db.load("jobs").unwrap_or_default();
        let entries = jobs
            .into_iter()
            .map(|mut job| {
                // A fetch command that got nothing has no files to work the status out from.
                let failed_empty = job.status == JobStatus::Failed && job.files.is_empty();
                // A fetch (command or Bandcamp) can't pick up where it left off.
                let interrupted = is_external(&job) && matches!(job.status, JobStatus::Queued | JobStatus::Downloading);
                for file in &mut job.files {
                    if !matches!(file.status, FileStatus::Done | FileStatus::Failed | FileStatus::Cancelled) {
                        file.status = FileStatus::Waiting;
                        file.place_in_queue = None;
                    }
                }
                if job.review != ReviewState::Ready {
                    job.review = ReviewState::Waiting;
                }
                job.refresh();
                if failed_empty {
                    job.status = JobStatus::Failed;
                }
                if interrupted {
                    job.status = JobStatus::Failed;
                    job.error = Some("delune restarted before this finished. Fetch it again.".into());
                }
                // A ready job's review isn't saved; it's recomputed on startup.
                if job.status == JobStatus::Ready {
                    job.review = ReviewState::Waiting;
                }
                job.waiting_for_slot = None;
                Entry { job, cancel: watch::channel(false).0, checked: None, slot: Slot::None }
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            tracing::info!(jobs = entries.len(), "restored saved downloads");
        }
        Self {
            jobs: Mutex::new(entries),
            counter: AtomicU32::new(0),
            store: Some(db.clone()),
            dirty: AtomicBool::new(false),
            slots: AtomicUsize::new(0),
            slot_freed: Notify::new(),
            status_changes: broadcast::channel(64).0,
        }
    }

    /// Jobs whose status changes from now on, with their status before.
    pub fn status_changes(&self) -> broadcast::Receiver<(DownloadJob, JobStatus)> {
        self.status_changes.subscribe()
    }

    /// Change how many jobs may download at once; `None` or 0 means no limit.
    pub fn set_slots(&self, slots: Option<u32>) {
        let slots = slots.map_or(0, |s| usize::try_from(s).unwrap_or(usize::MAX));
        if self.slots.swap(slots, Ordering::Relaxed) != slots {
            self.wake_waiting();
        }
    }

    fn wake_waiting(&self) {
        self.slot_freed.notify_waiters();
    }

    /// Take a slot for `id` if one is free and nothing waiting is ahead of it;
    /// otherwise mark it waiting. Also renumbers the line.
    fn try_take_slot(&self, id: &str) -> bool {
        let limit = self.slots.load(Ordering::Relaxed);
        let mut jobs = self.lock();
        let holding = jobs.iter().filter(|e| e.slot == Slot::Holding).count();
        if let Some(entry) = jobs.iter_mut().find(|e| e.job.id == id)
            && entry.slot == Slot::None
        {
            entry.slot = Slot::Waiting;
        }
        let mut line: Vec<usize> = (0..jobs.len()).filter(|&i| jobs[i].slot == Slot::Waiting).collect();
        line.sort_by(|&a, &b| {
            let (a, b) = (&jobs[a].job, &jobs[b].job);
            b.priority.cmp(&a.priority).then(a.created_at.cmp(&b.created_at)).then(a.id.cmp(&b.id))
        });
        let free = if limit == 0 { usize::MAX } else { limit.saturating_sub(holding) };
        let mut taken = false;
        for (place, &i) in line.iter().enumerate() {
            let entry = &mut jobs[i];
            if place < free && entry.job.id == id {
                entry.slot = Slot::Holding;
                entry.job.waiting_for_slot = None;
                taken = true;
            } else {
                // Places count from the jobs that can't start yet.
                let place = place.saturating_sub(free.min(line.len())) + 1;
                entry.job.waiting_for_slot = Some(u32::try_from(place).unwrap_or(u32::MAX));
            }
        }
        drop(jobs);
        self.changed();
        taken
    }

    /// Give up `id`'s slot, or its place in line.
    fn release_slot(&self, id: &str) {
        if let Some(entry) = self.lock().iter_mut().find(|e| e.job.id == id) {
            entry.slot = Slot::None;
            entry.job.waiting_for_slot = None;
        }
        self.changed();
        self.wake_waiting();
    }

    /// Wait until `id` may download. False if it was cancelled while waiting.
    async fn wait_for_slot(&self, id: &str, cancel: &mut watch::Receiver<bool>) -> bool {
        loop {
            let freed = self.slot_freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            if self.try_take_slot(id) {
                return true;
            }
            tokio::select! {
                () = &mut freed => {}
                _ = cancel.changed() => {
                    self.release_slot(id);
                    return false;
                }
            }
        }
    }

    /// Move a waiting job to the front of the line.
    fn prioritise(&self, id: &str) -> bool {
        let mut jobs = self.lock();
        let top = jobs.iter().map(|e| e.job.priority).max().unwrap_or(0);
        let Some(entry) = jobs.iter_mut().find(|e| e.job.id == id && e.slot == Slot::Waiting) else {
            return false;
        };
        entry.job.priority = top + 1;
        drop(jobs);
        self.changed();
        self.wake_waiting();
        true
    }

    /// Save jobs if anything changed since the last save.
    /// Save the jobs if anything changed since last time. Returns whether it had.
    pub fn save_if_changed(&self) -> bool {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return false;
        }
        if let Some(db) = &self.store
            && !db.save("jobs", &self.list())
        {
            self.dirty.store(true, Ordering::Relaxed);
        }
        true
    }

    fn changed(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Tell whoever follows download statuses that a job is gone.
    fn announce_removed(&self, mut job: DownloadJob) {
        let before = job.status;
        job.status = JobStatus::Cancelled;
        let _ = self.status_changes.send((job, before));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn new_id(&self) -> String {
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        format!("{millis:x}{:04x}", self.counter.fetch_add(1, Ordering::Relaxed) & 0xffff)
    }

    /// Give a job the album and artist it turned out to be.
    pub(crate) fn retitle(&self, id: &str, title: &str, artist: Option<&str>) {
        self.update(id, |job| {
            title.clone_into(&mut job.title);
            job.parent = artist.map(str::to_owned);
        });
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut DownloadJob)) {
        let changed = self.lock().iter_mut().find(|e| e.job.id == id).and_then(|entry| {
            let before = entry.job.status;
            f(&mut entry.job);
            entry.job.refresh();
            (entry.job.status != before).then(|| (entry.job.clone(), before))
        });
        self.changed();
        if let Some(change) = changed {
            let _ = self.status_changes.send(change);
        }
    }

    /// Status, review state and the review itself, when there is one.
    pub fn review(&self, id: &str) -> Option<(JobStatus, ReviewState, Option<Checked>)> {
        self.lock().iter().find(|e| e.job.id == id).map(|e| (e.job.status, e.job.review, e.checked.clone()))
    }

    /// Who started a job: `None` if there's no such job, `Some(None)` for jobs from before accounts.
    pub fn owner(&self, id: &str) -> Option<Option<String>> {
        self.lock().iter().find(|e| e.job.id == id).map(|e| e.job.requested_by.clone())
    }

    /// What a fetch command left behind. Unlike a Soulseek job the status can't be
    /// worked out from the files: a fetch that got nothing has none.
    fn fetched(&self, id: &str, files: Vec<JobFile>, status: JobStatus, error: Option<String>) {
        let changed = self.lock().iter_mut().find(|e| e.job.id == id).map(|entry| {
            let before = entry.job.status;
            entry.job.files = files;
            entry.job.refresh();
            entry.job.status = status;
            entry.job.error.clone_from(&error);
            if let Some(error) = error {
                entry.job.files.iter_mut().for_each(|f| f.error = Some(error.clone()));
            }
            (entry.job.clone(), before)
        });
        self.changed();
        if let Some(change) = changed {
            let _ = self.status_changes.send(change);
        }
    }

    pub fn mark_imported(&self, id: &str, folder: &str) {
        let changed = self.lock().iter_mut().find(|e| e.job.id == id).map(|entry| {
            let before = entry.job.status;
            entry.job.status = JobStatus::Imported;
            entry.job.imported_to = Some(folder.to_owned());
            entry.job.imported_at = Some(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
            entry.checked = None;
            (entry.job.clone(), before)
        });
        self.changed();
        if let Some(change) = changed {
            let _ = self.status_changes.send(change);
        }
    }

    fn set_review(&self, id: &str, state: ReviewState, checked: Option<Checked>) {
        if let Some(entry) = self.lock().iter_mut().find(|e| e.job.id == id) {
            entry.job.review = state;
            entry.checked = checked;
        }
        self.changed();
    }

    #[must_use]
    pub fn list(&self) -> Vec<DownloadJob> {
        let mut jobs: Vec<DownloadJob> = self.lock().iter().map(|e| e.job.clone()).collect();
        jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        jobs
    }
}

/// `GET /api/v1/downloads`: your downloads, or everyone's if you manage delune.
#[utoipa::path(
    get,
    operation_id = "downloads_list",
    path = "/api/v1/downloads",
    tag = "downloads",
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::DownloadJob>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn list(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<DownloadJob>> {
    Json(app.downloads.list().into_iter().filter(|job| user.can_see(job.requested_by.as_deref())).collect())
}

/// `POST /api/v1/downloads`
#[utoipa::path(
    post,
    operation_id = "downloads_create",
    path = "/api/v1/downloads",
    tag = "downloads",
    request_body = delune_core::api::DownloadJobRequest,
    responses(
        (status = 201, description = "Started", body = delune_core::api::DownloadJob),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 400, description = "Bad request", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn create(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(request): Json<DownloadJobRequest>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.download, "download") {
        return denied;
    }
    match begin(&app, request, &user.username) {
        Ok(job) => (StatusCode::CREATED, Json(job)).into_response(),
        Err((status, code, message)) => error(status, code, &message),
    }
}

/// Create and start a download job for `requested_by`.
///
/// # Errors
///
/// The status, code and message to report when the request can't be started.
pub fn begin(
    app: &AppState,
    request: DownloadJobRequest,
    requested_by: &str,
) -> Result<DownloadJob, (StatusCode, &'static str, String)> {
    let Some(client) = app.soulseek.clone() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "soulseek-not-configured",
            "Soulseek isn't set up, so nothing can be downloaded.".into(),
        ));
    };
    if request.files.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no-files", "Choose at least one file to download.".into()));
    }
    if let Some(stray) = request.files.iter().find(|f| folder_of(&f.path) != request.folder) {
        return Err((
            StatusCode::BAD_REQUEST,
            "file-outside-folder",
            format!("{} isn't in the folder being downloaded.", stray.path),
        ));
    }

    let id = app.downloads.new_id();
    let job = DownloadJob {
        id: id.clone(),
        username: request.username.clone(),
        folder: request.folder.clone(),
        title: request.title,
        parent: request.parent,
        created_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        status: JobStatus::Queued,
        files: request
            .files
            .iter()
            .map(|f| JobFile {
                path: f.path.clone(),
                name: safe_file_name(&f.path),
                size: f.size,
                status: FileStatus::Waiting,
                bytes: 0,
                place_in_queue: None,
                error: None,
            })
            .collect(),
        bytes: 0,
        total_bytes: request.files.iter().map(|f| f.size).sum(),
        review: ReviewState::Waiting,
        requested_by: Some(requested_by.to_owned()),
        imported_to: None,
        imported_at: None,
        priority: 0,
        waiting_for_slot: None,
        error: None,
    };
    let (cancel, cancel_rx) = watch::channel(false);
    app.downloads.lock().push(Entry { job: job.clone(), cancel, checked: None, slot: Slot::None });
    app.downloads.changed();
    tracing::info!(%id, username = %request.username, folder = %request.folder, files = job.files.len(), "download job created");
    crate::events::changed(app, crate::events::Topic::Downloads);
    start(app, client, &job, cancel_rx);
    Ok(job)
}

/// Start a job that something other than Soulseek fills in, such as a fetch command.
/// A job fetched by a command or from Bandcamp rather than downloaded from a peer;
/// only Soulseek downloads name a folder.
pub(crate) fn is_external(job: &DownloadJob) -> bool {
    job.folder.is_empty()
}

/// Begin a fetch job. The receiver turns `true` when someone stops it.
pub fn begin_external(
    app: &AppState,
    source: &str,
    title: &str,
    artist: Option<&str>,
    requested_by: &str,
) -> (DownloadJob, watch::Receiver<bool>) {
    let id = app.downloads.new_id();
    // Just the program's name; the full path would read oddly in "Downloading from…".
    let name = Path::new(source).file_name().and_then(|n| n.to_str()).unwrap_or(source);
    let job = DownloadJob {
        id,
        username: name.to_owned(),
        folder: String::new(),
        title: title.to_owned(),
        parent: artist.map(str::to_owned),
        created_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        status: JobStatus::Downloading,
        files: Vec::new(),
        bytes: 0,
        total_bytes: 0,
        review: ReviewState::Waiting,
        requested_by: Some(requested_by.to_owned()),
        imported_to: None,
        imported_at: None,
        priority: 0,
        waiting_for_slot: None,
        error: None,
    };
    let (cancel, cancelled) = watch::channel(false);
    app.downloads.lock().push(Entry { job: job.clone(), cancel, checked: None, slot: Slot::None });
    app.downloads.changed();
    (job, cancelled)
}

/// Run a fetch for job `id` until it finishes or someone stops it, then record the outcome.
pub async fn run_external(
    app: &AppState,
    id: &str,
    mut cancelled: watch::Receiver<bool>,
    work: impl std::future::Future<Output = Result<(), String>>,
) {
    let stop = async {
        while !*cancelled.borrow() {
            if cancelled.changed().await.is_err() {
                // The job was removed; nobody is waiting for the result.
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        outcome = work => finish_external(app, id, outcome).await,
        () = stop => stopped_external(app, id),
    }
}

/// Someone stopped a fetch before it finished. Dropping the work stops it: a command
/// is killed, a transfer is abandoned.
fn stopped_external(app: &AppState, id: &str) {
    app.downloads.fetched(id, Vec::new(), JobStatus::Cancelled, Some("Stopped.".into()));
    crate::events::changed(app, crate::events::Topic::Downloads);
}

/// A fetch command finished: list what it left and send the job on to review.
pub async fn finish_external(app: &AppState, id: &str, outcome: Result<(), String>) {
    let staging = staging_dir(&app.data_dir, id);
    let found = tokio::task::spawn_blocking({
        let staging = staging.clone();
        move || collect(&staging)
    })
    .await
    .unwrap_or_default();
    let status = crate::external::status_of(found.len(), &outcome);
    let reason = match &outcome {
        Err(reason) => Some(reason.clone()),
        Ok(()) if found.is_empty() => Some("The command didn't leave any music behind.".to_owned()),
        Ok(()) => None,
    };
    if let Some(reason) = &reason {
        tracing::warn!(%id, %reason, "fetch command didn't work out");
    }
    let files = found
        .iter()
        .map(|(name, size)| JobFile {
            path: name.clone(),
            name: name.clone(),
            size: *size,
            status: if status == JobStatus::Ready { FileStatus::Done } else { FileStatus::Failed },
            bytes: *size,
            place_in_queue: None,
            error: reason.clone(),
        })
        .collect();
    app.downloads.fetched(id, files, status, reason);
    if status == JobStatus::Ready {
        let job = app.downloads.list().into_iter().find(|j| j.id == id);
        if let Some(job) = job {
            recheck(app, &job);
        }
    }
}

/// Files a fetch left in the staging folder, by name and size, deepest paths flattened.
fn collect(staging: &Path) -> Vec<(String, u64)> {
    let mut files = Vec::new();
    let mut folders = vec![staging.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let Some(name) = path.strip_prefix(staging).ok().and_then(|p| p.to_str()).map(str::to_owned) else {
                continue;
            };
            let size = entry.metadata().map_or(0, |m| m.len());
            files.push((name, size));
        }
    }
    files.sort();
    files
}

/// Download a job's remaining files, then check them for review.
fn start(app: &AppState, client: delune_soulseek::Client, job: &DownloadJob, cancel: watch::Receiver<bool>) {
    let context = ReleaseContext {
        artist: job.parent.clone(),
        album: job.title.clone(),
        source: "Soulseek".into(),
        ..ReleaseContext::default()
    };
    let staging = staging_dir(&app.data_dir, &job.id);
    let downloads = app.downloads.clone();
    let (id, username, files) = (job.id.clone(), job.username.clone(), job.files.clone());
    let app = app.clone();
    tokio::spawn(async move {
        let mut cancel = cancel;
        if !downloads.wait_for_slot(&id, &mut cancel).await {
            downloads.update(&id, |job| job.status = JobStatus::Cancelled);
            return;
        }
        run_job(&downloads, client, &id, username, files, staging.clone(), cancel).await;
        downloads.release_slot(&id);
        let ready = downloads.review(&id).is_some_and(|(status, ..)| status == JobStatus::Ready);
        if ready {
            check_job(&app, &id, staging, context).await;
            crate::review::auto_import(&app, &id).await;
        }
    });
}

/// Pick up where saved jobs left off: resume unfinished downloads and redo
/// reviews. Call once at startup.
pub fn resume(app: &AppState) {
    tidy(app);
    let pending: Vec<(DownloadJob, watch::Receiver<bool>)> = {
        let mut jobs = app.downloads.lock();
        jobs.iter_mut()
            .filter(|e| matches!(e.job.status, JobStatus::Queued | JobStatus::Downloading | JobStatus::Ready))
            .map(|e| {
                let (cancel, rx) = watch::channel(false);
                e.cancel = cancel;
                (e.job.clone(), rx)
            })
            .collect()
    };
    for (job, cancel) in pending {
        if job.status == JobStatus::Ready {
            recheck(app, &job);
        } else if let Some(client) = app.soulseek.clone() {
            tracing::info!(id = %job.id, title = %job.title, "resuming download");
            start(app, client, &job, cancel);
        }
    }
}

/// Finished jobs are kept this long after they finish, then forgotten.
const KEEP_FINISHED: u64 = 60 * 24 * 60 * 60;

/// Forget long-finished jobs, and delete staging folders no job owns any more.
fn tidy(app: &AppState) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let kept: std::collections::HashSet<String> = {
        let mut jobs = app.downloads.lock();
        let before = jobs.len();
        jobs.retain(|e| {
            let finished = matches!(e.job.status, JobStatus::Imported | JobStatus::Failed | JobStatus::Cancelled);
            let at = e.job.imported_at.unwrap_or(e.job.created_at);
            !finished || now.saturating_sub(at) < KEEP_FINISHED
        });
        if jobs.len() != before {
            tracing::info!(forgotten = before - jobs.len(), "forgot long-finished downloads");
        }
        jobs.iter().map(|e| e.job.id.clone()).collect()
    };
    app.downloads.changed();
    let staging = app.data_dir.join("staging");
    tokio::task::spawn_blocking(move || {
        let Ok(entries) = std::fs::read_dir(&staging) else { return };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !kept.contains(&name)
                && entry.file_type().is_ok_and(|t| t.is_dir())
                && let Err(error) = std::fs::remove_dir_all(entry.path())
            {
                tracing::warn!(%error, folder = %name, "couldn't remove a leftover staging folder");
            }
        }
    });
}

/// Check a finished job again, for example after the naming template changed.
fn recheck(app: &AppState, job: &DownloadJob) {
    let context = ReleaseContext {
        artist: job.parent.clone(),
        album: job.title.clone(),
        source: "Soulseek".into(),
        ..ReleaseContext::default()
    };
    let (id, staging) = (job.id.clone(), staging_dir(&app.data_dir, &job.id));
    let app = app.clone();
    tokio::spawn(async move {
        check_job(&app, &id, staging, context).await;
        // Fetched jobs and restarts come through here too.
        crate::review::auto_import(&app, &id).await;
    });
}

/// Check every album waiting in review again, so planned paths follow new naming settings.
pub fn recheck_reviews(app: &AppState) {
    let ready: Vec<DownloadJob> = app
        .downloads
        .lock()
        .iter()
        .filter(|e| e.job.status == JobStatus::Ready && e.job.review != ReviewState::Checking)
        .map(|e| e.job.clone())
        .collect();
    for job in &ready {
        recheck(app, job);
    }
}

/// `POST /api/v1/downloads/{id}/stop`: stop downloading but keep what arrived, to resume later.
#[utoipa::path(
    post,
    operation_id = "downloads_stop",
    path = "/api/v1/downloads/{id}/stop",
    tag = "downloads",
    params(
        ("id" = String, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 409, description = "Can't right now", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn stop(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let stopped = {
        let jobs = app.downloads.lock();
        jobs.iter()
            .find(|e| e.job.id == id && user.can_see(e.job.requested_by.as_deref()))
            .filter(|e| matches!(e.job.status, JobStatus::Queued | JobStatus::Downloading))
            .map(|e| e.cancel.send(true).is_ok())
    };
    match stopped {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => error(StatusCode::CONFLICT, "not-running", "That download isn't running."),
    }
}

/// `POST /api/v1/downloads/{id}/resume`: start a stopped or failed download again,
/// keeping finished files and resuming partial ones.
#[utoipa::path(
    post,
    operation_id = "downloads_resume_one",
    path = "/api/v1/downloads/{id}/resume",
    tag = "downloads",
    params(
        ("id" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::DownloadJob),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 409, description = "Can't right now", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn resume_one(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let external = app.downloads.lock().iter().any(|e| e.job.id == id && is_external(&e.job));
    if external {
        return error(
            StatusCode::CONFLICT,
            "fetch-again",
            "This came from a fetch, not Soulseek. Fetch it again from where you found it.",
        );
    }
    let Some(client) = app.soulseek.clone() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "soulseek-not-configured", "Soulseek isn't set up.");
    };
    let restarted = {
        let mut jobs = app.downloads.lock();
        let Some(entry) = jobs.iter_mut().find(|e| e.job.id == id && user.can_see(e.job.requested_by.as_deref()))
        else {
            return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
        };
        if !matches!(entry.job.status, JobStatus::Failed | JobStatus::Cancelled) {
            return error(StatusCode::CONFLICT, "not-stopped", "That download is already running or finished.");
        }
        for file in &mut entry.job.files {
            if file.status != FileStatus::Done {
                file.status = FileStatus::Waiting;
                file.error = None;
                file.place_in_queue = None;
            }
        }
        entry.job.status = JobStatus::Queued;
        entry.job.refresh();
        let (cancel, cancel_rx) = watch::channel(false);
        entry.cancel = cancel;
        (entry.job.clone(), cancel_rx)
    };
    app.downloads.changed();
    let (job, cancel_rx) = restarted;
    tracing::info!(%id, by = %user.username, "download resumed");
    start(&app, client, &job, cancel_rx);
    Json(job).into_response()
}

/// `POST /api/v1/downloads/{id}/prioritise`: start this waiting download next.
#[utoipa::path(
    post,
    operation_id = "downloads_prioritise",
    path = "/api/v1/downloads/{id}/prioritise",
    tag = "downloads",
    params(
        ("id" = String, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 409, description = "Can't right now", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn prioritise(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    if app.downloads.owner(&id).is_none_or(|owner| !user.can_see(owner.as_deref())) {
        return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
    }
    if app.downloads.prioritise(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        error(StatusCode::CONFLICT, "not-waiting", "That download isn't waiting for a turn.")
    }
}

/// `DELETE /api/v1/downloads/{id}`: cancel if running, remove staged files, forget the job.
#[utoipa::path(
    delete,
    operation_id = "downloads_remove",
    path = "/api/v1/downloads/{id}",
    tag = "downloads",
    params(
        ("id" = String, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 404, description = "Not found", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn remove(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let removed = {
        let mut jobs = app.downloads.lock();
        jobs.iter().position(|e| e.job.id == id && user.can_see(e.job.requested_by.as_deref())).map(|i| jobs.remove(i))
    };
    let Some(entry) = removed else {
        return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
    };
    app.downloads.changed();
    app.downloads.wake_waiting();
    crate::events::changed(&app, crate::events::Topic::Downloads);
    let _ = entry.cancel.send(true);
    app.downloads.announce_removed(entry.job.clone());
    // Job ids are generated here, so this path can't escape the staging folder.
    let staging = app.data_dir.join("staging").join(&entry.job.id);
    if let Err(error) = tokio::fs::remove_dir_all(&staging).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, path = %staging.display(), "couldn't remove staged files");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Verify and plan a finished job so it can be reviewed.
async fn check_job(app: &AppState, id: &str, staging: PathBuf, mut context: ReleaseContext) {
    let downloads = &app.downloads;
    let (template, options) = app.naming.current();
    let library = LibrarySettings { library_dir: app.library.library_dir.clone(), template, options };
    downloads.set_review(id, ReviewState::Checking, None);
    joining_existing(app, &mut context, &library).await;
    let result = tokio::task::spawn_blocking(move || review::check(&staging, &context, &library)).await;
    match result {
        Ok(Ok(checked)) => {
            tracing::info!(%id, tracks = checked.report.tracks.len(), blocked = ?checked.report.blocked_reason, "review ready");
            downloads.set_review(id, ReviewState::Ready, Some(checked));
        }
        Ok(Err(error)) => {
            tracing::warn!(%id, %error, "couldn't check downloaded files");
            downloads.set_review(id, ReviewState::Failed, None);
        }
        Err(_) => downloads.set_review(id, ReviewState::Failed, None),
    }
}

/// When the library already has this album, find its folder and current tracklist so
/// the new tracks join it instead of starting a second copy.
async fn joining_existing(app: &AppState, context: &mut ReleaseContext, library: &LibrarySettings) {
    let Some(root) = library.library_dir.clone() else { return };
    // Navidrome gives the album's own spelling when it knows it; the folders on disk
    // are checked either way, since Navidrome may not have scanned a recent import.
    let found = crate::library::lookup(app, context.artist.as_deref(), &context.album, None).await;
    let known = found.state == delune_core::api::LibraryState::InLibrary;
    let Some(artist) = found.artist.clone().filter(|_| known).or_else(|| context.artist.clone()) else { return };
    let album = found.album.clone().filter(|_| known).unwrap_or_else(|| context.album.clone());
    let (template, options, year) = (library.template.clone(), library.options.clone(), found.year.filter(|_| known));
    let existing = tokio::task::spawn_blocking(move || {
        delune_library::merge::find_existing(&root, &template, &options, &artist, &album, year)
    })
    .await
    .ok()
    .flatten();
    let Some(existing) = existing else {
        tracing::debug!(album = %context.album, "in the library, but its folder wasn't found");
        return;
    };
    context.tracklist = crate::music::tracklist(app, Some(&existing.album_artist), &existing.album).await;
    context.existing = Some(existing);
}

async fn run_job(
    downloads: &Arc<Downloads>,
    client: delune_soulseek::Client,
    id: &str,
    username: String,
    files: Vec<JobFile>,
    staging: PathBuf,
    mut cancel: watch::Receiver<bool>,
) {
    // Queue every remaining file with the peer at once. Uploaders serve their queue
    // in order through their slot, so asking for one file at a time would send us
    // to the back of the line after every track.
    let mut tasks = tokio::task::JoinSet::new();
    let mut handles = Vec::new();
    for (index, file) in files.iter().enumerate() {
        // Already downloaded before a restart.
        if file.status == FileStatus::Done && staging.join(&file.name).exists() {
            continue;
        }
        let download = client.download(DownloadRequest {
            username: username.clone(),
            filename: file.path.clone(),
            destination: staging.join(&file.name),
        });
        handles.push(download.clone());
        let (downloads, id) = (downloads.clone(), id.to_owned());
        tasks.spawn(async move {
            let mut state = download.state();
            // Timed from the first byte, so waiting in the peer's queue doesn't count.
            let mut transferring_since = None;
            loop {
                let current = state.borrow_and_update().clone();
                downloads.update(&id, |job| apply(&mut job.files[index], &current));
                if matches!(current, DownloadState::Transferring { .. }) && transferring_since.is_none() {
                    transferring_since = Some(std::time::Instant::now());
                }
                if current.is_finished() || state.changed().await.is_err() {
                    let elapsed = transferring_since.map_or(0, |at| at.elapsed().as_secs().max(1));
                    return match current {
                        DownloadState::Completed { bytes } => Outcome::Done { bytes, seconds: elapsed },
                        DownloadState::Failed { .. } => Outcome::Failed,
                        _ => Outcome::Other,
                    };
                }
            }
        });
    }

    let mut peer = (0u32, 0u32, 0u64, 0u64);
    tokio::select! {
        () = async {
            while let Some(outcome) = tasks.join_next().await {
                match outcome {
                    Ok(Outcome::Done { bytes, seconds }) => {
                        peer.0 += 1;
                        peer.2 += bytes;
                        peer.3 += seconds;
                    }
                    Ok(Outcome::Failed) => peer.1 += 1,
                    _ => {}
                }
            }
        } => {}
        _ = cancel.changed() => {
            for download in &handles {
                download.cancel();
            }
            downloads.update(id, |job| job.status = JobStatus::Cancelled);
            tasks.abort_all();
            return;
        }
    }
    let status = downloads.lock().iter().find(|e| e.job.id == id).map(|e| e.job.status);
    tracing::info!(id, ?status, "download job finished");
    if let (Some(db), true) = (&downloads.store, peer.0 + peer.1 > 0) {
        db.record_peer(&username, peer.0, peer.1, peer.2, peer.3);
    }
}

/// How one file's download ended, for the peer's history.
enum Outcome {
    Done { bytes: u64, seconds: u64 },
    Failed,
    Other,
}

fn apply(file: &mut JobFile, state: &DownloadState) {
    file.place_in_queue = None;
    match state {
        DownloadState::Connecting => file.status = FileStatus::Connecting,
        DownloadState::Queued { place } => {
            file.status = FileStatus::Queued;
            file.place_in_queue = *place;
        }
        DownloadState::Starting { .. } => file.status = FileStatus::Starting,
        DownloadState::Transferring { bytes, .. } => {
            file.status = FileStatus::Transferring;
            file.bytes = *bytes;
        }
        DownloadState::Completed { bytes } => {
            file.status = FileStatus::Done;
            file.bytes = *bytes;
        }
        DownloadState::Failed { reason } => {
            file.status = FileStatus::Failed;
            file.error = Some(reason.clone());
        }
        DownloadState::Cancelled => file.status = FileStatus::Cancelled,
    }
}

fn folder_of(path: &str) -> &str {
    path.rfind(['\\', '/']).map_or("", |i| &path[..i])
}

/// The last path component, made safe to use as a file name in the staging folder.
fn safe_file_name(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or_default();
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { "file".to_owned() } else { trimmed.to_owned() }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

/// Where staged files for `job_id` live.
#[must_use]
pub fn staging_dir(data_dir: &Path, job_id: &str) -> PathBuf {
    data_dir.join("staging").join(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_cannot_escape_staging() {
        assert_eq!(safe_file_name(r"@@moon\Music\Album\01 - Airbag.flac"), "01 - Airbag.flac");
        assert_eq!(safe_file_name(r"@@moon\Music\..\.."), "file");
        assert_eq!(safe_file_name("a/b/.hidden.flac"), "hidden.flac");
        assert_eq!(safe_file_name(r"x\What?: yes.mp3"), "What__ yes.mp3");
    }

    #[test]
    fn saved_jobs_restore_as_resumable() {
        let dir = std::env::temp_dir().join(format!("delune-jobs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |status, bytes| JobFile {
            path: format!("x\\{bytes}.flac"),
            name: format!("{bytes}.flac"),
            size: 10,
            status,
            bytes,
            place_in_queue: Some(3),
            error: None,
        };
        let saved = vec![DownloadJob {
            id: "job1".into(),
            username: "peer".into(),
            folder: "x".into(),
            title: "Album".into(),
            parent: None,
            created_at: 1,
            status: JobStatus::Downloading,
            files: vec![file(FileStatus::Done, 10), file(FileStatus::Transferring, 4), file(FileStatus::Queued, 0)],
            bytes: 14,
            total_bytes: 30,
            review: ReviewState::Checking,
            requested_by: None,
            imported_to: None,
            imported_at: None,
            priority: 0,
            waiting_for_slot: None,
            error: None,
        }];
        std::fs::write(dir.join("jobs.json"), serde_json::to_vec(&saved).unwrap()).unwrap();

        let db = Arc::new(Database::open(&dir).unwrap());
        let downloads = Downloads::open(&db);
        let job = &downloads.list()[0];
        let statuses: Vec<_> = job.files.iter().map(|f| f.status).collect();
        assert_eq!(statuses, [FileStatus::Done, FileStatus::Waiting, FileStatus::Waiting]);
        assert_eq!(job.files[2].place_in_queue, None);
        assert_eq!(job.status, JobStatus::Downloading);
        assert_eq!(job.review, ReviewState::Waiting);

        // Changes are saved and read back.
        downloads.update("job1", |job| job.files[1].status = FileStatus::Done);
        downloads.save_if_changed();
        drop(db);
        let reopened = Arc::new(Database::open(&dir).unwrap());
        assert_eq!(Downloads::open(&reopened).list()[0].files[1].status, FileStatus::Done);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn waiting_job(id: &str, created_at: u64) -> Entry {
        Entry {
            job: DownloadJob {
                id: id.into(),
                username: "peer".into(),
                folder: "x".into(),
                title: id.into(),
                parent: None,
                created_at,
                status: JobStatus::Queued,
                files: vec![],
                bytes: 0,
                total_bytes: 0,
                review: ReviewState::Waiting,
                requested_by: None,
                imported_to: None,
                imported_at: None,
                priority: 0,
                waiting_for_slot: None,
                error: None,
            },
            cancel: watch::channel(false).0,
            checked: None,
            slot: Slot::None,
        }
    }

    #[tokio::test]
    async fn jobs_take_turns_when_only_some_may_download() {
        let downloads = Arc::new(Downloads::default());
        downloads.set_slots(Some(1));
        downloads.lock().extend([waiting_job("a", 1), waiting_job("b", 2), waiting_job("c", 3)]);
        let place = |id: &str| downloads.list().into_iter().find(|j| j.id == id).unwrap().waiting_for_slot;

        assert!(downloads.try_take_slot("a"));
        assert!(!downloads.try_take_slot("c"));
        assert!(!downloads.try_take_slot("b"));
        assert_eq!((place("a"), place("b"), place("c")), (None, Some(1), Some(2)));

        // Moving c to the front lets it start before b once a finishes.
        assert!(downloads.prioritise("c"));
        assert!(!downloads.prioritise("a"), "a is downloading, not waiting");
        let ((_keep_b, mut cancel_b), (_keep_c, mut cancel_c)) = (watch::channel(false), watch::channel(false));
        let (d, e) = (downloads.clone(), downloads.clone());
        let b = tokio::spawn(async move { d.wait_for_slot("b", &mut cancel_b).await });
        let c = tokio::spawn(async move { e.wait_for_slot("c", &mut cancel_c).await });
        tokio::task::yield_now().await;
        assert!(!b.is_finished() && !c.is_finished());
        downloads.release_slot("a");
        assert!(c.await.unwrap());
        assert!(!b.is_finished());
        assert_eq!(place("b"), Some(1));

        // Raising the limit lets b start too.
        downloads.set_slots(None);
        assert!(b.await.unwrap());
    }

    #[tokio::test]
    async fn cancelling_a_waiting_job_leaves_the_line() {
        let downloads = Downloads::default();
        downloads.set_slots(Some(1));
        downloads.lock().extend([waiting_job("a", 1), waiting_job("b", 2)]);
        assert!(downloads.try_take_slot("a"));
        let (cancel, mut cancel_rx) = watch::channel(false);
        let waiting = downloads.wait_for_slot("b", &mut cancel_rx);
        tokio::pin!(waiting);
        tokio::select! {
            biased;
            _ = &mut waiting => panic!("b started while a held the only slot"),
            () = tokio::task::yield_now() => {}
        }
        cancel.send(true).unwrap();
        assert!(!waiting.await);
        assert_eq!(downloads.lock()[1].slot, Slot::None);
    }

    #[test]
    fn folders() {
        assert_eq!(folder_of(r"@@moon\Music\Album\01.flac"), r"@@moon\Music\Album");
        assert_eq!(folder_of("01.flac"), "");
    }

    #[test]
    fn download_states_map_to_file_statuses() {
        let mut file = JobFile {
            path: String::new(),
            name: String::new(),
            size: 100,
            status: FileStatus::Waiting,
            bytes: 0,
            place_in_queue: None,
            error: None,
        };
        apply(&mut file, &DownloadState::Queued { place: Some(4) });
        assert_eq!((file.status, file.place_in_queue), (FileStatus::Queued, Some(4)));
        apply(&mut file, &DownloadState::Transferring { bytes: 40, size: 100 });
        assert_eq!((file.status, file.bytes, file.place_in_queue), (FileStatus::Transferring, 40, None));
        apply(&mut file, &DownloadState::Failed { reason: "nope".into() });
        assert_eq!((file.status, file.error.as_deref()), (FileStatus::Failed, Some("nope")));
    }
}
