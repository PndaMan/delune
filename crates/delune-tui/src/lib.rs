//! # delune-tui
//!
//! The terminal client. It is a *client*: everything it shows comes from a delune
//! server over the same HTTP API the web UI uses, so you can run it on your laptop
//! against the server next to Navidrome.
//!
//! Architecture follows the usual ratatui shape — a single [`App`] state, a pure
//! `ui::draw` function, and an event loop that merges key presses with messages
//! from background tasks over a channel. Background tasks never touch the terminal.

mod ui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use delune_core::api::Health;
use tokio::sync::mpsc;

/// How often the connection indicator refreshes.
const HEALTH_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected(Health),
    Unreachable(String),
}

/// All state the UI renders from.
#[derive(Debug)]
pub struct App {
    pub server_url: String,
    pub connection: Connection,
    pub query: String,
    pub should_quit: bool,
}

enum Message {
    Health(Result<Health, String>),
}

impl App {
    #[must_use]
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            connection: Connection::Connecting,
            query: String::new(),
            should_quit: false,
        }
    }

    fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => self.should_quit = true,
            KeyCode::Esc if self.query.is_empty() => self.should_quit = true,
            KeyCode::Esc => self.query.clear(),
            KeyCode::Backspace => {
                self.query.pop();
            }
            KeyCode::Char(c) => self.query.push(c),
            _ => {}
        }
    }

    fn on_message(&mut self, msg: Message) {
        match msg {
            Message::Health(Ok(h)) => self.connection = Connection::Connected(h),
            Message::Health(Err(e)) => self.connection = Connection::Unreachable(e),
        }
    }
}

/// Run the TUI against `server_url` until the user quits.
///
/// Blocks the calling thread on the terminal event loop, and must be called from
/// inside a multi-threaded Tokio runtime so background tasks keep running.
pub fn run(server_url: String) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(poll_health(server_url.trim_end_matches('/').to_owned(), tx));

    let mut terminal = ratatui::init();
    let mut app = App::new(server_url);
    let result = loop {
        if let Err(e) = terminal.draw(|frame| ui::draw(frame, &app)) {
            break Err(e.into());
        }
        while let Ok(msg) = rx.try_recv() {
            app.on_message(msg);
        }
        // Poll briefly so background messages repaint promptly without busy-looping.
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => app.on_key(key.code, key.modifiers),
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

async fn poll_health(base: String, tx: mpsc::UnboundedSender<Message>) {
    let http = reqwest::Client::new();
    let url = format!("{base}/api/v1/health");
    loop {
        let result = async { http.get(&url).send().await?.error_for_status()?.json::<Health>().await }
            .await
            .map_err(|e| if e.is_connect() { "can't reach server".to_owned() } else { e.to_string() });
        if tx.send(Message::Health(result)).is_err() {
            return; // UI has exited.
        }
        tokio::time::sleep(HEALTH_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_escape() {
        let mut app = App::new("http://localhost:7474");
        for c in "ok computer".chars() {
            app.on_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(app.query, "ok computer");
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.query.is_empty() && !app.should_quit, "first Esc clears the query");
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.should_quit, "second Esc quits");
    }
}
