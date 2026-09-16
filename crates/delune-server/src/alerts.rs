//! Notifications that reach people outside delune: push on their phones and browsers,
//! an ntfy topic, or a Discord channel.
//!
//! Each person chooses their own channels and which kinds of notification stay in
//! delune only. Everything [`crate::notifications`] records is handed here and sent on
//! in the background, so a slow webhook never holds anything up.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use delune_core::api::{
    AlertDevice, AlertSettings, AlertSettingsUpdate, AlertTestResult, ApiError, Notification, NotificationKind,
    PushDevice,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;
use crate::webpush::{self, Subscription, Vapid};

/// Push subscriptions kept per person.
const MAX_DEVICES: usize = 20;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Channels {
    #[serde(default)]
    ntfy: Option<String>,
    #[serde(default)]
    discord: Option<String>,
    #[serde(default)]
    muted: Vec<NotificationKind>,
    #[serde(default)]
    devices: Vec<Device>,
    /// Where delune was opened from when these were saved, for links in messages.
    #[serde(default)]
    origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Device {
    id: String,
    label: String,
    added_at: u64,
    subscription: Subscription,
}

/// Something to send, for someone.
#[derive(Debug, Clone)]
struct Alert {
    username: String,
    kind: Option<NotificationKind>,
    title: String,
    detail: Option<String>,
    link: Option<String>,
}

#[derive(Debug, Default)]
pub struct Alerts {
    store: Option<Arc<Database>>,
    channels: Mutex<BTreeMap<String, Channels>>,
    vapid: OnceLock<Option<Vapid>>,
    outbox: OnceLock<mpsc::UnboundedSender<Alert>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

impl Alerts {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let channels = db.load("alert-channels").unwrap_or_default();
        Self { store: Some(db.clone()), channels: Mutex::new(channels), ..Self::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Channels>> {
        self.channels.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, channels: &BTreeMap<String, Channels>) {
        if let Some(db) = &self.store {
            db.save("alert-channels", channels);
        }
    }

    /// delune's push key pair, made the first time it's needed and kept.
    fn vapid(&self) -> Option<&Vapid> {
        self.vapid
            .get_or_init(|| {
                let engine = base64::engine::general_purpose::STANDARD;
                let saved: Option<String> = self.store.as_ref().and_then(|db| db.load("vapid-key"));
                if let Some(pair) = saved.and_then(|s| engine.decode(s).ok()).and_then(|b| Vapid::from_pkcs8(&b).ok()) {
                    return Some(pair);
                }
                let (pair, pkcs8) = Vapid::generate().ok()?;
                if let Some(db) = &self.store {
                    db.save("vapid-key", &engine.encode(pkcs8));
                }
                Some(pair)
            })
            .as_ref()
    }

    /// Pass a notification on to `username`'s channels.
    pub fn send(&self, username: &str, notification: &Notification) {
        if let Some(outbox) = self.outbox.get() {
            let _ = outbox.send(Alert {
                username: username.to_owned(),
                kind: Some(notification.kind),
                title: notification.title.clone(),
                detail: notification.detail.clone(),
                link: notification.link.clone(),
            });
        }
    }

    fn settings(&self, username: &str) -> AlertSettings {
        let channels = self.lock().get(username).cloned().unwrap_or_default();
        AlertSettings {
            ntfy: channels.ntfy,
            discord: channels.discord,
            muted: channels.muted,
            devices: channels
                .devices
                .iter()
                .map(|d| AlertDevice { id: d.id.clone(), label: d.label.clone(), added_at: d.added_at })
                .collect(),
            push_key: self.vapid().map(Vapid::public_key),
        }
    }
}

/// Start delivering. Call once at startup.
pub fn start(app: &AppState) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Alert>();
    if app.alerts.outbox.set(tx).is_err() {
        return;
    }
    let app = app.clone();
    tokio::spawn(async move {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("delune/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        while let Some(alert) = rx.recv().await {
            deliver(&app, &http, &alert).await;
        }
    });
}

fn absolute(origin: Option<&str>, link: Option<&str>) -> Option<String> {
    let origin = origin?;
    let link = link.unwrap_or("/");
    Some(format!("{}{}", origin.trim_end_matches('/'), if link.starts_with('/') { link } else { "/" }))
}

/// Send one alert everywhere its person wants it, and report how each went.
async fn deliver(app: &AppState, http: &reqwest::Client, alert: &Alert) -> Vec<AlertTestResult> {
    let Some(channels) = app.alerts.lock().get(&alert.username).cloned() else { return Vec::new() };
    if alert.kind.is_some_and(|k| channels.muted.contains(&k)) {
        return Vec::new();
    }
    let mut results = Vec::new();
    let url = absolute(channels.origin.as_deref(), alert.link.as_deref());

    if !channels.devices.is_empty()
        && let Some(vapid) = app.alerts.vapid()
    {
        let payload = serde_json::json!({
            "title": alert.title,
            "body": alert.detail,
            "url": alert.link.clone().unwrap_or_else(|| "/".into()),
            "tag": alert.kind.map(|k| format!("{k:?}")),
        })
        .to_string();
        let mut gone = Vec::new();
        for device in &channels.devices {
            let outcome = webpush::send(http, vapid, &device.subscription, payload.as_bytes()).await;
            if outcome == webpush::Outcome::Gone {
                gone.push(device.id.clone());
            }
            results.push(AlertTestResult {
                channel: format!("Push: {}", device.label),
                ok: outcome == webpush::Outcome::Delivered,
                message: match outcome {
                    webpush::Outcome::Delivered => "Sent".into(),
                    webpush::Outcome::Gone => "This device unsubscribed, so it was removed".into(),
                    webpush::Outcome::Failed => "The push service refused it".into(),
                },
            });
        }
        if !gone.is_empty() {
            let mut all = app.alerts.lock();
            if let Some(mine) = all.get_mut(&alert.username) {
                mine.devices.retain(|d| !gone.contains(&d.id));
            }
            app.alerts.save(&all);
        }
    }

    if let Some(topic) = &channels.ntfy {
        let mut request = http
            .post(topic)
            .header("Title", alert.title.clone())
            .header("Tags", "notes")
            .body(alert.detail.clone().unwrap_or_else(|| alert.title.clone()));
        if let Some(url) = &url {
            request = request.header("Click", url.clone());
        }
        results.push(outcome("ntfy", request.send().await));
    }

    if let Some(webhook) = &channels.discord {
        let body = serde_json::json!({
            "username": "delune",
            "embeds": [{
                "title": alert.title,
                "description": alert.detail,
                "url": url,
                "color": 0x00ab_9dff,
            }],
        });
        results.push(outcome("Discord", http.post(webhook).json(&body).send().await));
    }
    results
}

fn outcome(channel: &str, response: Result<reqwest::Response, reqwest::Error>) -> AlertTestResult {
    let (ok, message) = match response {
        Ok(r) if r.status().is_success() => (true, "Sent".to_owned()),
        Ok(r) => (false, format!("It answered {}", r.status())),
        Err(e) if e.is_timeout() => (false, "It didn't answer in time".to_owned()),
        Err(e) if e.is_connect() => (false, "Couldn't reach it".to_owned()),
        Err(e) => (false, e.to_string()),
    };
    AlertTestResult { channel: channel.to_owned(), ok, message }
}

/// Check a webhook or topic address someone typed.
fn checked_url(value: Option<String>, discord: bool, may_use_http: bool) -> Result<Option<String>, String> {
    let Some(value) = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) else { return Ok(None) };
    let url = url::Url::parse(&value).map_err(|_| format!("“{value}” isn't a web address."))?;
    if discord {
        let host_ok =
            matches!(url.host_str(), Some("discord.com" | "discordapp.com" | "ptb.discord.com" | "canary.discord.com"));
        if url.scheme() != "https" || !host_ok || !url.path().starts_with("/api/webhooks/") {
            return Err("That isn't a Discord webhook address (https://discord.com/api/webhooks/…).".into());
        }
    } else if !(url.scheme() == "https" || (may_use_http && url.scheme() == "http")) {
        return Err("Use an https:// ntfy address.".into());
    }
    Ok(Some(url.into()))
}

/// Where the browser reached delune, for absolute links in messages.
fn origin_of(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()).filter(|o| *o != "null") {
        return Some(origin.to_owned());
    }
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let https = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).is_some_and(|p| p == "https");
    Some(format!("{}://{host}", if https { "https" } else { "http" }))
}

/// `GET /api/v1/notifications/settings`
#[utoipa::path(
    get,
    operation_id = "alerts_settings",
    path = "/api/v1/notifications/settings",
    tag = "notifications",
    responses(
        (status = 200, description = "OK", body = delune_core::api::AlertSettings),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn settings(State(app): State<AppState>, user: CurrentUser) -> Json<AlertSettings> {
    Json(app.alerts.settings(&user.username))
}

/// `PUT /api/v1/notifications/settings`
#[utoipa::path(
    put,
    operation_id = "alerts_update",
    path = "/api/v1/notifications/settings",
    tag = "notifications",
    request_body = delune_core::api::AlertSettingsUpdate,
    responses(
        (status = 200, description = "Saved", body = delune_core::api::AlertSettings),
        (status = 400, description = "An address isn't usable", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Json(update): Json<AlertSettingsUpdate>,
) -> Response {
    // Only people who run delune may point it at plain-http addresses (a local ntfy).
    let manage = user.permissions.manage;
    let ntfy = match checked_url(update.ntfy, false, manage) {
        Ok(v) => v,
        Err(message) => return error(StatusCode::BAD_REQUEST, "bad-ntfy", &message),
    };
    let discord = match checked_url(update.discord, true, false) {
        Ok(v) => v,
        Err(message) => return error(StatusCode::BAD_REQUEST, "bad-discord", &message),
    };
    {
        let mut all = app.alerts.lock();
        let mine = all.entry(user.username.clone()).or_default();
        mine.ntfy = ntfy;
        mine.discord = discord;
        mine.muted = update.muted;
        mine.origin = origin_of(&headers).or_else(|| mine.origin.clone());
        app.alerts.save(&all);
    }
    Json(app.alerts.settings(&user.username)).into_response()
}

/// `POST /api/v1/notifications/devices`: this browser wants push notifications.
#[utoipa::path(
    post,
    operation_id = "alerts_add_device",
    path = "/api/v1/notifications/devices",
    tag = "notifications",
    request_body = delune_core::api::PushDevice,
    responses(
        (status = 200, description = "Subscribed", body = delune_core::api::AlertSettings),
        (status = 400, description = "Not a push subscription", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn add_device(
    State(app): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Json(device): Json<PushDevice>,
) -> Response {
    let subscription = Subscription { endpoint: device.endpoint, p256dh: device.p256dh, auth: device.auth };
    let valid = url::Url::parse(&subscription.endpoint).is_ok_and(|u| u.scheme() == "https")
        && webpush::encrypt(&subscription, b"check").is_ok();
    if !valid {
        return error(StatusCode::BAD_REQUEST, "bad-subscription", "That isn't a usable push subscription.");
    }
    {
        let mut all = app.alerts.lock();
        let mine = all.entry(user.username.clone()).or_default();
        mine.devices.retain(|d| d.subscription.endpoint != subscription.endpoint);
        mine.devices.push(Device {
            id: crate::store::new_id("d"),
            label: device.label.chars().take(80).collect(),
            added_at: now(),
            subscription,
        });
        let excess = mine.devices.len().saturating_sub(MAX_DEVICES);
        mine.devices.drain(..excess);
        mine.origin = origin_of(&headers).or_else(|| mine.origin.clone());
        app.alerts.save(&all);
    }
    Json(app.alerts.settings(&user.username)).into_response()
}

/// `DELETE /api/v1/notifications/devices/{id}`
#[utoipa::path(
    delete,
    operation_id = "alerts_remove_device",
    path = "/api/v1/notifications/devices/{id}",
    tag = "notifications",
    params(("id" = String, Path)),
    responses(
        (status = 200, description = "Removed", body = delune_core::api::AlertSettings),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn remove_device(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(id): UrlPath<String>,
) -> Json<AlertSettings> {
    {
        let mut all = app.alerts.lock();
        if let Some(mine) = all.get_mut(&user.username) {
            mine.devices.retain(|d| d.id != id);
        }
        app.alerts.save(&all);
    }
    Json(app.alerts.settings(&user.username))
}

/// `POST /api/v1/notifications/test`: send a test message everywhere, now.
#[utoipa::path(
    post,
    operation_id = "alerts_test",
    path = "/api/v1/notifications/test",
    tag = "notifications",
    responses(
        (status = 200, description = "How each channel fared", body = Vec<delune_core::api::AlertTestResult>),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn test(State(app): State<AppState>, user: CurrentUser) -> Json<Vec<AlertTestResult>> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap_or_default();
    let alert = Alert {
        username: user.username.clone(),
        kind: None,
        title: "delune can reach you".into(),
        detail: Some("This is a test. Downloads ready for review and other news will arrive like this.".into()),
        link: Some("/".into()),
    };
    Json(deliver(&app, &http, &alert).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_webhooks_and_topics_are_accepted() {
        let ok = |v: &str, discord, http| checked_url(Some(v.into()), discord, http);
        assert!(ok("https://discord.com/api/webhooks/1/abc", true, false).is_ok());
        assert!(ok("https://discord.com.evil.io/api/webhooks/1/abc", true, false).is_err());
        assert!(ok("http://discord.com/api/webhooks/1/abc", true, true).is_err());
        assert!(ok("https://discord.com/users/1", true, false).is_err());
        assert!(ok("https://ntfy.sh/delune-test", false, false).is_ok());
        assert!(ok("http://ntfy.home/delune", false, false).is_err(), "plain http is for admins");
        assert!(ok("http://ntfy.home/delune", false, true).is_ok());
        assert!(ok("file:///etc/passwd", false, true).is_err());
        assert_eq!(checked_url(Some("  ".into()), false, false), Ok(None));
        assert_eq!(
            absolute(Some("https://delune.example.com/"), Some("/review")).as_deref(),
            Some("https://delune.example.com/review")
        );
        assert_eq!(absolute(Some("https://x"), Some("https://evil")).as_deref(), Some("https://x/"));
    }

    #[tokio::test]
    async fn notifications_reach_ntfy_and_respect_mutes() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, mut seen) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { break };
                let mut buf = vec![0u8; 4096];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let _ = seen_tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
            }
        });

        let app = AppState::default();
        app.alerts.lock().insert(
            "sam".into(),
            Channels {
                ntfy: Some(format!("http://127.0.0.1:{port}/delune")),
                muted: vec![NotificationKind::Imported],
                origin: Some("https://delune.example.com".into()),
                ..Channels::default()
            },
        );
        start(&app);
        let note = |kind, title: &str| Notification {
            id: "n".into(),
            kind,
            at: 0,
            title: title.into(),
            detail: Some("details".into()),
            link: Some("/review".into()),
            read: false,
        };
        app.alerts.send("sam", &note(NotificationKind::Imported, "muted one"));
        app.alerts.send("sam", &note(NotificationKind::ReviewReady, "USB is ready for review"));
        app.alerts.send("nobody", &note(NotificationKind::ReviewReady, "no channels"));

        let request = tokio::time::timeout(Duration::from_secs(5), seen.recv()).await.unwrap().unwrap();
        assert!(request.starts_with("POST /delune"), "{request}");
        assert!(request.to_lowercase().contains("title: usb is ready for review"));
        assert!(request.to_lowercase().contains("click: https://delune.example.com/review"));
        assert!(
            tokio::time::timeout(Duration::from_millis(300), seen.recv()).await.is_err(),
            "the muted kind wasn't sent"
        );
    }
}
