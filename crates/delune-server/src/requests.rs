//! Requests: people ask for an album, and someone who manages delune approves or
//! declines it.
//!
//! Approving a request with a particular copy downloads it straight away;
//! otherwise it goes on the wishlist, which downloads a good enough copy when one
//! turns up. Either way the download belongs to the requester, so it shows up in
//! their Downloads and Review. The request's status follows that download until the
//! album is in the library. Kept in `<data dir>/requests.json`.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{
    ApiError, DownloadJob, JobStatus, MinQuality, MusicRequest, NewRequest, NotificationKind, RequestDecision,
    RequestStatus, WishlistRequest,
};

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

/// Requests kept, pending or not.
const MAX_REQUESTS: usize = 2000;

#[derive(Debug, Default)]
pub struct Requests {
    store: Option<Arc<Database>>,
    items: Mutex<Vec<MusicRequest>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Requests {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let items = db.load("requests").unwrap_or_default();
        Self { store: Some(db.clone()), items: Mutex::new(items) }
    }

    fn with<R>(&self, change: impl FnOnce(&mut Vec<MusicRequest>) -> R) -> R {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        let result = change(&mut items);
        if let Some(db) = &self.store {
            db.save("requests", &*items);
        }
        result
    }

    fn snapshot(&self) -> Vec<MusicRequest> {
        self.items.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn describe(request: &MusicRequest) -> String {
    match &request.artist {
        Some(artist) => format!("{} by {artist}", request.title),
        None => request.title.clone(),
    }
}

/// Where a download leaves the request that asked for it.
const fn status_for(job: JobStatus) -> RequestStatus {
    match job {
        JobStatus::Queued | JobStatus::Downloading => RequestStatus::Downloading,
        JobStatus::Ready => RequestStatus::Review,
        JobStatus::Imported => RequestStatus::Available,
        JobStatus::Failed | JobStatus::Cancelled => RequestStatus::Failed,
    }
}

/// A download changed: move along any request it belongs to.
pub fn job_changed(app: &AppState, job: &DownloadJob) {
    // A wishlist item started this download for an approved request.
    let from_wishlist = app.wishlist.started(&job.id);
    let now_available = app.requests.with(|items| {
        let mut available = Vec::new();
        for request in items.iter_mut() {
            let linked = request.download_id.as_deref() == Some(&job.id)
                || request.wishlist_id.as_ref().is_some_and(|w| from_wishlist.contains(w));
            if !linked || matches!(request.status, RequestStatus::Pending | RequestStatus::Declined) {
                continue;
            }
            request.download_id = Some(job.id.clone());
            let next = status_for(job.status);
            if next == RequestStatus::Available && request.status != RequestStatus::Available {
                available.push(request.clone());
            }
            request.status = next;
        }
        available
    });
    for request in now_available {
        app.notifications.notify(
            &request.requested_by,
            NotificationKind::Imported,
            format!("{} is in the library", describe(&request)),
            Some("Your request is ready to play in Navidrome.".into()),
            "/review",
        );
    }
}

/// Whether download `job_id` was started for a request.
#[must_use]
pub fn asked_for(app: &AppState, job_id: &str) -> bool {
    app.requests.snapshot().iter().any(|r| r.download_id.as_deref() == Some(job_id))
}

/// `GET /api/v1/requests`: your requests, or everyone's if you manage delune.
pub async fn list(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<MusicRequest>> {
    let mut items: Vec<MusicRequest> =
        app.requests.snapshot().into_iter().filter(|r| user.can_see(Some(&r.requested_by))).collect();
    // Pending first, then newest.
    items.sort_by(|a, b| {
        (b.status == RequestStatus::Pending)
            .cmp(&(a.status == RequestStatus::Pending))
            .then(b.requested_at.cmp(&a.requested_at))
    });
    Json(items)
}

/// `POST /api/v1/requests`
pub async fn create(State(app): State<AppState>, user: CurrentUser, Json(new): Json<NewRequest>) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.request, "request albums") {
        return denied;
    }
    let title = new.title.trim().to_owned();
    let query = new.query.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() || query.chars().count() < 3 {
        return error(StatusCode::BAD_REQUEST, "request-too-vague", "Say which album you'd like.");
    }
    if let Some(download) = &new.download
        && (download.files.is_empty() || download.username.trim().is_empty())
    {
        return error(StatusCode::BAD_REQUEST, "no-files", "Choose at least one file to request.");
    }
    let note = new.note.map(|n| n.trim().chars().take(500).collect::<String>()).filter(|n| !n.is_empty());

    let created = app.requests.with(|items| {
        // Asking twice for the same thing, while it's still open, is one request.
        let open = |r: &&mut MusicRequest| {
            r.requested_by == user.username
                && r.query.eq_ignore_ascii_case(&query)
                && matches!(r.status, RequestStatus::Pending | RequestStatus::Searching | RequestStatus::Downloading)
        };
        if let Some(existing) = items.iter_mut().find(open) {
            return Some((false, existing.clone()));
        }
        if items.len() >= MAX_REQUESTS {
            // Forget the oldest finished requests first.
            let oldest = items
                .iter()
                .enumerate()
                .filter(|(_, r)| !matches!(r.status, RequestStatus::Pending))
                .min_by_key(|(_, r)| r.requested_at)
                .map(|(i, _)| i)?;
            items.remove(oldest);
        }
        let request = MusicRequest {
            id: format!("r{:x}{:04x}", now(), items.len() & 0xffff),
            requested_by: user.username.clone(),
            requested_at: now(),
            title,
            artist: new.artist.map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()),
            query,
            link: new.link,
            download: new.download,
            quality_label: new.quality_label,
            note,
            status: RequestStatus::Pending,
            decided_by: None,
            decided_at: None,
            reason: None,
            download_id: None,
            wishlist_id: None,
        };
        items.push(request.clone());
        Some((true, request))
    });
    let Some((fresh, request)) = created else {
        return error(StatusCode::CONFLICT, "too-many-requests", "There are too many open requests.");
    };
    if !fresh {
        return Json(request).into_response();
    }
    tracing::info!(by = %user.username, title = %request.title, "album requested");

    // Someone who can approve it themselves doesn't need telling.
    for manager in app.accounts.managers() {
        if manager != user.username {
            app.notifications.notify(
                &manager,
                NotificationKind::RequestNew,
                format!("{} asked for {}", user.username, describe(&request)),
                request.note.clone(),
                "/downloads",
            );
        }
    }
    (StatusCode::CREATED, Json(request)).into_response()
}

/// `POST /api/v1/requests/{id}/decision`: approve or decline.
pub async fn decide(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(id): UrlPath<String>,
    Json(decision): Json<RequestDecision>,
) -> Response {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "approve requests") {
        return denied;
    }
    let Some(request) = app.requests.snapshot().into_iter().find(|r| r.id == id) else {
        return error(StatusCode::NOT_FOUND, "no-such-request", "That request doesn't exist.");
    };
    if request.status != RequestStatus::Pending {
        return error(StatusCode::CONFLICT, "already-decided", "Someone has already dealt with that request.");
    }

    let reason = decision.reason.map(|r| r.trim().chars().take(500).collect::<String>()).filter(|r| !r.is_empty());
    let (status, download_id, wishlist_id) = if !decision.approve {
        (RequestStatus::Declined, None, None)
    } else if let Some(download) = request.download.clone() {
        match crate::downloads::begin(&app, download, &request.requested_by) {
            Ok(job) => (status_for(job.status), Some(job.id), None),
            Err((status, code, message)) => return error(status, code, &message),
        }
    } else {
        let wish = WishlistRequest {
            query: request.query.clone(),
            track: None,
            playlist: None,
            auto_download: true,
            min_quality: MinQuality::Any,
        };
        match crate::wishlist::insert_for(&app, &request.requested_by, wish) {
            Ok(item) => (RequestStatus::Searching, None, Some(item.id)),
            Err((status, code, message)) => return error(status, code, message),
        }
    };

    let decided = app.requests.with(|items| {
        let request = items.iter_mut().find(|r| r.id == id)?;
        request.status = status;
        request.decided_by = Some(user.username.clone());
        request.decided_at = Some(now());
        request.reason.clone_from(&reason);
        request.download_id = download_id;
        request.wishlist_id = wishlist_id;
        Some(request.clone())
    });
    let Some(request) = decided else {
        return error(StatusCode::NOT_FOUND, "no-such-request", "That request was removed.");
    };
    tracing::info!(by = %user.username, title = %request.title, approved = decision.approve, "request decided");

    if request.requested_by != user.username {
        let (kind, title, detail) = if decision.approve {
            let detail = if request.download_id.is_some() {
                "It's downloading now."
            } else {
                "delune is looking for a good copy and will download it when one turns up."
            };
            (
                NotificationKind::RequestApproved,
                format!("{} approved your request for {}", user.username, describe(&request)),
                Some(detail.to_owned()),
            )
        } else {
            (
                NotificationKind::RequestDeclined,
                format!("{} declined your request for {}", user.username, describe(&request)),
                reason,
            )
        };
        app.notifications.notify(&request.requested_by, kind, title, detail, "/downloads");
    }
    Json(request).into_response()
}

/// `DELETE /api/v1/requests/{id}`: withdraw your own request, or remove any if you manage.
pub async fn remove(State(app): State<AppState>, user: CurrentUser, UrlPath(id): UrlPath<String>) -> Response {
    let removed = app.requests.with(|items| {
        let index = items.iter().position(|r| r.id == id && user.can_see(Some(&r.requested_by)))?;
        Some(items.remove(index))
    });
    match removed {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => error(StatusCode::NOT_FOUND, "no-such-request", "That request doesn't exist."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_move_requests_along() {
        assert_eq!(status_for(JobStatus::Queued), RequestStatus::Downloading);
        assert_eq!(status_for(JobStatus::Ready), RequestStatus::Review);
        assert_eq!(status_for(JobStatus::Imported), RequestStatus::Available);
        assert_eq!(status_for(JobStatus::Cancelled), RequestStatus::Failed);
    }
}
