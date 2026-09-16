//! Per-person notifications: requests decided, downloads ready for review or failed,
//! albums added to the library, and (for people who manage delune) new requests.
//!
//! Kept in `<data dir>/notifications.json`, newest first, at most
//! a hundred per person. The web UI polls for them and shows an unread count.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use delune_core::api::{DownloadJob, JobStatus, Notification, NotificationKind, Notifications};
use serde::Deserialize;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

/// Notifications kept per person.
const KEEP: usize = 100;

#[derive(Debug, Default)]
pub struct Notifier {
    store: Option<Arc<Database>>,
    inboxes: Mutex<BTreeMap<String, Vec<Notification>>>,
    counter: Mutex<u64>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Notifier {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let inboxes = db.load("notifications").unwrap_or_default();
        Self { store: Some(db.clone()), inboxes: Mutex::new(inboxes), counter: Mutex::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<Notification>>> {
        self.inboxes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, inboxes: &BTreeMap<String, Vec<Notification>>) {
        if let Some(db) = &self.store {
            db.save("notifications", inboxes);
        }
    }

    /// Tell `username` about something.
    pub fn notify(&self, username: &str, kind: NotificationKind, title: String, detail: Option<String>, link: &str) {
        let id = {
            let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
            *counter += 1;
            format!("n{:x}{:04x}", now(), *counter & 0xffff)
        };
        let mut inboxes = self.lock();
        let inbox = inboxes.entry(username.to_owned()).or_default();
        inbox.insert(0, Notification { id, kind, at: now(), title, detail, link: Some(link.to_owned()), read: false });
        inbox.truncate(KEEP);
        self.save(&inboxes);
    }

    #[must_use]
    pub fn inbox(&self, username: &str) -> Notifications {
        let inboxes = self.lock();
        let items = inboxes.get(username).cloned().unwrap_or_default();
        let unread = u32::try_from(items.iter().filter(|n| !n.read).count()).unwrap_or(u32::MAX);
        Notifications { unread, items }
    }

    fn mark_read(&self, username: &str, ids: Option<&[String]>) {
        let mut inboxes = self.lock();
        if let Some(inbox) = inboxes.get_mut(username) {
            for n in inbox.iter_mut().filter(|n| ids.is_none_or(|ids| ids.contains(&n.id))) {
                n.read = true;
            }
        }
        self.save(&inboxes);
    }

    fn clear(&self, username: &str) {
        let mut inboxes = self.lock();
        inboxes.remove(username);
        self.save(&inboxes);
    }
}

/// Follow download status changes for as long as the server runs. Call once at startup,
/// before jobs resume.
pub fn start(app: &AppState) {
    let mut changes = app.downloads.status_changes();
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            match changes.recv().await {
                Ok((job, before)) => {
                    job_changed(&app, &job, before);
                    crate::requests::job_changed(&app, &job);
                    crate::wishlist::job_changed(&app, &job);
                    crate::events::changed(&app, crate::events::Topic::Downloads);
                    crate::events::changed(&app, crate::events::Topic::Notifications);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "missed some download status changes");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// A download changed status: tell whoever it was for when it's ready or failed.
pub fn job_changed(app: &AppState, job: &DownloadJob, before: JobStatus) {
    let Some(owner) = job.requested_by.as_deref() else { return };
    let name = match &job.parent {
        Some(artist) => format!("{} by {artist}", job.title),
        None => job.title.clone(),
    };
    match (before, job.status) {
        (JobStatus::Queued | JobStatus::Downloading, JobStatus::Ready) => app.notifications.notify(
            owner,
            NotificationKind::ReviewReady,
            format!("{name} is ready for review"),
            Some(format!("All {} files arrived from {}.", job.files.len(), job.username)),
            "/review",
        ),
        (JobStatus::Queued | JobStatus::Downloading, JobStatus::Failed) => app.notifications.notify(
            owner,
            NotificationKind::DownloadFailed,
            format!("{name} didn't finish downloading"),
            Some("Some files couldn't be downloaded. Resume it or find another copy.".into()),
            "/downloads",
        ),
        _ => {}
    }
}

/// `GET /api/v1/notifications`
#[utoipa::path(
    get,
    operation_id = "notifications_list",
    path = "/api/v1/notifications",
    tag = "notifications",
    responses(
        (status = 200, description = "OK", body = delune_core::api::Notifications),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn list(State(app): State<AppState>, user: CurrentUser) -> Json<Notifications> {
    Json(app.notifications.inbox(&user.username))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MarkRead {
    /// Which to mark; all of them when missing.
    #[serde(default)]
    ids: Option<Vec<String>>,
}

/// `POST /api/v1/notifications/read`
#[utoipa::path(
    post,
    operation_id = "notifications_read",
    path = "/api/v1/notifications/read",
    tag = "notifications",
    request_body = MarkRead,
    responses(
        (status = 200, description = "OK", body = delune_core::api::Notifications),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn read(State(app): State<AppState>, user: CurrentUser, Json(body): Json<MarkRead>) -> Json<Notifications> {
    app.notifications.mark_read(&user.username, body.ids.as_deref());
    crate::events::changed(&app, crate::events::Topic::Notifications);
    Json(app.notifications.inbox(&user.username))
}

/// `DELETE /api/v1/notifications`
#[utoipa::path(
    delete,
    operation_id = "notifications_clear",
    path = "/api/v1/notifications",
    tag = "notifications",
    responses(
        (status = 204, description = "Done"),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn clear(State(app): State<AppState>, user: CurrentUser) -> Response {
    app.notifications.clear(&user.username);
    crate::events::changed(&app, crate::events::Topic::Notifications);
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inboxes_are_per_person_capped_and_marked_read() {
        let notifier = Notifier::default();
        for i in 0..(KEEP + 5) {
            notifier.notify("sam", NotificationKind::ReviewReady, format!("album {i}"), None, "/review");
        }
        notifier.notify("alex", NotificationKind::RequestNew, "hello".into(), None, "/downloads");

        let sam = notifier.inbox("sam");
        assert_eq!((sam.items.len(), sam.unread), (KEEP, u32::try_from(KEEP).unwrap()));
        assert_eq!(sam.items[0].title, format!("album {}", KEEP + 4), "newest first");

        let first = sam.items[0].id.clone();
        notifier.mark_read("sam", Some(std::slice::from_ref(&first)));
        assert_eq!(notifier.inbox("sam").unread, u32::try_from(KEEP).unwrap() - 1);
        notifier.mark_read("sam", None);
        assert_eq!(notifier.inbox("sam").unread, 0);
        assert_eq!(notifier.inbox("alex").unread, 1);

        notifier.clear("sam");
        assert!(notifier.inbox("sam").items.is_empty());
    }
}
