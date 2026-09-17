//! Checking and tidying the library: albums split over several folders, tracks in an
//! album twice, and the trash that fixes (and replaced tracks) go to.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{
    ApiError, HealthFinding, HealthFixRequest, HealthFixed, HealthIgnoreRequest, HealthKind, LibraryHealth, TrashBatch,
    TrashRestored,
};
use delune_library::health;

use crate::AppState;
use crate::accounts::CurrentUser;

/// How long replaced and removed files wait in the trash.
pub const TRASH_DAYS: u32 = 30;

#[must_use]
pub fn trash_age() -> Duration {
    Duration::from_secs(u64::from(TRASH_DAYS) * 24 * 60 * 60)
}

/// The last check, so a fix acts on exactly what someone was shown.
#[derive(Debug, Default)]
pub struct LastScan(std::sync::Mutex<Option<health::Scan>>);

impl LastScan {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<health::Scan>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Where ignored findings are kept.
const IGNORED: &str = "library-ignored";

fn ignored(app: &AppState) -> std::collections::BTreeSet<String> {
    app.db.load(IGNORED).unwrap_or_default()
}

fn finding(f: &health::Finding) -> HealthFinding {
    HealthFinding {
        id: f.id.clone(),
        key: f.key.clone(),
        kind: match f.kind {
            health::Kind::SplitAlbum => HealthKind::SplitAlbum,
            health::Kind::DuplicateTracks => HealthKind::DuplicateTracks,
            health::Kind::MixedAlbum => HealthKind::MixedAlbum,
        },
        folders: f.folders.clone(),
        duplicates: f.duplicates.clone(),
        files: count(f.files),
        album: f.album.as_ref().map(|a| a.album.clone()),
        album_artist: f.album.as_ref().and_then(|a| a.album_artist.clone()),
        retag: f.album.as_ref().map_or(0, |a| count(a.retag.len())),
        strays: f.album.as_ref().map(|a| a.strays.clone()).unwrap_or_default(),
    }
}

fn trash(root: &std::path::Path) -> Vec<TrashBatch> {
    delune_library::trash::batches(root)
        .into_iter()
        .map(|b| TrashBatch {
            id: b.id,
            created_at: b.created_at,
            label: b.label,
            changes: b
                .changes
                .into_iter()
                .map(|c| delune_core::api::TrashChange { kind: c.kind, path: c.path, to: c.to })
                .collect(),
        })
        .collect()
}

/// `GET /api/v1/library/health`: look the library over.
#[utoipa::path(
    get,
    operation_id = "library_health",
    path = "/api/v1/library/health",
    tag = "library",
    responses(
        (status = 200, description = "What was found", body = LibraryHealth),
        (status = 409, description = "No library folder", body = ApiError),
        (status = 403, description = "Not allowed", body = ApiError),
    ),
)]
pub async fn check(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "check the library") {
        return denied;
    }
    let Some(root) = app.library.library_dir.clone() else {
        return error(StatusCode::CONFLICT, "no-library", "No library folder is configured.");
    };
    let scanned = tokio::task::spawn_blocking(move || {
        let scan = health::scan(&root);
        let trash = trash(&root);
        (scan, trash)
    })
    .await;
    let Ok((scan, trash)) = scanned else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "check-failed", "The check stopped unexpectedly.");
    };
    let skip = ignored(&app);
    let shown: Vec<HealthFinding> = scan.findings.iter().filter(|f| !skip.contains(&f.key)).map(finding).collect();
    let report = LibraryHealth {
        albums: count(scan.albums),
        tracks: count(scan.tracks),
        ignored: count(scan.findings.len() - shown.len()),
        findings: shown,
        trash,
        trash_days: TRASH_DAYS,
    };
    *app.library_health.lock() = Some(scan);
    Json(report).into_response()
}

/// `POST /api/v1/library/health/fix`: fix one finding from the last check. Files that
/// go away move to the library's trash; the answer names the batch that undoes it.
#[utoipa::path(
    post,
    operation_id = "library_health_fix",
    path = "/api/v1/library/health/fix",
    tag = "library",
    request_body = HealthFixRequest,
    responses(
        (status = 200, description = "Fixed", body = HealthFixed),
        (status = 409, description = "The library changed since the check", body = ApiError),
        (status = 403, description = "Not allowed", body = ApiError),
    ),
)]
pub async fn fix(State(app): State<AppState>, user: CurrentUser, Json(request): Json<HealthFixRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "tidy the library") {
        return denied;
    }
    let Some(root) = app.library.library_dir.clone() else {
        return error(StatusCode::CONFLICT, "no-library", "No library folder is configured.");
    };
    let found =
        app.library_health.lock().as_ref().and_then(|scan| scan.findings.iter().find(|f| f.id == request.id).cloned());
    let Some(found) = found else {
        return error(StatusCode::CONFLICT, "changed", "That's not in the latest check. Check again first.");
    };
    let kept = root.join(&found.folders[0]);
    let result = tokio::task::spawn_blocking(move || health::fix(&root, &found)).await;
    let fixed = match result {
        Ok(Ok(fixed)) => fixed,
        Ok(Err(health::FixError::Io(e))) => {
            tracing::warn!(error = %e, "a library fix failed and was undone");
            return error(StatusCode::CONFLICT, "fix-failed", &format!("That didn't work, and nothing changed: {e}."));
        }
        Ok(Err(e)) => return error(StatusCode::CONFLICT, "changed", &e.to_string()),
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "fix-failed", "The fix stopped unexpectedly."),
    };
    if let Some(scan) = app.library_health.lock().as_mut() {
        scan.findings.retain(|f| f.id != request.id);
    }
    tracing::info!(by = %user.username, moved = fixed.moved, trashed = fixed.trashed, batch = %fixed.batch, "tidied the library");
    // The album that's left gets artwork, whichever copy had it.
    let app2 = app.clone();
    tokio::spawn(async move {
        crate::finishing::cover_folder(&app2, &kept).await;
        after_change(&app2);
    });
    Json(HealthFixed {
        moved: count(fixed.moved),
        trashed: count(fixed.trashed),
        retagged: count(fixed.retagged),
        batch: fixed.batch,
    })
    .into_response()
}

/// `POST /api/v1/library/health/ignore`: stop showing a finding, for good.
#[utoipa::path(
    post,
    operation_id = "library_health_ignore",
    path = "/api/v1/library/health/ignore",
    tag = "library",
    request_body = HealthIgnoreRequest,
    responses(
        (status = 204, description = "Ignored"),
        (status = 403, description = "Not allowed", body = ApiError),
    ),
)]
pub async fn ignore(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(request): Json<HealthIgnoreRequest>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "tidy the library") {
        return denied;
    }
    let mut keys = ignored(&app);
    if !request.key.is_empty() && keys.len() < 10_000 {
        keys.insert(request.key);
    }
    app.db.save(IGNORED, &keys);
    StatusCode::NO_CONTENT.into_response()
}

/// `DELETE /api/v1/library/health/ignore`: show every ignored finding again.
#[utoipa::path(
    delete,
    operation_id = "library_health_unignore",
    path = "/api/v1/library/health/ignore",
    tag = "library",
    responses(
        (status = 204, description = "Nothing is ignored now"),
        (status = 403, description = "Not allowed", body = ApiError),
    ),
)]
pub async fn unignore(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "tidy the library") {
        return denied;
    }
    app.db.save(IGNORED, &std::collections::BTreeSet::<String>::new());
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/v1/library/trash/{id}/restore`: put a batch back.
#[utoipa::path(
    post,
    operation_id = "library_trash_restore",
    path = "/api/v1/library/trash/{id}/restore",
    tag = "library",
    params(("id" = String, Path)),
    responses(
        (status = 200, description = "Put back", body = TrashRestored),
        (status = 403, description = "Not allowed", body = ApiError),
    ),
)]
pub async fn restore(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "restore from the trash") {
        return denied;
    }
    let Some(root) = app.library.library_dir.clone() else {
        return error(StatusCode::CONFLICT, "no-library", "No library folder is configured.");
    };
    let result = tokio::task::spawn_blocking(move || delune_library::tidy::undo(&root, &id)).await;
    match result {
        Ok(Ok(left)) => {
            *app.library_health.lock() = None;
            after_change(&app);
            Json(TrashRestored { left }).into_response()
        }
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, "restore-failed", &e.to_string()),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "restore-failed", "Restoring stopped unexpectedly."),
    }
}

fn after_change(app: &AppState) {
    app.library_cache.clear();
    crate::sharing::refresh(app);
    let app = app.clone();
    tokio::spawn(async move { crate::finishing::rescan(&app).await });
}
