//! Download jobs.
//!
//! A job is one folder from one peer: the release someone picked from search
//! results. All of its files are requested at once (the peer queues them and sends
//! as its upload slots allow) into a staging folder of its own,
//! `<data dir>/staging/<job id>/`.
//! Nothing touches the music library here; a finished job waits for review.
//!
//! Jobs are saved to `<data dir>/jobs.json` whenever they change, and unfinished
//! jobs resume on startup from whatever is already on disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
use tokio::sync::watch;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::review::{self, Checked, LibrarySettings};

/// All jobs plus the cancel switches of the ones still running.
#[derive(Debug, Default)]
pub struct Downloads {
    jobs: Mutex<Vec<Entry>>,
    counter: AtomicU32,
    /// Where jobs are saved; `None` keeps them in memory only (tests).
    store: Option<PathBuf>,
    dirty: AtomicBool,
}

#[derive(Debug)]
struct Entry {
    job: DownloadJob,
    cancel: watch::Sender<bool>,
    checked: Option<Checked>,
}

impl Downloads {
    /// Load saved jobs from `data_dir`. Jobs that were mid-download come back as
    /// queued, and reviews that were in progress will be redone.
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        let store = data_dir.join("jobs.json");
        let jobs: Vec<DownloadJob> = match std::fs::read(&store) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %store.display(), "couldn't read saved downloads; starting fresh");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        let entries = jobs
            .into_iter()
            .map(|mut job| {
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
                // A ready job's review isn't saved; it's recomputed on startup.
                if job.status == JobStatus::Ready {
                    job.review = ReviewState::Waiting;
                }
                Entry { job, cancel: watch::channel(false).0, checked: None }
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            tracing::info!(jobs = entries.len(), "restored saved downloads");
        }
        Self {
            jobs: Mutex::new(entries),
            counter: AtomicU32::new(0),
            store: Some(store),
            dirty: AtomicBool::new(false),
        }
    }

    /// Write jobs to disk if anything changed since the last save.
    pub fn save_if_changed(&self) {
        let Some(store) = &self.store else { return };
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let jobs = self.list();
        let result = serde_json::to_vec_pretty(&jobs).map_err(std::io::Error::other).and_then(|bytes| {
            if let Some(parent) = store.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = store.with_extension("json.tmp");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, store)
        });
        if let Err(error) = result {
            tracing::warn!(%error, "couldn't save downloads");
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    fn changed(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn new_id(&self) -> String {
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        format!("{millis:x}{:04x}", self.counter.fetch_add(1, Ordering::Relaxed) & 0xffff)
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut DownloadJob)) {
        if let Some(entry) = self.lock().iter_mut().find(|e| e.job.id == id) {
            f(&mut entry.job);
            entry.job.refresh();
        }
        self.changed();
    }

    /// Status, review state and the review itself, when there is one.
    pub fn review(&self, id: &str) -> Option<(JobStatus, ReviewState, Option<Checked>)> {
        self.lock().iter().find(|e| e.job.id == id).map(|e| (e.job.status, e.job.review, e.checked.clone()))
    }

    /// Who started a job: `None` if there's no such job, `Some(None)` for jobs from before accounts.
    pub fn owner(&self, id: &str) -> Option<Option<String>> {
        self.lock().iter().find(|e| e.job.id == id).map(|e| e.job.requested_by.clone())
    }

    pub fn mark_imported(&self, id: &str) {
        if let Some(entry) = self.lock().iter_mut().find(|e| e.job.id == id) {
            entry.job.status = JobStatus::Imported;
            entry.checked = None;
        }
        self.changed();
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
pub async fn list(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<DownloadJob>> {
    Json(app.downloads.list().into_iter().filter(|job| user.can_see(job.requested_by.as_deref())).collect())
}

/// `POST /api/v1/downloads`
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
    };
    let (cancel, cancel_rx) = watch::channel(false);
    app.downloads.lock().push(Entry { job: job.clone(), cancel, checked: None });
    app.downloads.changed();
    tracing::info!(%id, username = %request.username, folder = %request.folder, files = job.files.len(), "download job created");
    start(app, client, &job, cancel_rx);
    Ok(job)
}

/// Download a job's remaining files, then check them for review.
fn start(app: &AppState, client: delune_soulseek::Client, job: &DownloadJob, cancel: watch::Receiver<bool>) {
    let context = ReleaseContext { artist: job.parent.clone(), album: job.title.clone(), source: "Soulseek".into() };
    let staging = staging_dir(&app.data_dir, &job.id);
    let downloads = app.downloads.clone();
    let library = app.library.clone();
    let (id, username, files) = (job.id.clone(), job.username.clone(), job.files.clone());
    tokio::spawn(async move {
        run_job(&downloads, client, &id, username, files, staging.clone(), cancel).await;
        let ready = downloads.review(&id).is_some_and(|(status, ..)| status == JobStatus::Ready);
        if ready {
            check_job(&downloads, &id, staging, context, library).await;
        }
    });
}

/// Pick up where saved jobs left off: resume unfinished downloads and redo
/// reviews. Call once at startup.
pub fn resume(app: &AppState) {
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
            let context =
                ReleaseContext { artist: job.parent.clone(), album: job.title.clone(), source: "Soulseek".into() };
            let (downloads, library, staging) =
                (app.downloads.clone(), app.library.clone(), staging_dir(&app.data_dir, &job.id));
            tokio::spawn(async move { check_job(&downloads, &job.id, staging, context, library).await });
        } else if let Some(client) = app.soulseek.clone() {
            tracing::info!(id = %job.id, title = %job.title, "resuming download");
            start(app, client, &job, cancel);
        }
    }
}

/// `DELETE /api/v1/downloads/{id}`: cancel if running, remove staged files, forget the job.
pub async fn remove(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let removed = {
        let mut jobs = app.downloads.lock();
        jobs.iter().position(|e| e.job.id == id && user.can_see(e.job.requested_by.as_deref())).map(|i| jobs.remove(i))
    };
    let Some(entry) = removed else {
        return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
    };
    app.downloads.changed();
    let _ = entry.cancel.send(true);
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
async fn check_job(
    downloads: &Downloads,
    id: &str,
    staging: PathBuf,
    context: ReleaseContext,
    library: Arc<LibrarySettings>,
) {
    downloads.set_review(id, ReviewState::Checking, None);
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
            loop {
                let current = state.borrow_and_update().clone();
                downloads.update(&id, |job| apply(&mut job.files[index], &current));
                if current.is_finished() || state.changed().await.is_err() {
                    break;
                }
            }
        });
    }

    tokio::select! {
        () = async { while tasks.join_next().await.is_some() {} } => {}
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
        }];
        std::fs::write(dir.join("jobs.json"), serde_json::to_vec(&saved).unwrap()).unwrap();

        let downloads = Downloads::open(&dir);
        let job = &downloads.list()[0];
        let statuses: Vec<_> = job.files.iter().map(|f| f.status).collect();
        assert_eq!(statuses, [FileStatus::Done, FileStatus::Waiting, FileStatus::Waiting]);
        assert_eq!(job.files[2].place_in_queue, None);
        assert_eq!(job.status, JobStatus::Downloading);
        assert_eq!(job.review, ReviewState::Waiting);

        // Changes are saved and read back.
        downloads.update("job1", |job| job.files[1].status = FileStatus::Done);
        downloads.save_if_changed();
        assert_eq!(Downloads::open(&dir).list()[0].files[1].status, FileStatus::Done);
        std::fs::remove_dir_all(dir).unwrap();
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
