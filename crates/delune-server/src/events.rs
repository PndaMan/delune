//! Telling clients what changed, as it changes.
//!
//! The web UI keeps a stream open to `/api/v1/events`. Whenever something it shows
//! changes — a download moves on, a request is decided, settings are saved — the
//! server names the topic and the client refreshes just that. Without this the UI
//! only caught up on its next poll, which is why things used to need a page refresh.

use std::convert::Infallible;

use axum::{
    extract::State,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::AppState;
use crate::accounts::CurrentUser;

/// What changed. Each matches what the web UI keeps under that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(utoipa::ToSchema)]
pub enum Topic {
    Downloads,
    Requests,
    Notifications,
    Wishlist,
    Follows,
    Sharing,
    People,
    Naming,
    Session,
    Soulseek,
    Chat,
    Favourites,
}

impl Topic {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Downloads => "downloads",
            Self::Requests => "requests",
            Self::Notifications => "notifications",
            Self::Wishlist => "wishlist",
            Self::Follows => "follows",
            Self::Sharing => "sharing",
            Self::People => "people",
            Self::Naming => "naming",
            Self::Session => "session",
            Self::Soulseek => "soulseek",
            Self::Chat => "chat",
            Self::Favourites => "favourites",
        }
    }
}

/// Everyone listening for changes.
#[derive(Debug)]
pub struct Changes(broadcast::Sender<Topic>);

impl Default for Changes {
    fn default() -> Self {
        Self(broadcast::channel(256).0)
    }
}

impl Changes {
    /// Say that `topic` changed. Cheap, and fine to call when nobody is listening.
    pub fn send(&self, topic: Topic) {
        let _ = self.0.send(topic);
    }

    fn subscribe(&self) -> broadcast::Receiver<Topic> {
        self.0.subscribe()
    }
}

/// Note a change for every client watching.
pub fn changed(app: &AppState, topic: Topic) {
    app.changes.send(topic);
}

/// `GET /api/v1/events`: topics that changed, as Server-Sent Events.
#[utoipa::path(
    get,
    operation_id = "events",
    path = "/api/v1/events",
    tag = "system",
    responses(
        (status = 200, description = "A stream of topic names", body = Topic, content_type = "text/event-stream"),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn stream(State(app): State<AppState>, _user: CurrentUser) -> Response {
    let changes = app.changes.subscribe();
    let events = futures_util::stream::unfold(changes, |mut changes| async move {
        match changes.recv().await {
            Ok(topic) => Some((Ok::<_, Infallible>(Event::default().data(topic.as_str())), changes)),
            // Behind after a burst: ask for everything rather than miss something.
            Err(broadcast::error::RecvError::Lagged(_)) => Some((Ok(Event::default().data("all")), changes)),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });
    Sse::new(events).keep_alive(KeepAlive::default()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listeners_hear_about_changes() {
        let changes = Changes::default();
        let mut listener = changes.subscribe();
        changes.send(Topic::Downloads);
        changes.send(Topic::Requests);
        assert_eq!(listener.recv().await.unwrap(), Topic::Downloads);
        assert_eq!(listener.recv().await.unwrap(), Topic::Requests);
        assert_eq!(Topic::Notifications.as_str(), "notifications");
    }
}
