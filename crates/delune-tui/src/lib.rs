//! # delune-tui
//!
//! The terminal client. It is a *client*: everything it shows comes from a delune
//! server over the same HTTP API the web UI uses, so you can run it on your laptop
//! against the server next to Navidrome.
//!
//! Architecture follows the usual ratatui shape — a single [`App`] state, a pure
//! `ui::draw` function, and an event loop that merges key presses with messages
//! from background tasks over a channel. Background tasks never touch the terminal.

mod api;
pub mod auth;
mod sse;
mod ui;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use delune_core::api::{
    ApiError, Candidate, DownloadJob, Health, JobStatus, ResolvedLink, ReviewReport, ReviewState, SearchEvent,
    SoulseekState, SoulseekStatus,
};
use futures_util::StreamExt;
use reqwest::Method;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// How often the connection indicator refreshes.
const STATUS_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected { health: Health, soulseek: Option<SoulseekStatus> },
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchState {
    Idle,
    Running { query: String, started: Instant, timeout: Duration, peers: u32 },
    Done { query: String, peers: u32 },
    Failed(String),
}

/// The three places the TUI can be, like the web app's first three tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Search,
    Downloads,
    Review,
}

/// A review report, as far as it has loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Loading,
    Ready(Box<ReviewReport>),
    Failed(String),
}

/// A question waiting for y or n.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub prompt: String,
    action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    Results,
}

/// All state the UI renders from.
#[derive(Debug)]
pub struct App {
    pub server_url: String,
    pub connection: Connection,
    pub query: String,
    pub focus: Focus,
    pub search: SearchState,
    /// Sorted best-first.
    pub results: Vec<Candidate>,
    /// What a pasted link turned out to be.
    pub resolved: Option<ResolvedLink>,
    pub selected: usize,
    pub should_quit: bool,
    pub screen: Screen,
    /// Every download job, newest first.
    pub jobs: Vec<DownloadJob>,
    pub job_selected: usize,
    pub review_selected: usize,
    pub reports: HashMap<String, Report>,
    /// A short message about the last action, and whether it went wrong.
    pub notice: Option<(String, bool)>,
    pub confirm: Option<Confirm>,
    search_task: Option<JoinHandle<()>>,
}

#[derive(Debug)]
enum Message {
    Status(Result<(Health, Option<SoulseekStatus>), String>),
    Search(SearchEvent),
    SearchError(String),
    Jobs(Result<Vec<DownloadJob>, String>),
    Report(String, Result<ReviewReport, String>),
    Notice(Result<String, String>),
}

/// Something the event loop must do after a key press.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    None,
    StartSearch(String),
    Download(Box<Candidate>),
    Stop(String),
    Resume(String),
    Remove(String),
    Import(String),
    LoadReport(String),
}

impl App {
    #[must_use]
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            connection: Connection::Connecting,
            query: String::new(),
            focus: Focus::Input,
            search: SearchState::Idle,
            results: Vec::new(),
            resolved: None,
            selected: 0,
            should_quit: false,
            screen: Screen::Search,
            jobs: Vec::new(),
            job_selected: 0,
            review_selected: 0,
            reports: HashMap::new(),
            notice: None,
            confirm: None,
            search_task: None,
        }
    }

    #[must_use]
    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.results.get(self.selected)
    }

    /// Jobs waiting for review, in the order the Review screen lists them.
    #[must_use]
    pub fn reviewable(&self) -> Vec<&DownloadJob> {
        self.jobs.iter().filter(|j| j.status == JobStatus::Ready).collect()
    }

    fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Action::None;
        }
        if let Some(confirm) = self.confirm.take() {
            return match code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => confirm.action,
                _ => Action::None,
            };
        }
        let typing = self.screen == Screen::Search && self.focus == Focus::Input;
        if !typing {
            match code {
                KeyCode::Char('1') => {
                    self.screen = Screen::Search;
                    return Action::None;
                }
                KeyCode::Char('2') => {
                    self.screen = Screen::Downloads;
                    return Action::None;
                }
                KeyCode::Char('3') => {
                    self.screen = Screen::Review;
                    return self.report_to_load();
                }
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Action::None;
                }
                _ => {}
            }
        }
        match self.screen {
            Screen::Search => self.on_search_key(code),
            Screen::Downloads => self.on_downloads_key(code),
            Screen::Review => self.on_review_key(code),
        }
    }

    fn on_downloads_key(&mut self, code: KeyCode) -> Action {
        let last = self.jobs.len().saturating_sub(1);
        let job = self.jobs.get(self.job_selected).cloned();
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.job_selected = self.job_selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.job_selected = (self.job_selected + 1).min(last),
            KeyCode::Esc | KeyCode::Char('/') => {
                self.screen = Screen::Search;
                self.focus = Focus::Input;
            }
            KeyCode::Char('s') => {
                if let Some(job) = job {
                    return match job.status {
                        JobStatus::Queued | JobStatus::Downloading => Action::Stop(job.id),
                        JobStatus::Failed | JobStatus::Cancelled => Action::Resume(job.id),
                        _ => Action::None,
                    };
                }
            }
            KeyCode::Char('x') => {
                if let Some(job) = job {
                    self.confirm = Some(Confirm {
                        prompt: format!("Remove “{}” and its downloaded files?", job.title),
                        action: Action::Remove(job.id),
                    });
                }
            }
            KeyCode::Enter | KeyCode::Char('r') if job.as_ref().is_some_and(|j| j.status == JobStatus::Ready) => {
                if let Some(job) = job {
                    self.screen = Screen::Review;
                    self.review_selected = self.reviewable().iter().position(|j| j.id == job.id).unwrap_or(0);
                    return self.report_to_load();
                }
            }
            _ => {}
        }
        Action::None
    }

    fn on_review_key(&mut self, code: KeyCode) -> Action {
        let jobs: Vec<DownloadJob> = self.reviewable().into_iter().cloned().collect();
        let last = jobs.len().saturating_sub(1);
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.review_selected = self.review_selected.saturating_sub(1);
                return self.report_to_load();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.review_selected = (self.review_selected + 1).min(last);
                return self.report_to_load();
            }
            KeyCode::Esc | KeyCode::Char('/') => {
                self.screen = Screen::Search;
                self.focus = Focus::Input;
            }
            KeyCode::Char('i') => {
                if let Some(job) = jobs.get(self.review_selected) {
                    let blocked = match self.reports.get(&job.id) {
                        Some(Report::Ready(report)) => report.blocked_reason.clone(),
                        _ => Some("The files are still being checked.".into()),
                    };
                    if let Some(reason) = blocked {
                        self.notice = Some((reason, true));
                    } else {
                        self.confirm = Some(Confirm {
                            prompt: format!("Import “{}” into the library?", job.title),
                            action: Action::Import(job.id.clone()),
                        });
                    }
                }
            }
            KeyCode::Char('x') => {
                if let Some(job) = jobs.get(self.review_selected) {
                    self.confirm = Some(Confirm {
                        prompt: format!("Discard “{}” and its downloaded files?", job.title),
                        action: Action::Remove(job.id.clone()),
                    });
                }
            }
            _ => {}
        }
        Action::None
    }

    /// Ask for the selected job's review report if we don't have it yet.
    fn report_to_load(&mut self) -> Action {
        let Some(job) = self.reviewable().get(self.review_selected).map(|j| (j.id.clone(), j.review)) else {
            return Action::None;
        };
        if job.1 == ReviewState::Ready && !matches!(self.reports.get(&job.0), Some(Report::Ready(_) | Report::Loading))
        {
            self.reports.insert(job.0.clone(), Report::Loading);
            return Action::LoadReport(job.0);
        }
        Action::None
    }

    fn on_search_key(&mut self, code: KeyCode) -> Action {
        match self.focus {
            Focus::Input => match code {
                KeyCode::Enter if !self.query.trim().is_empty() => {
                    return Action::StartSearch(self.query.trim().into());
                }
                KeyCode::Esc if self.query.is_empty() => self.should_quit = true,
                KeyCode::Esc => self.query.clear(),
                KeyCode::Down | KeyCode::Tab if !self.results.is_empty() => self.focus = Focus::Results,
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(c) => self.query.push(c),
                _ => {}
            },
            Focus::Results => match code {
                KeyCode::Char('d') => {
                    if let Some(candidate) = self.selected_candidate() {
                        return Action::Download(Box::new(candidate.clone()));
                    }
                }
                KeyCode::Esc | KeyCode::Tab | KeyCode::Char('/') => self.focus = Focus::Input,
                KeyCode::Up | KeyCode::Char('k') if self.selected == 0 => self.focus = Focus::Input,
                KeyCode::Up | KeyCode::Char('k') => self.selected -= 1,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.results.len().saturating_sub(1));
                }
                KeyCode::PageDown => self.selected = (self.selected + 10).min(self.results.len().saturating_sub(1)),
                KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
                KeyCode::Home | KeyCode::Char('g') => self.selected = 0,
                KeyCode::End | KeyCode::Char('G') => self.selected = self.results.len().saturating_sub(1),
                _ => {}
            },
        }
        Action::None
    }

    fn on_message(&mut self, msg: Message) {
        match msg {
            Message::Status(Ok((health, soulseek))) => self.connection = Connection::Connected { health, soulseek },
            Message::Status(Err(e)) => self.connection = Connection::Unreachable(e),
            Message::SearchError(e) => self.search = SearchState::Failed(e),
            Message::Search(event) => self.on_search_event(event),
            Message::Jobs(Ok(jobs)) => {
                // Reviews that were redone need fetching again.
                for job in &jobs {
                    if job.review != ReviewState::Ready {
                        self.reports.remove(&job.id);
                    }
                }
                self.jobs = jobs;
                self.job_selected = self.job_selected.min(self.jobs.len().saturating_sub(1));
                self.review_selected = self.review_selected.min(self.reviewable().len().saturating_sub(1));
            }
            Message::Jobs(Err(_)) => {}
            Message::Report(id, result) => {
                let report = result.map_or_else(Report::Failed, |r| Report::Ready(Box::new(r)));
                self.reports.insert(id, report);
            }
            Message::Notice(result) => {
                self.notice = Some(match result {
                    Ok(text) => (text, false),
                    Err(text) => (text, true),
                });
            }
        }
    }

    fn on_search_event(&mut self, event: SearchEvent) {
        match event {
            SearchEvent::Resolved { link } => self.resolved = Some(link),
            SearchEvent::Started { query, timeout_secs } => {
                self.search = SearchState::Running {
                    query,
                    started: Instant::now(),
                    timeout: Duration::from_secs(timeout_secs.into()),
                    peers: 0,
                };
            }
            SearchEvent::Candidates { items } => {
                if let SearchState::Running { peers, .. } = &mut self.search {
                    *peers += 1;
                }
                // Keep the same candidate selected while new results slot in around it.
                let selected_id = self.selected_candidate().map(|c| c.id.clone());
                self.results.extend(items);
                Candidate::rank(&mut self.results);
                if let Some(id) = selected_id {
                    self.selected = self.results.iter().position(|c| c.id == id).unwrap_or(0);
                }
            }
            SearchEvent::Finished { peers, .. } => {
                let query = match &self.search {
                    SearchState::Running { query, .. } => query.clone(),
                    _ => String::new(),
                };
                self.search = SearchState::Done { query, peers };
            }
            SearchEvent::Failed { error } => self.search = SearchState::Failed(error.message),
        }
    }

    fn start_search(&mut self, http: &reqwest::Client, base: &str, query: String, tx: mpsc::UnboundedSender<Message>) {
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        self.results.clear();
        self.resolved = None;
        self.selected = 0;
        self.search = SearchState::Running {
            query: query.clone(),
            started: Instant::now(),
            timeout: Duration::from_secs(20),
            peers: 0,
        };
        self.search_task = Some(tokio::spawn(stream_search(http.clone(), base.to_owned(), query, tx)));
    }
}

/// Run the TUI against `server_url` until the user quits.
///
/// Blocks the calling thread on the terminal event loop, and must be called from
/// inside a multi-threaded Tokio runtime so background tasks keep running.
pub fn run(server_url: String, http: &reqwest::Client) -> Result<()> {
    let base = server_url.trim_end_matches('/').to_owned();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(poll_status(http.clone(), base.clone(), tx.clone()));
    tokio::spawn(poll_jobs(http.clone(), base.clone(), tx.clone()));

    let mut terminal = ratatui::init();
    let mut app = App::new(server_url);
    let result = loop {
        if let Err(e) = terminal.draw(|frame| ui::draw(frame, &app)) {
            break Err(e.into());
        }
        while let Ok(msg) = rx.try_recv() {
            app.on_message(msg);
        }
        // Poll briefly so background messages and the progress bar repaint promptly
        // without busy-looping.
        if app.screen == Screen::Review
            && let Action::LoadReport(id) = app.report_to_load()
        {
            perform(Action::LoadReport(id), http, &base, &tx);
        }
        match event::poll(Duration::from_millis(80)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => match app.on_key(key.code, key.modifiers) {
                    Action::StartSearch(query) => app.start_search(http, &base, query, tx.clone()),
                    Action::None => {}
                    action => {
                        app.notice = None;
                        perform(action, http, &base, &tx);
                    }
                },
                Ok(_) => {}
                Err(e) => break Err(e.into()),
            },
            Ok(false) => {}
            Err(e) => break Err(e.into()),
        }
        if app.should_quit {
            break Ok(());
        }
    };
    ratatui::restore();
    result
}

/// Carry out an action in the background; the result arrives as a message.
fn perform(action: Action, http: &reqwest::Client, base: &str, tx: &mpsc::UnboundedSender<Message>) {
    let (http, base, tx) = (http.clone(), base.to_owned(), tx.clone());
    tokio::spawn(async move {
        let downloads = format!("{base}/api/v1/downloads");
        let notice = match action {
            Action::Download(candidate) => {
                let body = serde_json::json!({
                    "username": candidate.username,
                    "folder": candidate.folder,
                    "title": candidate.title,
                    "parent": candidate.parent,
                    "files": candidate.files.iter().map(|f| serde_json::json!({ "path": f.path, "size": f.size })).collect::<Vec<_>>(),
                });
                api::send(&http, Method::POST, &downloads, Some(&body))
                    .await
                    .map(|()| format!("Downloading “{}” for review. Press 2 to watch it.", candidate.title))
            }
            Action::Stop(id) => api::send(&http, Method::POST, &format!("{downloads}/{id}/stop"), None)
                .await
                .map(|()| "Stopped.".into()),
            Action::Resume(id) => api::send(&http, Method::POST, &format!("{downloads}/{id}/resume"), None)
                .await
                .map(|()| "Resumed.".into()),
            Action::Remove(id) => api::send(&http, Method::DELETE, &format!("{downloads}/{id}"), None)
                .await
                .map(|()| "Removed, with its files.".into()),
            Action::Import(id) => {
                let result: Result<delune_core::api::ImportResult, String> = async {
                    let response =
                        http.post(format!("{downloads}/{id}/import")).send().await.map_err(|e| e.to_string())?;
                    if response.status().is_success() {
                        response.json().await.map_err(|e| e.to_string())
                    } else {
                        Err(response.json::<ApiError>().await.map_or_else(|_| "Import failed.".into(), |e| e.message))
                    }
                }
                .await;
                result.map(|r| format!("Imported {} files into {}.", r.imported, r.folder))
            }
            Action::LoadReport(id) => {
                let report = api::get::<ReviewReport>(&http, &format!("{downloads}/{id}/review")).await;
                let _ = tx.send(Message::Report(id, report));
                return;
            }
            Action::None | Action::StartSearch(_) => return,
        };
        let _ = tx.send(Message::Notice(notice));
        // Show the change straight away instead of at the next poll.
        let _ = tx.send(Message::Jobs(api::get(&http, &downloads).await));
    });
}

async fn poll_jobs(http: reqwest::Client, base: String, tx: mpsc::UnboundedSender<Message>) {
    let url = format!("{base}/api/v1/downloads");
    loop {
        if tx.send(Message::Jobs(api::get(&http, &url).await)).is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1_500)).await;
    }
}

async fn poll_status(http: reqwest::Client, base: String, tx: mpsc::UnboundedSender<Message>) {
    loop {
        let result = async {
            let health = http.get(format!("{base}/api/v1/health")).send().await?.error_for_status()?.json().await?;
            let soulseek = http.get(format!("{base}/api/v1/soulseek")).send().await?.json().await.ok();
            Ok::<_, reqwest::Error>((health, soulseek))
        }
        .await
        .map_err(|e| if e.is_connect() { "can't reach server".to_owned() } else { e.to_string() });
        if tx.send(Message::Status(result)).is_err() {
            return; // UI has exited.
        }
        tokio::time::sleep(STATUS_INTERVAL).await;
    }
}

async fn stream_search(http: reqwest::Client, base: String, query: String, tx: mpsc::UnboundedSender<Message>) {
    let mut url = match url::Url::parse(&format!("{base}/api/v1/search")) {
        Ok(url) => url,
        Err(e) => {
            let _ = tx.send(Message::SearchError(format!("Bad server URL: {e}")));
            return;
        }
    };
    url.query_pairs_mut().append_pair("q", &query);

    let response = match http.get(url).header("accept", "text/event-stream").send().await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Message::SearchError(format!("Can't reach the server: {e}")));
            return;
        }
    };
    if !response.status().is_success() {
        let status = response.status();
        let message =
            response.json::<ApiError>().await.map_or_else(|_| format!("Search failed ({status})."), |e| e.message);
        let _ = tx.send(Message::SearchError(message));
        return;
    }

    let mut parser = sse::SseParser::default();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let Ok(chunk) = chunk else {
            let _ = tx.send(Message::SearchError("The connection to the server dropped.".into()));
            return;
        };
        for data in parser.push(&chunk) {
            if let Ok(event) = serde_json::from_str::<SearchEvent>(&data)
                && tx.send(Message::Search(event)).is_err()
            {
                return;
            }
        }
    }
}

/// Whether the server's Soulseek client is usable, for the header.
#[must_use]
pub fn soulseek_ready(connection: &Connection) -> bool {
    matches!(
        connection,
        Connection::Connected { soulseek: Some(SoulseekStatus { state: SoulseekState::Online, .. }), .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use delune_core::{Codec, Quality};

    fn candidate(id: &str, quality: Quality) -> Candidate {
        Candidate {
            id: id.into(),
            username: "peer".into(),
            folder: id.into(),
            title: id.into(),
            parent: None,
            files: vec![],
            audio_files: 10,
            total_bytes: 1,
            duration_secs: None,
            quality: Some(quality),
            quality_label: Some(quality.to_string()),
            quality_rank: quality.rank(),
            mixed_quality: false,
            has_cover: false,
            free_slot: true,
            avg_speed: 1,
            queue_length: 0,
        }
    }

    #[test]
    fn typing_enter_and_escape() {
        let mut app = App::new("http://localhost:7474");
        for c in "ok computer".chars() {
            app.on_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(app.on_key(KeyCode::Enter, KeyModifiers::NONE), Action::StartSearch("ok computer".into()));
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.query.is_empty() && !app.should_quit, "first Esc clears the query");
        assert_eq!(app.on_key(KeyCode::Enter, KeyModifiers::NONE), Action::None, "empty query doesn't search");
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.should_quit, "second Esc quits");
    }

    fn job(id: &str, status: JobStatus) -> DownloadJob {
        DownloadJob {
            id: id.into(),
            username: "peer".into(),
            folder: r"music\Twoism".into(),
            title: "Twoism".into(),
            parent: Some("Boards of Canada".into()),
            created_at: 0,
            status,
            files: vec![delune_core::api::JobFile {
                path: r"music\Twoism\01 Sixtyniner.flac".into(),
                name: "01 Sixtyniner.flac".into(),
                size: 100,
                status: delune_core::api::FileStatus::Transferring,
                bytes: 40,
                place_in_queue: None,
                error: None,
            }],
            bytes: 40,
            total_bytes: 100,
            review: ReviewState::Waiting,
            requested_by: None,
        }
    }

    #[test]
    fn screens_download_and_confirm() {
        let mut app = App::new("x");
        app.on_search_event(SearchEvent::Candidates {
            items: vec![candidate("cd", Quality::lossless(Codec::Flac, 16, 44_100))],
        });
        app.on_key(KeyCode::Char('1'), KeyModifiers::NONE);
        assert_eq!(app.query, "1", "digits type into the search box");
        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        assert!(matches!(app.on_key(KeyCode::Char('d'), KeyModifiers::NONE), Action::Download(c) if c.id == "cd"));

        app.on_message(Message::Jobs(Ok(vec![job("a", JobStatus::Downloading), job("b", JobStatus::Cancelled)])));
        app.on_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(app.screen, Screen::Downloads);
        assert_eq!(app.on_key(KeyCode::Char('s'), KeyModifiers::NONE), Action::Stop("a".into()));
        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.on_key(KeyCode::Char('s'), KeyModifiers::NONE), Action::Resume("b".into()));

        assert_eq!(app.on_key(KeyCode::Char('x'), KeyModifiers::NONE), Action::None);
        assert!(app.confirm.is_some(), "removing asks first");
        assert_eq!(app.on_key(KeyCode::Char('n'), KeyModifiers::NONE), Action::None);
        assert!(app.confirm.is_none());
        app.on_key(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.on_key(KeyCode::Char('y'), KeyModifiers::NONE), Action::Remove("b".into()));
    }

    #[test]
    fn review_waits_for_checks_before_importing() {
        let mut app = App::new("x");
        let mut ready = job("r", JobStatus::Ready);
        ready.review = ReviewState::Ready;
        app.on_message(Message::Jobs(Ok(vec![ready])));
        assert_eq!(app.on_key(KeyCode::Esc, KeyModifiers::NONE), Action::None);
        app.focus = Focus::Results;
        assert_eq!(app.on_key(KeyCode::Char('3'), KeyModifiers::NONE), Action::LoadReport("r".into()));
        app.on_key(KeyCode::Char('i'), KeyModifiers::NONE);
        assert!(app.notice.as_ref().is_some_and(|(_, bad)| *bad), "can't import before the report arrives");

        let report = ReviewReport {
            album_artist: "Boards of Canada".into(),
            album: "Twoism".into(),
            year: Some(1995),
            tracks: vec![],
            cover: None,
            warnings: vec![],
            conflicts: vec![],
            library_dir: None,
            blocked_reason: None,
        };
        app.on_message(Message::Report("r".into(), Ok(report)));
        app.on_key(KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(app.on_key(KeyCode::Char('y'), KeyModifiers::NONE), Action::Import("r".into()));
    }

    #[test]
    fn renders_every_screen() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new("http://localhost:7474");
        app.on_message(Message::Jobs(Ok(vec![job("a", JobStatus::Downloading)])));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        for screen in [Screen::Search, Screen::Downloads, Screen::Review] {
            app.screen = screen;
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            let text: String =
                terminal.backend().buffer().content().iter().map(ratatui::buffer::Cell::symbol).collect();
            if std::env::var_os("DELUNE_PRINT_SCREENS").is_some() {
                for row in text.chars().collect::<Vec<_>>().chunks(100) {
                    println!("{}", row.iter().collect::<String>());
                }
            }
            assert!(text.contains("Downloads"), "the header lists the screens");
            if screen == Screen::Downloads {
                assert!(text.contains("Downloading 0 of 1 from peer"));
            }
        }
    }

    #[test]
    fn results_merge_sorted_and_keep_selection() {
        let mut app = App::new("x");
        app.on_search_event(SearchEvent::Started { query: "q".into(), timeout_secs: 20 });
        app.on_search_event(SearchEvent::Candidates { items: vec![candidate("mp3", Quality::lossy(Codec::Mp3, 320))] });
        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.focus, Focus::Results);
        assert_eq!(app.selected_candidate().unwrap().id, "mp3");

        app.on_search_event(SearchEvent::Candidates {
            items: vec![candidate("hires", Quality::lossless(Codec::Flac, 24, 96_000))],
        });
        assert_eq!(app.results[0].id, "hires", "better quality sorts to the top");
        assert_eq!(app.selected_candidate().unwrap().id, "mp3", "selection follows the item, not the row");

        app.on_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.selected_candidate().unwrap().id, "hires");
        app.on_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.focus, Focus::Input, "moving up from the first row returns to the search box");

        app.on_search_event(SearchEvent::Finished { peers: 2, candidates: 2 });
        assert_eq!(app.search, SearchState::Done { query: "q".into(), peers: 2 });
    }
}
