//! Review and import.
//!
//! When a download job finishes, [`check`] inspects and verifies every staged file
//! and plans where each will land in the library. The review screen shows that
//! report; approving it calls [`import()`], which moves the files and asks Navidrome
//! to rescan. Nothing reaches the library any other way (ADR 0005).

use std::path::{Path, PathBuf};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{ApiError, ImportResult, JobStatus, ReviewReport, ReviewState, ReviewTrack};
use delune_library::import::{self, Plan, ReleaseContext, StagedTrack};
use delune_library::{NamingOptions, Template, inspect, verify};

use crate::AppState;
use crate::accounts::CurrentUser;

/// Import settings, from the server configuration.
#[derive(Debug, Clone)]
pub struct LibrarySettings {
    /// The music folder Navidrome scans. Without it, reviews work but importing doesn't.
    pub library_dir: Option<PathBuf>,
    pub template: Template,
    pub options: NamingOptions,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            library_dir: None,
            template: Template::parse(DEFAULT_TEMPLATE).expect("default template is valid"),
            options: NamingOptions::default(),
        }
    }
}

pub const DEFAULT_TEMPLATE: &str = "{album_artist}/[{year} - ]{album}/{track} - {title}";

/// The outcome of checking a job: the report shown to people, and the plan used
/// if they approve it.
#[derive(Debug, Clone)]
pub struct Checked {
    pub report: ReviewReport,
    pub plan: Plan,
}

const AUDIO: &[&str] = &["flac", "alac", "wav", "aif", "aiff", "mp3", "m4a", "aac", "opus", "ogg", "oga"];
const IMAGES: &[&str] = &["jpg", "jpeg", "png", "webp"];

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// Inspect, verify and plan the files in `staging`. CPU- and disk-heavy; call it
/// from a blocking thread.
pub fn check(staging: &Path, context: &ReleaseContext, settings: &LibrarySettings) -> std::io::Result<Checked> {
    let mut audio: Vec<PathBuf> = Vec::new();
    let mut images: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(staging)? {
        let path = entry?.path();
        let ext = extension(&path);
        if AUDIO.contains(&ext.as_str()) {
            audio.push(path);
        } else if IMAGES.contains(&ext.as_str()) {
            images.push(path);
        }
    }
    audio.sort();

    let mut staged = Vec::new();
    let mut findings = Vec::new();
    let mut unreadable = Vec::new();
    for path in &audio {
        match examine(path) {
            Ok((track, verification, problem)) => {
                staged.push(track);
                findings.push((verification, problem));
            }
            Err(message) => unreadable.push(format!(
                "{} couldn't be read and won't be imported ({message}).",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }

    let plan = import::plan(&staged, &images, context, &settings.template, &settings.options);
    let conflicts = match &settings.library_dir {
        Some(root) => import::conflicts(&plan, root)
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.strip_prefix(root).unwrap_or(&p).display().to_string())
            .collect(),
        None => Vec::new(),
    };

    let tracks: Vec<ReviewTrack> = plan
        .tracks
        .iter()
        .zip(&staged)
        .zip(&findings)
        .map(|((planned, track), (verification, problem))| ReviewTrack {
            file: track.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            destination: planned.destination.clone(),
            title: planned.fields.title.clone(),
            artist: planned.fields.artist.clone(),
            track: planned.fields.track,
            disc: planned.fields.disc,
            quality: Some(track.info.quality),
            quality_label: Some(track.info.quality.to_string()),
            duration_secs: Some(track.info.duration_secs),
            cutoff_hz: verification.as_ref().and_then(|v| v.cutoff_hz),
            suspect_transcode: verification.as_ref().is_some_and(|v| v.suspect_transcode),
            problem: problem.clone(),
        })
        .collect();

    let mut warnings = plan.warnings.clone();
    warnings.extend(unreadable);

    let blocked_reason = blocked_reason(settings, &tracks, &conflicts);

    let first = plan.tracks.first().map(|t| &t.fields);
    let report = ReviewReport {
        album_artist: first.map(|f| f.album_artist.clone()).unwrap_or_default(),
        album: first.map_or_else(|| context.album.clone(), |f| f.album.clone()),
        year: first.and_then(|f| f.year),
        cover: plan.cover.as_ref().map(|(_, destination)| destination.clone()),
        tracks,
        warnings,
        conflicts,
        library_dir: settings.library_dir.as_ref().map(|p| p.display().to_string()),
        blocked_reason,
    };
    Ok(Checked { report, plan })
}

/// Inspect and verify one audio file, describing anything wrong with it.
fn examine(path: &Path) -> Result<(StagedTrack, Option<verify::Verification>, Option<String>), String> {
    let info = inspect::inspect(path).map_err(|e| e.to_string())?;
    let verification = verify::verify(path, info.quality.codec.is_lossless());
    let problem = match &verification {
        Err(error) => Some(format!("Won't play: {error}")),
        Ok(v) if v.decode_errors > 0 => {
            Some(format!("{} damaged sections; this copy will skip or click", v.decode_errors))
        }
        Ok(v) if f64::from(info.duration_secs) - v.decoded_secs > 2.0 => {
            Some("The file is shorter than its header says; the download may be incomplete".into())
        }
        Ok(v) if v.suspect_transcode => Some(format!(
            "Sound stops at {} kHz, which usually means it was converted from a lossy file",
            v.cutoff_hz.unwrap_or(0) / 1000
        )),
        Ok(_) => None,
    };
    Ok((StagedTrack { path: path.to_owned(), info }, verification.ok(), problem))
}

fn blocked_reason(settings: &LibrarySettings, tracks: &[ReviewTrack], conflicts: &[String]) -> Option<String> {
    let unplayable =
        tracks.iter().filter(|t| t.problem.as_deref().is_some_and(|p| p.starts_with("Won't play"))).count();
    if settings.library_dir.is_none() {
        Some(
            "No library folder is configured. Start the server with DELUNE_LIBRARY_DIR set to your music folder."
                .into(),
        )
    } else if tracks.is_empty() {
        Some("There are no playable audio files to import.".into())
    } else if !conflicts.is_empty() {
        Some(format!("{} of these files already exist in your library.", conflicts.len()))
    } else if unplayable > 0 {
        Some(format!("{unplayable} file(s) won't play. Download a different copy instead."))
    } else {
        None
    }
}

/// `GET /api/v1/downloads/{id}/review`
pub async fn report(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    if !app.downloads.owner(&id).is_some_and(|owner| user.can_see(owner.as_deref())) {
        return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
    }
    let Some((status, review, checked)) = app.downloads.review(&id) else {
        return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
    };
    match (review, checked) {
        (ReviewState::Ready, Some(checked)) => Json(checked.report).into_response(),
        (ReviewState::Failed, _) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, "review-failed", "The files couldn't be checked.")
        }
        _ if status == JobStatus::Imported => {
            error(StatusCode::GONE, "already-imported", "This release is already in your library.")
        }
        _ => error(StatusCode::ACCEPTED, "not-ready", "Still checking the files."),
    }
}

/// `POST /api/v1/downloads/{id}/import`
pub async fn import(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let Some(owner) = app.downloads.owner(&id).filter(|owner| user.can_see(owner.as_deref())) else {
        return error(StatusCode::NOT_FOUND, "no-such-download", "That download doesn't exist.");
    };
    let own = owner.as_deref() == Some(user.username.as_str());
    if !(user.permissions.manage || (own && user.can_import)) {
        return error(
            StatusCode::FORBIDDEN,
            "needs-approval",
            "An admin needs to approve this before it goes into the library.",
        );
    }
    let Some((status, _, Some(checked))) = app.downloads.review(&id) else {
        return error(StatusCode::CONFLICT, "not-ready", "This download isn't ready for import yet.");
    };
    if status != JobStatus::Ready {
        return error(StatusCode::CONFLICT, "not-ready", "Only finished downloads can be imported.");
    }
    if let Some(reason) = &checked.report.blocked_reason {
        return error(StatusCode::CONFLICT, "blocked", reason);
    }
    let Some(root) = app.library.library_dir.clone() else {
        return error(StatusCode::CONFLICT, "no-library", "No library folder is configured.");
    };

    let plan = checked.plan.clone();
    let result = tokio::task::spawn_blocking({
        let root = root.clone();
        move || import::execute(&plan, &root)
    })
    .await;
    let imported = match result {
        Ok(Ok(imported)) => imported,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, "import-failed", &format!("Import stopped: {e}.")),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "import-failed", "Import stopped unexpectedly."),
    };

    app.library_cache.clear();
    // Share what just arrived.
    crate::sharing::refresh(&app);
    let staging = crate::downloads::staging_dir(&app.data_dir, &id);
    let _ = tokio::fs::remove_dir_all(&staging).await;

    let folder = checked
        .plan
        .tracks
        .first()
        .and_then(|t| t.destination.rsplit_once('/').map(|(dir, _)| dir.to_owned()))
        .unwrap_or_default();
    app.downloads.mark_imported(&id, &folder);
    let scan_started = match &app.navidrome {
        Some(navidrome) => match navidrome.start_scan(false).await {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "imported, but Navidrome didn't start a scan");
                false
            }
        },
        None => false,
    };
    tracing::info!(%id, files = imported.len(), %folder, scan_started, "imported into the library");
    Json(ImportResult { imported: u32::try_from(imported.len()).unwrap_or(u32::MAX), folder, scan_started })
        .into_response()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}
