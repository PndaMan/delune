//! # delune-tui
//!
//! The terminal client. It is a *client*: everything it shows comes from a delune
//! server over the same HTTP API the web UI uses, so you can run it on your laptop
//! against the server next to Navidrome.
//!
//! Architecture follows the usual ratatui shape — a single [`App`] state, a pure
//! `ui::draw` function, and an event loop that merges key presses with messages
//! from background tasks over a channel. Background tasks never touch the terminal.

mod sse;
mod ui;

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use delune_core::api::{ApiError, Candidate, Health, SearchEvent, SoulseekState, SoulseekStatus};
use futures_util::StreamExt;
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
    pub selected: usize,
    pub should_quit: bool,
    search_task: Option<JoinHandle<()>>,
}

#[derive(Debug)]
enum Message {
    Status(Result<(Health, Option<SoulseekStatus>), String>),
    Search(SearchEvent),
    SearchError(String),
}

/// Something the event loop must do after a key press.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    None,
    StartSearch(String),
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
            selected: 0,
            should_quit: false,
            search_task: None,
        }
    }

    #[must_use]
    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.results.get(self.selected)
    }

    fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Action::None;
        }
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
                KeyCode::Char('q') => self.should_quit = true,
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
        }
    }

    fn on_search_event(&mut self, event: SearchEvent) {
        match event {
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

    fn start_search(&mut self, base: &str, query: String, tx: mpsc::UnboundedSender<Message>) {
        if let Some(task) = self.search_task.take() {
            task.abort();
        }
        self.results.clear();
        self.selected = 0;
        self.search = SearchState::Running {
            query: query.clone(),
            started: Instant::now(),
            timeout: Duration::from_secs(20),
            peers: 0,
        };
        self.search_task = Some(tokio::spawn(stream_search(base.to_owned(), query, tx)));
    }
}

/// Run the TUI against `server_url` until the user quits.
///
/// Blocks the calling thread on the terminal event loop, and must be called from
/// inside a multi-threaded Tokio runtime so background tasks keep running.
pub fn run(server_url: String) -> Result<()> {
    let base = server_url.trim_end_matches('/').to_owned();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(poll_status(base.clone(), tx.clone()));

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
        match event::poll(Duration::from_millis(80)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if let Action::StartSearch(query) = app.on_key(key.code, key.modifiers) {
                        app.start_search(&base, query, tx.clone());
                    }
                }
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

async fn poll_status(base: String, tx: mpsc::UnboundedSender<Message>) {
    let http = reqwest::Client::new();
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

async fn stream_search(base: String, query: String, tx: mpsc::UnboundedSender<Message>) {
    let mut url = match url::Url::parse(&format!("{base}/api/v1/search")) {
        Ok(url) => url,
        Err(e) => {
            let _ = tx.send(Message::SearchError(format!("Bad server URL: {e}")));
            return;
        }
    };
    url.query_pairs_mut().append_pair("q", &query);

    let response = match reqwest::Client::new().get(url).header("accept", "text/event-stream").send().await {
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
