//! Adding music you already have: tracks, a folder or a zip, uploaded from the browser.
//!
//! An upload becomes a download like any other. Its files go through the same checks
//! (do they play, are they really lossless), are named from their tags, join an album
//! already in the library, and wait for review unless the uploader may import directly.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path as UrlPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, FinishUpload, UploadReceived, UploadSession};
use futures_util::StreamExt as _;
use tokio::io::AsyncWriteExt as _;

use crate::AppState;
use crate::accounts::CurrentUser;

/// The most one upload may add up to, unpacked.
pub const MAX_UPLOAD: u64 = 20 * 1024 * 1024 * 1024;

/// The most one piece of a session upload may carry. Well under Cloudflare's 100 MB.
pub const MAX_CHUNK: u64 = 64 * 1024 * 1024;

/// Sessions nobody has touched for this long are thrown away.
const SESSION_IDLE: Duration = Duration::from_secs(24 * 60 * 60);

const MAX_SESSION_FILES: usize = 10_000;

const AUDIO: &[&str] = &["flac", "alac", "wav", "aif", "aiff", "mp3", "m4a", "aac", "opus", "ogg", "oga", "wv", "ape"];
const IMAGES: &[&str] = &["jpg", "jpeg", "png", "webp"];

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn extension(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// Whether delune keeps a file of this name.
fn wanted(name: &str) -> bool {
    let ext = extension(name);
    AUDIO.contains(&ext.as_str()) || IMAGES.contains(&ext.as_str())
}

/// A safe, unused file name in the staging folder for `name` (which may carry a path).
fn slot(staging: &Path, taken: &mut HashSet<String>, name: &str) -> Option<PathBuf> {
    let base = Path::new(&name.replace('\\', "/")).file_name()?.to_str()?.trim().to_owned();
    let base: String = base
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    // Hidden files, like macOS's "._song.flac" shadows, aren't music.
    if base.is_empty() || base.starts_with('.') {
        return None;
    }
    let (stem, ext) = match base.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_owned(), format!(".{ext}")),
        None => (base.clone(), String::new()),
    };
    let mut candidate = base;
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    Some(staging.join(candidate))
}

/// Unpack the music and pictures in `archive` into `staging`, flat. Returns what it wrote.
fn unzip(archive: &Path, staging: &Path, taken: &mut HashSet<String>, budget: u64) -> Result<u64, String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "That isn't a readable zip.".to_owned())?;
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| "The zip is damaged.".to_owned())?;
        let Some(relative) = entry.enclosed_name() else { continue };
        let name = relative.to_string_lossy().into_owned();
        // macOS adds "__MACOSX/._name" shadows of every file.
        if entry.is_dir() || !wanted(&name) || name.contains("__MACOSX") {
            continue;
        }
        let Some(path) = slot(staging, taken, &name) else { continue };
        let mut out = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let room = budget.saturating_sub(total);
        let written = std::io::copy(&mut (&mut entry).take(room + 1), &mut out).map_err(|e| e.to_string())?;
        if written > room {
            return Err("That zip unpacks to far more than delune takes in one upload.".into());
        }
        total += written;
    }
    Ok(total)
}

/// The album and artist the uploaded files are tagged with, most common first.
fn tagged_release(staging: &Path) -> (Option<String>, Option<String>) {
    let mut albums: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut artists: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir(staging) else { return (None, None) };
    for entry in entries.flatten().take(200) {
        let path = entry.path();
        if !AUDIO.contains(&extension(&path.to_string_lossy()).as_str()) {
            continue;
        }
        let Ok(info) = delune_library::inspect::inspect(&path) else { continue };
        if let Some(album) = info.tags.album {
            *albums.entry(album).or_default() += 1;
        }
        if let Some(artist) = info.tags.album_artist.or(info.tags.artist) {
            *artists.entry(artist).or_default() += 1;
        }
    }
    let top = |m: std::collections::HashMap<String, usize>| m.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v);
    (top(albums), top(artists))
}

/// `POST /api/v1/uploads`: multipart with `file` fields (tracks, pictures or zips), and
/// optionally `title`, `artist` and `follow` (`true` to keep the album complete).
#[utoipa::path(
    post,
    operation_id = "uploads_create",
    path = "/api/v1/uploads",
    tag = "downloads",
    request_body(content_type = "multipart/form-data", description = "Files, and optional title, artist and follow"),
    responses(
        (status = 200, description = "Added, and being checked", body = delune_core::api::DownloadJob),
        (status = 400, description = "Nothing usable was uploaded", body = delune_core::api::ApiError),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn create(State(app): State<AppState>, user: CurrentUser, mut form: Multipart) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.download, "add music") {
        return denied;
    }
    let (job, _cancel) = crate::downloads::begin_external(&app, "upload", "Upload", None, &user.username);
    let staging = crate::downloads::staging_dir(&app.data_dir, &job.id);
    if let Err(e) = tokio::fs::create_dir_all(&staging).await {
        crate::downloads::finish_external(&app, &job.id, Err(format!("Couldn't store the upload: {e}"))).await;
        return error(StatusCode::INTERNAL_SERVER_ERROR, "store-failed", "Couldn't store the upload.");
    }

    let mut taken = HashSet::new();
    let (mut title, mut artist, mut follow) = (None, None, false);
    let mut total = 0u64;
    let mut failure: Option<String> = None;
    loop {
        let field = match form.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => {
                failure = Some(format!("The upload was interrupted: {e}"));
                break;
            }
        };
        match field.name().unwrap_or_default() {
            "title" => title = field.text().await.ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()),
            "artist" => artist = field.text().await.ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()),
            "follow" => follow = field.text().await.is_ok_and(|t| t == "true"),
            "file" => {
                let name = field.file_name().unwrap_or_default().to_owned();
                let zip = extension(&name) == "zip";
                if !zip && !wanted(&name) {
                    continue;
                }
                let target = if zip {
                    Some(staging.join(format!(".upload-{}.zip", taken.len())))
                } else {
                    slot(&staging, &mut taken, &name)
                };
                let Some(target) = target else { continue };
                if let Err(e) = write_field(field, &target, MAX_UPLOAD.saturating_sub(total)).await {
                    failure = Some(e);
                    break;
                }
                let size = tokio::fs::metadata(&target).await.map_or(0, |m| m.len());
                if zip {
                    let (archive, into, budget) = (target.clone(), staging.clone(), MAX_UPLOAD.saturating_sub(total));
                    let mut names = std::mem::take(&mut taken);
                    let unpacked = tokio::task::spawn_blocking(move || {
                        let result = unzip(&archive, &into, &mut names, budget);
                        let _ = std::fs::remove_file(&archive);
                        (result, names)
                    })
                    .await;
                    match unpacked {
                        Ok((Ok(written), names)) => {
                            taken = names;
                            total += written;
                        }
                        Ok((Err(e), _)) => {
                            failure = Some(e);
                            break;
                        }
                        Err(_) => {
                            failure = Some("Couldn't unpack the zip.".into());
                            break;
                        }
                    }
                } else {
                    total += size;
                }
            }
            _ => {}
        }
    }

    let outcome = match failure {
        Some(reason) => Err(reason),
        None if taken.is_empty() => {
            Err("No music was in the upload. delune takes audio files, pictures and zips of them.".into())
        }
        None => Ok(()),
    };
    if outcome.is_ok() {
        name_and_follow(&app, &job.id, &staging, (title, artist), follow.then_some(user.username.as_str())).await;
    }
    let failed = outcome.as_ref().err().cloned();
    crate::downloads::finish_external(&app, &job.id, outcome).await;
    if let Some(message) = failed {
        return error(StatusCode::BAD_REQUEST, "upload-failed", &message);
    }
    let job = app.downloads.list().into_iter().find(|j| j.id == job.id);
    job.map_or_else(|| StatusCode::NO_CONTENT.into_response(), |job| Json(job).into_response())
}

/// Name the upload after what its files say (unless told), and follow the album for
/// `follower` if asked.
async fn name_and_follow(
    app: &AppState,
    id: &str,
    staging: &Path,
    (title, artist): (Option<String>, Option<String>),
    follower: Option<&str>,
) {
    let dir = staging.to_path_buf();
    let (album, by) = tokio::task::spawn_blocking(move || tagged_release(&dir)).await.unwrap_or_default();
    let (title, artist) = (title.or(album), artist.or(by));
    app.downloads.retitle(id, title.as_deref().unwrap_or("Upload"), artist.as_deref());
    if let (Some(who), Some(album)) = (follower, title) {
        let (app, who) = (app.clone(), who.to_owned());
        tokio::spawn(async move {
            if let Err(message) = crate::automation::follow_album_for(&app, &who, artist.as_deref(), &album).await {
                tracing::info!(%album, %message, "couldn't follow an uploaded album");
            }
        });
    }
}

async fn write_field(mut field: axum::extract::multipart::Field<'_>, path: &Path, budget: u64) -> Result<(), String> {
    let mut file = tokio::fs::File::create(path).await.map_err(|e| format!("Couldn't store the upload: {e}"))?;
    let mut written = 0u64;
    while let Some(chunk) = field.chunk().await.map_err(|e| format!("The upload was interrupted: {e}"))? {
        written += chunk.len() as u64;
        if written > budget {
            return Err("That's more than delune takes in one upload (20 GB).".into());
        }
        file.write_all(&chunk).await.map_err(|e| format!("Couldn't store the upload: {e}"))?;
    }
    file.flush().await.map_err(|e| e.to_string())
}

/// Uploads arriving in pieces: one request per piece of each file, then a finish.
#[derive(Debug, Default)]
pub struct Sessions {
    inner: std::sync::Mutex<HashMap<String, Session>>,
}

#[derive(Debug)]
struct Session {
    owner: String,
    dir: PathBuf,
    /// Each file's name and how much of it has arrived, by index.
    files: Vec<(String, u64)>,
    touched: Instant,
    /// A piece is being written; another for the same session waits its turn.
    busy: bool,
}

impl Session {
    fn total(&self) -> u64 {
        self.files.iter().map(|(_, n)| n).sum()
    }
}

fn sessions_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("upload-sessions")
}

impl Sessions {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Forget sessions left idle, and their files.
    fn sweep(&self) {
        let stale: Vec<PathBuf> = {
            let mut sessions = self.lock();
            let old: Vec<String> = sessions
                .iter()
                .filter(|(_, s)| !s.busy && s.touched.elapsed() > SESSION_IDLE)
                .map(|(id, _)| id.clone())
                .collect();
            old.iter().filter_map(|id| sessions.remove(id)).map(|s| s.dir).collect()
        };
        for dir in stale {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Remove what an earlier run left in the sessions folder; sessions don't outlive it.
pub fn clear_sessions(data_dir: &Path) {
    let _ = std::fs::remove_dir_all(sessions_dir(data_dir));
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ChunkQuery {
    /// The file's name, as the person has it.
    name: String,
    /// Where this piece starts in the file.
    offset: u64,
}

/// `POST /api/v1/uploads/sessions`: start an upload sent in pieces.
#[utoipa::path(
    post,
    operation_id = "uploads_start",
    path = "/api/v1/uploads/sessions",
    tag = "downloads",
    responses(
        (status = 200, description = "Send pieces to this session", body = UploadSession),
        (status = 403, description = "Not allowed", body = ApiError),
        (status = 401, description = "Signed out", body = ApiError),
    ),
)]
pub async fn start(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.download, "add music") {
        return denied;
    }
    app.upload_sessions.sweep();
    let mut raw = [0u8; 16];
    if getrandom::fill(&mut raw).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "store-failed", "Couldn't start the upload.");
    }
    let id = hex::encode(raw);
    let dir = sessions_dir(&app.data_dir).join(&id);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        tracing::warn!(error = %e, "couldn't start an upload");
        return error(StatusCode::INTERNAL_SERVER_ERROR, "store-failed", "Couldn't store the upload.");
    }
    app.upload_sessions.lock().insert(
        id.clone(),
        Session { owner: user.username.clone(), dir, files: Vec::new(), touched: Instant::now(), busy: false },
    );
    Json(UploadSession { id, max_chunk: MAX_CHUNK }).into_response()
}

/// Clears a session's busy mark however the piece's request ends.
struct Busy<'a> {
    app: &'a AppState,
    id: &'a str,
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        if let Some(session) = self.app.upload_sessions.lock().get_mut(self.id) {
            session.busy = false;
            session.touched = Instant::now();
        }
    }
}

/// `PUT /api/v1/uploads/sessions/{id}/files/{index}`: one piece of file `index`,
/// starting at `offset`. A piece that doesn't start where the file ends is refused with
/// how much has arrived, so a retry can carry on from there.
#[utoipa::path(
    put,
    operation_id = "uploads_piece",
    path = "/api/v1/uploads/sessions/{id}/files/{index}",
    tag = "downloads",
    params(("id" = String, Path), ("index" = usize, Path), ChunkQuery),
    request_body(content_type = "application/octet-stream", description = "The bytes"),
    responses(
        (status = 200, description = "Stored", body = UploadReceived),
        (status = 409, description = "Not where the file ends; carry on from `received`", body = UploadReceived),
        (status = 404, description = "No such upload", body = ApiError),
        (status = 413, description = "Too big", body = ApiError),
    ),
)]
pub async fn piece(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath((id, index)): UrlPath<(String, usize)>,
    Query(query): Query<ChunkQuery>,
    body: Body,
) -> Response {
    let (path, received, total) = {
        let mut sessions = app.upload_sessions.lock();
        let Some(session) = sessions.get_mut(&id).filter(|s| s.owner == user.username) else {
            return error(StatusCode::NOT_FOUND, "no-upload", "That upload has ended. Start it again.");
        };
        if session.busy {
            return error(StatusCode::CONFLICT, "busy", "Another piece of this upload is still arriving.");
        }
        if index > session.files.len() || index >= MAX_SESSION_FILES {
            return error(StatusCode::BAD_REQUEST, "bad-index", "Files have to be sent in order.");
        }
        if index == session.files.len() {
            session.files.push((query.name.clone(), 0));
        }
        let received = session.files[index].1;
        if query.offset != received {
            return (StatusCode::CONFLICT, Json(UploadReceived { received })).into_response();
        }
        session.busy = true;
        (session.dir.join(index.to_string()), received, session.total())
    };
    let _busy = Busy { app: &app, id: &id };

    let file = tokio::fs::OpenOptions::new().create(true).append(true).open(&path).await;
    let mut file = match file {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!(error = %e, "couldn't store an upload piece");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "store-failed", "Couldn't store the upload.");
        }
    };
    // What's on disk is the truth: a piece cut off halfway still counts for what arrived.
    if file.metadata().await.map_or(0, |m| m.len()) != received {
        let _ = file.set_len(received).await;
    }
    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    let mut failure = None;
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            failure = Some(error(StatusCode::BAD_REQUEST, "interrupted", "The piece was cut off."));
            break;
        };
        written += chunk.len() as u64;
        if written > MAX_CHUNK || total + written > MAX_UPLOAD {
            failure = Some(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too-big",
                "That's more than delune takes in one upload (20 GB).",
            ));
            break;
        }
        if let Err(e) = file.write_all(&chunk).await {
            tracing::warn!(error = %e, "couldn't store an upload piece");
            failure = Some(error(StatusCode::INTERNAL_SERVER_ERROR, "store-failed", "Couldn't store the upload."));
            break;
        }
    }
    let _ = file.flush().await;
    let on_disk = file.metadata().await.map_or(received, |m| m.len());
    // Past the limit nothing is kept of the piece.
    let kept = if failure.as_ref().is_some_and(|r| r.status() == StatusCode::PAYLOAD_TOO_LARGE) {
        let _ = file.set_len(received).await;
        received
    } else {
        on_disk
    };
    if let Some(entry) = app.upload_sessions.lock().get_mut(&id).and_then(|s| s.files.get_mut(index)) {
        entry.1 = kept;
    }
    failure.unwrap_or_else(|| Json(UploadReceived { received: kept }).into_response())
}

/// `DELETE /api/v1/uploads/sessions/{id}`: give up on an upload.
#[utoipa::path(
    delete,
    operation_id = "uploads_abandon",
    path = "/api/v1/uploads/sessions/{id}",
    tag = "downloads",
    params(("id" = String, Path)),
    responses((status = 204, description = "Gone")),
)]
pub async fn abandon(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> StatusCode {
    let removed = {
        let mut sessions = app.upload_sessions.lock();
        match sessions.get(&id) {
            Some(s) if s.owner == user.username && !s.busy => sessions.remove(&id),
            _ => None,
        }
    };
    if let Some(session) = removed {
        let _ = tokio::fs::remove_dir_all(session.dir).await;
    }
    StatusCode::NO_CONTENT
}

/// `POST /api/v1/uploads/sessions/{id}/finish`: everything has arrived. The upload
/// becomes a download straight away and is checked in the background, so this answers
/// quickly however big it was.
#[utoipa::path(
    post,
    operation_id = "uploads_finish",
    path = "/api/v1/uploads/sessions/{id}/finish",
    tag = "downloads",
    params(("id" = String, Path)),
    request_body = FinishUpload,
    responses(
        (status = 200, description = "Added, and being checked", body = delune_core::api::DownloadJob),
        (status = 400, description = "Nothing usable was uploaded", body = ApiError),
        (status = 404, description = "No such upload", body = ApiError),
    ),
)]
pub async fn finish(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(id): UrlPath<String>,
    Json(request): Json<FinishUpload>,
) -> Response {
    let session = {
        let mut sessions = app.upload_sessions.lock();
        match sessions.get(&id) {
            Some(s) if s.owner == user.username && !s.busy => sessions.remove(&id),
            Some(s) if s.owner == user.username => {
                return error(StatusCode::CONFLICT, "busy", "A piece of this upload is still arriving.");
            }
            _ => None,
        }
    };
    let Some(session) = session else {
        return error(StatusCode::NOT_FOUND, "no-upload", "That upload has ended. Start it again.");
    };
    let usable = session.files.iter().any(|(name, n)| *n > 0 && (wanted(name) || extension(name) == "zip"));
    if !usable {
        let _ = tokio::fs::remove_dir_all(&session.dir).await;
        return error(
            StatusCode::BAD_REQUEST,
            "upload-failed",
            "No music was in the upload. delune takes audio files, pictures and zips of them.",
        );
    }

    let clean = |t: Option<String>| t.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    let (title, artist) = (clean(request.title), clean(request.artist));
    let (job, _cancel) = crate::downloads::begin_external(
        &app,
        "upload",
        title.as_deref().unwrap_or("Upload"),
        artist.as_deref(),
        &user.username,
    );
    let follower = request.follow.then(|| user.username.clone());
    let (app2, job_id) = (app.clone(), job.id.clone());
    tokio::spawn(async move {
        let staging = crate::downloads::staging_dir(&app2.data_dir, &job_id);
        let dir = session.dir.clone();
        let files = session.files;
        let outcome = tokio::task::spawn_blocking({
            let staging = staging.clone();
            move || gather(&dir, &files, &staging)
        })
        .await
        .unwrap_or_else(|_| Err("Couldn't unpack the upload.".into()));
        let _ = tokio::fs::remove_dir_all(&session.dir).await;
        if outcome.is_ok() {
            name_and_follow(&app2, &job_id, &staging, (title, artist), follower.as_deref()).await;
        }
        crate::downloads::finish_external(&app2, &job_id, outcome).await;
    });
    Json(job).into_response()
}

/// Move a session's files into `staging`, unpacking zips. Pictures and music only.
fn gather(dir: &Path, files: &[(String, u64)], staging: &Path) -> Result<(), String> {
    std::fs::create_dir_all(staging).map_err(|e| format!("Couldn't store the upload: {e}"))?;
    let mut taken = HashSet::new();
    let mut total = 0u64;
    for (index, (name, size)) in files.iter().enumerate() {
        let source = dir.join(index.to_string());
        if *size == 0 {
            continue;
        }
        if extension(name) == "zip" {
            total += unzip(&source, staging, &mut taken, MAX_UPLOAD.saturating_sub(total))?;
        } else if wanted(name) {
            let Some(target) = slot(staging, &mut taken, name) else { continue };
            std::fs::rename(&source, &target).map_err(|e| format!("Couldn't store the upload: {e}"))?;
            total += size;
        }
    }
    if taken.is_empty() {
        return Err("No music was in the upload. delune takes audio files, pictures and zips of them.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use axum::body::Body;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    #[test]
    fn names_stay_inside_and_unique() {
        let staging = Path::new("/staging");
        let mut taken = HashSet::new();
        assert_eq!(slot(staging, &mut taken, "../../etc/passwd.flac"), Some(staging.join("passwd.flac")));
        assert_eq!(slot(staging, &mut taken, r"CD1\01 Song.flac"), Some(staging.join("01 Song.flac")));
        assert_eq!(slot(staging, &mut taken, "cd2/01 song.flac"), Some(staging.join("01 song (2).flac")));
        assert_eq!(slot(staging, &mut taken, ".hidden"), None);
        assert_eq!(slot(staging, &mut taken, "Album/._01 Song.flac"), None);
        assert!(wanted("cover.JPG") && wanted("a.flac") && !wanted("notes.exe") && !wanted("x.cue"));
    }

    fn part(boundary: &str, name: &str, filename: Option<&str>, body: &[u8]) -> Vec<u8> {
        let mut out = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"").into_bytes();
        if let Some(filename) = filename {
            out.extend_from_slice(
                format!("; filename=\"{filename}\"\r\nContent-Type: application/octet-stream").as_bytes(),
            );
        }
        out.extend_from_slice(b"\r\n\r\n");
        out.extend_from_slice(body);
        out.extend_from_slice(b"\r\n");
        out
    }

    fn wav() -> Vec<u8> {
        let data = 44_100u32 * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&44_100u32.to_le_bytes());
        out.extend_from_slice(&88_200u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, data as usize));
        out
    }

    #[tokio::test]
    async fn uploads_become_downloads_waiting_for_review() {
        let root = std::env::temp_dir().join(format!("delune-upload-{}", std::process::id()));
        let accounts =
            std::sync::Arc::new(crate::accounts::Accounts::in_memory(Some("http://navidrome.invalid".into())));
        let (token, _) = accounts.signed_in("sam", false);
        let app = AppState { accounts, data_dir: root.join("data"), ..AppState::default() };
        let router = crate::router(app.clone());

        let zip = {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("Album/02 - Second.wav", options).unwrap();
            zip.write_all(&wav()).unwrap();
            zip.start_file("__MACOSX/Album/._02 - Second.wav", options).unwrap();
            zip.write_all(b"junk").unwrap();
            zip.start_file("Album/readme.txt", options).unwrap();
            zip.write_all(b"skip me").unwrap();
            zip.finish().unwrap().into_inner()
        };
        let boundary = "delune-boundary";
        let mut body = Vec::new();
        body.extend(part(boundary, "title", None, b"Twoism"));
        body.extend(part(boundary, "artist", None, b"Boards of Canada"));
        body.extend(part(boundary, "file", Some("01 - First.wav"), &wav()));
        body.extend(part(boundary, "file", Some("Album.zip"), &zip));
        body.extend(part(boundary, "file", Some("virus.exe"), b"no"));
        body.extend(format!("--{boundary}--\r\n").into_bytes());
        let request = axum::http::Request::post("/api/v1/uploads")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
        let job: delune_core::api::DownloadJob = serde_json::from_slice(&bytes).unwrap();
        assert_eq!((job.title.as_str(), job.parent.as_deref()), ("Twoism", Some("Boards of Canada")));
        let mut names: Vec<&str> = job.files.iter().map(|f| f.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["01 - First.wav", "02 - Second.wav"], "zips unpack flat; other files are left out");
        assert_eq!(job.status, delune_core::api::JobStatus::Ready);

        // An upload with nothing usable says so.
        let mut empty = part(boundary, "file", Some("notes.txt"), b"hi");
        empty.extend(format!("--{boundary}--\r\n").into_bytes());
        let request = axum::http::Request::post("/api/v1/uploads")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(empty))
            .unwrap();
        assert_eq!(router.oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn uploads_in_pieces_resume_and_finish() {
        let root = std::env::temp_dir().join(format!("delune-upload-pieces-{}", std::process::id()));
        let accounts =
            std::sync::Arc::new(crate::accounts::Accounts::in_memory(Some("http://navidrome.invalid".into())));
        let (token, _) = accounts.signed_in("sam", false);
        let (other, _) = accounts.signed_in("kim", false);
        let app = AppState { accounts, data_dir: root.join("data"), ..AppState::default() };
        let router = crate::router(app.clone());
        let call = |method: &str, uri: String, token: &str, body: Vec<u8>, json: bool| {
            let mut request = axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"));
            if json {
                request = request.header("content-type", "application/json");
            }
            let request = request.body(Body::from(body)).unwrap();
            let router = router.clone();
            async move {
                let response = router.oneshot(request).await.unwrap();
                let status = response.status();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                (status, serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default())
            }
        };

        let (status, session) = call("POST", "/api/v1/uploads/sessions".into(), &token, Vec::new(), false).await;
        assert_eq!(status, StatusCode::OK);
        let id = session["id"].as_str().unwrap().to_owned();
        let audio = wav();
        let (first, rest) = audio.split_at(1000);
        let url =
            |offset: usize| format!("/api/v1/uploads/sessions/{id}/files/0?name=01%20-%20First.wav&offset={offset}");

        let (status, got) = call("PUT", url(0), &token, first.to_vec(), false).await;
        assert_eq!((status, got["received"].as_u64()), (StatusCode::OK, Some(1000)));
        // A retry of the first piece is told where to carry on from.
        let (status, got) = call("PUT", url(0), &token, first.to_vec(), false).await;
        assert_eq!((status, got["received"].as_u64()), (StatusCode::CONFLICT, Some(1000)));
        // Someone else can't add to it.
        let (status, _) = call("PUT", url(1000), &other, rest.to_vec(), false).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, got) = call("PUT", url(1000), &token, rest.to_vec(), false).await;
        assert_eq!((status, got["received"].as_u64()), (StatusCode::OK, Some(audio.len() as u64)));
        // Files go in order.
        let skip = format!("/api/v1/uploads/sessions/{id}/files/5?name=x.flac&offset=0");
        assert_eq!(call("PUT", skip, &token, b"x".to_vec(), false).await.0, StatusCode::BAD_REQUEST);

        let finish = format!("/api/v1/uploads/sessions/{id}/finish");
        let body = br#"{"title":"Twoism","artist":"Boards of Canada"}"#.to_vec();
        let (status, job) = call("POST", finish.clone(), &token, body.clone(), true).await;
        assert_eq!(status, StatusCode::OK, "{job}");
        assert_eq!(job["title"], "Twoism");
        let id_of_job = job["id"].as_str().unwrap().to_owned();
        let mut ready = None;
        for _ in 0..100 {
            let found = app.downloads.list().into_iter().find(|j| j.id == id_of_job);
            if found.as_ref().is_some_and(|j| j.status != delune_core::api::JobStatus::Downloading) {
                ready = found;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let job = ready.expect("the upload was checked");
        assert_eq!(job.status, delune_core::api::JobStatus::Ready);
        assert_eq!(job.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["01 - First.wav"]);
        // A finished session is gone.
        assert_eq!(call("POST", finish, &token, body, true).await.0, StatusCode::NOT_FOUND);
        assert!(!sessions_dir(&app.data_dir).join(&id).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
