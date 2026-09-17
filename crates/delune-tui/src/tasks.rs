//! Work done in the background. Results come back to the event loop as messages;
//! nothing here touches the terminal.

use std::sync::Arc;
use std::time::Duration;

use delune_core::api::{
    AlbumInfo, ArtistInfo, Candidate, DownloadJob, Health, ImportResult, LibraryMatch, LibraryState, Me, MusicSearch,
    ReviewReport, SearchEvent, SoulseekStatus,
};
use futures_util::StreamExt;
use reqwest::{Client, Method};
use tokio::sync::{Notify, Semaphore, mpsc};

use crate::api::{self, Failure, Outcome};
use crate::app::{Action, Message};
use crate::sse;

pub type Sender = mpsc::UnboundedSender<Message>;

/// Shared by every background task.
#[derive(Clone)]
pub struct Context {
    pub http: Client,
    pub base: String,
    pub tx: Sender,
    /// Wakes the download-list refresher.
    pub jobs_changed: Arc<Notify>,
    /// Keeps library lookups from swamping Navidrome.
    lookups: Arc<Semaphore>,
}

impl Context {
    #[must_use]
    pub fn new(http: Client, base: String, tx: Sender) -> Self {
        Self { http, base, tx, jobs_changed: Arc::new(Notify::new()), lookups: Arc::new(Semaphore::new(4)) }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Pass a result on, noticing when the session has ended.
    fn settle<T>(&self, result: Outcome<T>) -> Result<T, String> {
        result.map_err(|Failure { message, signed_out }| {
            if signed_out {
                let _ = self.tx.send(Message::SignedOut);
            }
            message
        })
    }
}

fn download_body(candidate: &Candidate) -> serde_json::Value {
    serde_json::json!({
        "username": candidate.username,
        "folder": candidate.folder,
        "title": candidate.title,
        "parent": candidate.parent,
        "files": candidate.files.iter().map(|f| serde_json::json!({ "path": f.path, "size": f.size })).collect::<Vec<_>>(),
    })
}

/// Carry out an action; the result arrives as a message.
#[allow(clippy::too_many_lines, reason = "one arm per action, each a single request")]
pub fn perform(action: Action, cx: &Context) {
    if matches!(action, Action::Cover { .. } | Action::Picture { .. }) {
        tokio::spawn(fetch_picture(action, cx.clone()));
        return;
    }
    let cx = cx.clone();
    tokio::spawn(async move {
        let downloads = cx.url("/api/v1/downloads");
        let notice = match action {
            Action::Download(candidate) => {
                let body = download_body(&candidate);
                api::send(&cx.http, Method::POST, &downloads, Some(&body))
                    .await
                    .map(|()| format!("Downloading “{}”. Press 2 to watch it.", candidate.title))
            }
            Action::Request(candidate) => {
                let artist = crate::matching::artist_from_folder(candidate.parent.as_deref());
                let query =
                    artist.as_deref().map_or_else(|| candidate.title.clone(), |a| format!("{a} {}", candidate.title));
                let body = serde_json::json!({
                    "title": candidate.title,
                    "artist": artist,
                    "query": query,
                    "download": download_body(&candidate),
                    "quality_label": candidate.quality_label,
                });
                api::send(&cx.http, Method::POST, &cx.url("/api/v1/requests"), Some(&body))
                    .await
                    .map(|()| format!("Asked for “{}”. You'll hear when an admin decides.", candidate.title))
            }
            Action::Stop(id) => api::send(&cx.http, Method::POST, &format!("{downloads}/{id}/stop"), None)
                .await
                .map(|()| "Stopped.".into()),
            Action::Resume(id) => api::send(&cx.http, Method::POST, &format!("{downloads}/{id}/resume"), None)
                .await
                .map(|()| "Resuming.".into()),
            Action::Prioritise(id) => api::send(&cx.http, Method::POST, &format!("{downloads}/{id}/prioritise"), None)
                .await
                .map(|()| "Moved to the front of the line.".into()),
            Action::Remove(id) => api::send(&cx.http, Method::DELETE, &format!("{downloads}/{id}"), None)
                .await
                .map(|()| "Removed.".into()),
            Action::Import(id) => {
                api::call::<ImportResult>(&cx.http, Method::POST, &format!("{downloads}/{id}/import"), None).await.map(
                    |r| {
                        let scan = if r.scan_started { " Navidrome is picking it up." } else { "" };
                        format!("Imported {} files into {}.{scan}", r.imported, r.folder)
                    },
                )
            }
            Action::LoadReport(id) => {
                let report = api::get::<ReviewReport>(&cx.http, &format!("{downloads}/{id}/review")).await;
                let report = cx.settle(report);
                let _ = cx.tx.send(Message::Report(id, report));
                return;
            }
            Action::Catalog(query) => {
                let url = api::url_with(&cx.base, "/api/v1/music/search", &[("q", &query)]);
                let found = cx.settle(api::get::<MusicSearch>(&cx.http, &url).await);
                let _ = cx.tx.send(Message::Catalog(query, found));
                return;
            }
            Action::Library { key, artist, album, context } => {
                let Ok(_permit) = cx.lookups.acquire().await else { return };
                let mut params = vec![("album", album.as_str()), ("context", context.as_str())];
                if let Some(artist) = &artist {
                    params.push(("artist", artist.as_str()));
                }
                let url = api::url_with(&cx.base, "/api/v1/library/album", &params);
                let found = cx.settle(api::get::<LibraryMatch>(&cx.http, &url).await).unwrap_or(LibraryMatch {
                    state: LibraryState::Unknown,
                    album: None,
                    artist: None,
                    year: None,
                    tracks: Vec::new(),
                    quality_label: None,
                });
                let _ = cx.tx.send(Message::Library(key, found));
                return;
            }
            Action::Artist(name) => {
                let url = api::url_with(&cx.base, "/api/v1/music/artist", &[("name", &name)]);
                let found = cx.settle(api::get::<ArtistInfo>(&cx.http, &url).await);
                let _ = cx.tx.send(Message::Artist(name, found));
                return;
            }
            Action::Album { key, artist, title } => {
                let mut params = vec![("album", title.as_str())];
                if let Some(artist) = &artist {
                    params.push(("artist", artist.as_str()));
                }
                let url = api::url_with(&cx.base, "/api/v1/music/album", &params);
                let found = cx.settle(api::get::<AlbumInfo>(&cx.http, &url).await);
                let _ = cx.tx.send(Message::Album(key, found));
                return;
            }
            Action::Cover { .. } | Action::Picture { .. } | Action::None | Action::StartSearch(_) => return,
        };
        let notice = cx.settle(notice);
        let _ = cx.tx.send(Message::Notice(notice));
        // Show the change straight away.
        cx.jobs_changed.notify_one();
    });
}

/// A cover (looked up by album) or a picture (by address), sent back decoded.
async fn fetch_picture(action: Action, cx: Context) {
    let Ok(_permit) = cx.lookups.acquire().await else { return };
    let (key, image) = match action {
        Action::Cover { key, artist, album } => {
            let mut params = vec![("album", album.as_str())];
            if let Some(artist) = &artist {
                params.push(("artist", artist.as_str()));
            }
            let url = api::url_with(&cx.base, "/api/v1/artwork", &params);
            let found = api::get::<serde_json::Value>(&cx.http, &url).await.ok();
            let thumb = found
                .as_ref()
                .and_then(|a| a.get("thumb").or_else(|| a.get("cover")))
                .and_then(|t| t.as_str())
                .map(str::to_owned);
            let image = match thumb {
                Some(path) => picture(&cx, &path).await,
                None => None,
            };
            (key, image)
        }
        Action::Picture { key, url } => {
            let image = picture(&cx, &url).await;
            (key, image)
        }
        _ => return,
    };
    let _ = cx.tx.send(Message::Picture(key, image));
}

/// Fetch and decode a picture; `path` may be relative to the server.
async fn picture(cx: &Context, path: &str) -> Option<image::DynamicImage> {
    // Only from delune itself: the client sends the session token with every request,
    // and that must never reach another host.
    let url = if path.starts_with('/') && !path.starts_with("//") {
        cx.url(path)
    } else if path.starts_with(&format!("{}/", cx.base)) {
        path.to_owned()
    } else {
        return None;
    };
    let bytes = api::bytes(&cx.http, &url, 4 << 20).await.ok()?;
    tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).ok()).await.ok().flatten()
}

/// Start everything that keeps the screens current.
pub fn start(cx: &Context) {
    tokio::spawn(poll_status(cx.clone()));
    tokio::spawn(refresh_jobs(cx.clone()));
    tokio::spawn(listen(cx.clone()));
    tokio::spawn(fetch_me(cx.clone()));
}

async fn fetch_me(cx: Context) {
    if let Ok(me) = cx.settle(api::get::<Option<Me>>(&cx.http, &cx.url("/api/v1/session")).await) {
        let _ = cx.tx.send(Message::Me(me));
    }
}

/// Refetch the download list when told to, and every few seconds regardless.
async fn refresh_jobs(cx: Context) {
    let url = cx.url("/api/v1/downloads");
    loop {
        if let Ok(jobs) = cx.settle(api::get::<Vec<DownloadJob>>(&cx.http, &url).await)
            && cx.tx.send(Message::Jobs(jobs)).is_err()
        {
            return;
        }
        if cx.tx.is_closed() {
            return;
        }
        // Progress events can come many times a second; one refresh per half second is plenty.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), cx.jobs_changed.notified()).await;
    }
}

/// Follow the server's change feed, refreshing whatever changed.
async fn listen(cx: Context) {
    let url = cx.url("/api/v1/events");
    loop {
        if cx.tx.is_closed() {
            return;
        }
        if let Ok(response) = cx.http.get(&url).header("accept", "text/event-stream").send().await
            && response.status().is_success()
        {
            let mut parser = sse::SseParser::default();
            let mut body = response.bytes_stream();
            while let Some(Ok(chunk)) = body.next().await {
                for topic in parser.push(&chunk) {
                    match topic.as_str() {
                        "downloads" | "progress" => cx.jobs_changed.notify_one(),
                        "session" | "people" => {
                            tokio::spawn(fetch_me(cx.clone()));
                        }
                        "all" => {
                            cx.jobs_changed.notify_one();
                            tokio::spawn(fetch_me(cx.clone()));
                        }
                        _ => {}
                    }
                }
            }
            // Reconnected: catch up on anything missed.
            cx.jobs_changed.notify_one();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn poll_status(cx: Context) {
    loop {
        let result = async {
            let health: Health = cx.http.get(cx.url("/api/v1/health")).send().await?.error_for_status()?.json().await?;
            let soulseek: Option<SoulseekStatus> =
                cx.http.get(cx.url("/api/v1/soulseek")).send().await?.json().await.ok();
            Ok::<_, reqwest::Error>((health, soulseek))
        }
        .await
        .map_err(|e| if e.is_connect() { "can't reach the server".to_owned() } else { e.to_string() });
        if cx.tx.send(Message::Status(result)).is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}

/// Stream a Soulseek search into the app.
pub async fn stream_search(cx: Context, query: String) {
    let url = api::url_with(&cx.base, "/api/v1/search", &[("q", &query)]);
    let response = match cx.http.get(url).header("accept", "text/event-stream").send().await {
        Ok(r) => r,
        Err(e) => {
            let _ = cx.tx.send(Message::SearchError(format!("Can't reach the server: {e}")));
            return;
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            let _ = cx.tx.send(Message::SignedOut);
        }
        let message = response
            .json::<delune_core::api::ApiError>()
            .await
            .map_or_else(|_| format!("Search failed ({status})."), |e| e.message);
        let _ = cx.tx.send(Message::SearchError(message));
        return;
    }

    let mut parser = sse::SseParser::default();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let Ok(chunk) = chunk else {
            let _ = cx.tx.send(Message::SearchError("The connection to the server dropped.".into()));
            return;
        };
        for data in parser.push(&chunk) {
            if let Ok(event) = serde_json::from_str::<SearchEvent>(&data)
                && cx.tx.send(Message::Search(event)).is_err()
            {
                return;
            }
        }
    }
}
