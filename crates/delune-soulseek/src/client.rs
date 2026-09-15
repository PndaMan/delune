//! The Soulseek client: one long-lived session with the server, a listener for
//! incoming peer connections, and searches whose results stream in as peers answer.
//!
//! # Shape
//!
//! [`Client::start`] spawns a *supervisor* task and returns a cheap, cloneable
//! handle. The supervisor owns the server connection and reconnects with
//! exponential backoff when it drops. Callers never see sockets; they see:
//!
//! - [`Client::state`] — a watch channel of [`SessionState`] for status displays.
//! - [`Client::search`] — returns a [`Search`] that yields [`SearchResponse`]s until
//!   its timeout.
//!
//! ```text
//!   Client handle ──commands──▶ supervisor ──▶ server session (TCP :2242)
//!         ▲                        │  ConnectToPeer
//!         │                        ▼
//!   Search ◀──responses── registry ◀── peer connections (ours or theirs)
//! ```
//!
//! # Behaving well
//!
//! - Searches pass through a [`SearchLimiter`] before reaching the server.
//! - Reconnects back off from 5 s up to 5 minutes, so an outage doesn't turn into a
//!   login storm.
//! - A rejected login or being logged in elsewhere stops the client instead of
//!   retrying: fighting another client for the same account gets it banned.
//! - Concurrent peer connections are capped.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::time::{Instant, sleep, timeout};
use tokio_util::codec::Framed;

use crate::connection;
use crate::frame::{FrameCodec, MAX_SERVER_FRAME, split_code};
use crate::limiter::{DEFAULT_MAX_SEARCHES, DEFAULT_WINDOW, SearchLimiter};
use crate::peer::SearchResponse;
use crate::server::{ConnectionType, LoginRejection, ServerEvent, ServerRequest, Status};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const PING_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct Config {
    /// `host:port` of the Soulseek server.
    pub server: String,
    pub username: String,
    pub password: String,
    /// Port for incoming peer connections. `Some(0)` picks a free port; `None`
    /// doesn't listen, so only peers we can reach ourselves will deliver results.
    pub listen_port: Option<u16>,
    /// How long a search keeps collecting responses.
    pub search_timeout: Duration,
    pub max_searches: usize,
    pub search_window: Duration,
    pub max_peer_connections: usize,
    /// First reconnect delay; doubles on each failure up to five minutes.
    pub reconnect_base: Duration,
}

impl Config {
    #[must_use]
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            server: crate::DEFAULT_SERVER.to_owned(),
            username: username.into(),
            password: password.into(),
            listen_port: Some(2234),
            search_timeout: Duration::from_secs(20),
            max_searches: DEFAULT_MAX_SEARCHES,
            search_window: DEFAULT_WINDOW,
            max_peer_connections: 200,
            reconnect_base: Duration::from_secs(5),
        }
    }
}

/// Where the session is, for display and for deciding whether to search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Connecting {
        attempt: u32,
    },
    Online {
        public_ip: Ipv4Addr,
        listen_port: Option<u16>,
        supporter: bool,
    },
    Reconnecting {
        reason: String,
        retry_in: Duration,
    },
    /// Terminal: the client won't try again on its own.
    Stopped(StopReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    LoginRejected(LoginRejection),
    /// This account logged in from another client.
    LoggedInElsewhere,
    /// Every [`Client`] handle was dropped.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not connected to Soulseek")]
    NotOnline(SessionState),
    #[error("search text is empty")]
    EmptyQuery,
    #[error("the Soulseek client has stopped")]
    Closed,
}

/// Handle to a running client. Cloning is cheap; the client stops when the last
/// handle is dropped.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    commands: mpsc::Sender<Command>,
    state: watch::Receiver<SessionState>,
    limiter: tokio::sync::Mutex<SearchLimiter>,
    registry: Registry,
    tokens: AtomicU32,
    search_timeout: Duration,
}

#[derive(Debug)]
enum Command {
    Search { token: u32, query: String },
}

impl Client {
    /// Start the client. Must be called inside a Tokio runtime.
    #[must_use]
    pub fn start(config: Config) -> Self {
        let (commands, command_rx) = mpsc::channel(64);
        let (state_tx, state) = watch::channel(SessionState::Connecting { attempt: 1 });
        let registry = Registry::default();
        let inner = Arc::new(Inner {
            commands,
            state,
            limiter: tokio::sync::Mutex::new(SearchLimiter::new(config.max_searches, config.search_window)),
            registry: registry.clone(),
            tokens: AtomicU32::new(initial_token()),
            search_timeout: config.search_timeout,
        });
        tokio::spawn(supervise(config, command_rx, state_tx, registry));
        Self { inner }
    }

    /// Subscribe to session state changes.
    #[must_use]
    pub fn state(&self) -> watch::Receiver<SessionState> {
        self.inner.state.clone()
    }

    /// Search the network. Waits for the rate limiter if needed, then returns a
    /// [`Search`] that yields responses as peers send them.
    pub async fn search(&self, query: &str) -> Result<Search, Error> {
        let query = query.trim();
        if query.is_empty() {
            return Err(Error::EmptyQuery);
        }
        self.ensure_online()?;

        loop {
            let wait = self.inner.limiter.lock().await.try_acquire(Instant::now());
            match wait {
                Ok(()) => break,
                Err(delay) => {
                    tracing::info!(?delay, "search rate limit reached; waiting");
                    sleep(delay).await;
                }
            }
        }
        self.ensure_online()?;

        let token = self.inner.tokens.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(256);
        self.inner.registry.insert(token, tx);
        let guard = SearchGuard { token, registry: self.inner.registry.clone() };

        self.inner
            .commands
            .send(Command::Search { token, query: query.to_owned() })
            .await
            .map_err(|_| Error::Closed)?;

        Ok(Search { token, rx, deadline: Instant::now() + self.inner.search_timeout, _guard: guard })
    }

    fn ensure_online(&self) -> Result<(), Error> {
        match &*self.inner.state.borrow() {
            SessionState::Online { .. } => Ok(()),
            other => Err(Error::NotOnline(other.clone())),
        }
    }
}

/// An in-flight search. Dropping it stops collecting responses.
#[derive(Debug)]
pub struct Search {
    token: u32,
    rx: mpsc::Receiver<SearchResponse>,
    deadline: Instant,
    _guard: SearchGuard,
}

impl Search {
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }

    /// The next response, or `None` once the search has timed out.
    pub async fn next(&mut self) -> Option<SearchResponse> {
        tokio::time::timeout_at(self.deadline, self.rx.recv()).await.ok().flatten()
    }
}

#[derive(Debug)]
struct SearchGuard {
    token: u32,
    registry: Registry,
}

impl Drop for SearchGuard {
    fn drop(&mut self) {
        self.registry.remove(self.token);
    }
}

/// Routes search responses from peer connections to the search that asked.
#[derive(Debug, Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<HashMap<u32, mpsc::Sender<SearchResponse>>>>);

impl Registry {
    fn insert(&self, token: u32, tx: mpsc::Sender<SearchResponse>) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(token, tx);
    }

    fn remove(&self, token: u32) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&token);
    }

    pub(crate) fn deliver(&self, response: SearchResponse) {
        if response.files.is_empty() {
            return;
        }
        let tx = self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&response.token).cloned();
        if let Some(tx) = tx {
            // A full channel means the consumer is far behind; dropping is kinder than
            // stalling every peer connection.
            let _ = tx.try_send(response);
        } else {
            tracing::trace!(token = response.token, "response for unknown or finished search");
        }
    }
}

/// Shared by every peer connection task.
#[derive(Debug, Clone)]
pub(crate) struct PeerContext {
    pub registry: Registry,
    pub permits: Arc<Semaphore>,
}

fn initial_token() -> u32 {
    // Tokens only need to be unique within this client; start somewhere arbitrary so
    // restarts don't reuse recent tokens.
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() | 1)
}

enum SessionEnd {
    Shutdown,
    Stopped(StopReason),
    Lost { reason: String, was_online: bool },
}

async fn supervise(
    config: Config,
    mut commands: mpsc::Receiver<Command>,
    state: watch::Sender<SessionState>,
    registry: Registry,
) {
    let ctx = PeerContext { registry, permits: Arc::new(Semaphore::new(config.max_peer_connections)) };

    let (listen_port, listener_task) = match config.listen_port {
        Some(port) => match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await {
            Ok(listener) => {
                let port = listener.local_addr().ok().map(|a| a.port());
                (port, Some(tokio::spawn(connection::accept_loop(listener, ctx.clone()))))
            }
            Err(error) => {
                tracing::warn!(port, %error, "can't listen for peers; results will be slower and fewer");
                (None, None)
            }
        },
        None => (None, None),
    };

    let mut attempt = 1;
    loop {
        state.send_replace(SessionState::Connecting { attempt });
        match run_session(&config, listen_port, &mut commands, &state, &ctx).await {
            SessionEnd::Shutdown => {
                state.send_replace(SessionState::Stopped(StopReason::Shutdown));
                break;
            }
            SessionEnd::Stopped(reason) => {
                tracing::warn!(?reason, "Soulseek session stopped");
                state.send_replace(SessionState::Stopped(reason));
                // Keep draining so callers get `Closed`-style errors instead of hanging.
                while commands.recv().await.is_some() {}
                break;
            }
            SessionEnd::Lost { reason, was_online } => {
                if was_online {
                    attempt = 1;
                }
                let retry_in = backoff(config.reconnect_base, attempt);
                tracing::warn!(%reason, ?retry_in, "lost Soulseek connection");
                state.send_replace(SessionState::Reconnecting { reason, retry_in });
                attempt += 1;

                let wake = sleep(retry_in);
                tokio::pin!(wake);
                loop {
                    tokio::select! {
                        () = &mut wake => break,
                        cmd = commands.recv() => match cmd {
                            None => {
                                state.send_replace(SessionState::Stopped(StopReason::Shutdown));
                                if let Some(task) = listener_task { task.abort(); }
                                return;
                            }
                            // Searches are refused while offline, so this can only be a
                            // race with the connection dropping; the search simply times out.
                            Some(Command::Search { .. }) => {}
                        },
                    }
                }
            }
        }
    }
    if let Some(task) = listener_task {
        task.abort();
    }
}

fn backoff(base: Duration, attempt: u32) -> Duration {
    let factor = 1u32 << attempt.saturating_sub(1).min(16);
    base.saturating_mul(factor).min(Duration::from_secs(300))
}

async fn run_session(
    config: &Config,
    listen_port: Option<u16>,
    commands: &mut mpsc::Receiver<Command>,
    state: &watch::Sender<SessionState>,
    ctx: &PeerContext,
) -> SessionEnd {
    let lost = |reason: String| SessionEnd::Lost { reason, was_online: false };

    let stream = match timeout(CONNECT_TIMEOUT, TcpStream::connect(&config.server)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => return lost(format!("can't connect to {}: {e}", config.server)),
        Err(_) => return lost(format!("timed out connecting to {}", config.server)),
    };
    let mut server = Framed::new(stream, FrameCodec::new(MAX_SERVER_FRAME));

    let login = ServerRequest::Login { username: config.username.clone(), password: config.password.clone() };
    if let Err(e) = server.send(login.encode()).await {
        return lost(format!("login failed to send: {e}"));
    }

    let (public_ip, supporter) = loop {
        let frame = match timeout(CONNECT_TIMEOUT, server.next()).await {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(e))) => return lost(format!("login failed: {e}")),
            Ok(None) => return lost("server closed the connection during login".into()),
            Err(_) => return lost("server didn't answer the login".into()),
        };
        match decode_server(&frame) {
            Some(ServerEvent::LoginOk { public_ip, supporter, greeting }) => {
                tracing::info!(%public_ip, supporter, greeting = %greeting.trim(), "logged in to Soulseek");
                break (public_ip, supporter);
            }
            Some(ServerEvent::LoginRejected(reason)) => {
                return SessionEnd::Stopped(StopReason::LoginRejected(reason));
            }
            // Nothing else should arrive before the login reply; ignore it if it does.
            _ => {}
        }
    };

    let mut greeting = vec![
        ServerRequest::SharedFoldersFiles { folders: 0, files: 0 },
        ServerRequest::HaveNoParent(true),
        ServerRequest::SetStatus(Status::Online),
    ];
    if let Some(port) = listen_port {
        greeting.insert(0, ServerRequest::SetWaitPort { port: u32::from(port) });
    }
    for request in greeting {
        if let Err(e) = server.send(request.encode()).await {
            return lost(format!("connection dropped after login: {e}"));
        }
    }

    state.send_replace(SessionState::Online { public_ip, listen_port, supporter });
    let lost = |reason: String| SessionEnd::Lost { reason, was_online: true };

    // Peer tasks report failed indirect connections back to the server through this.
    let (server_out, mut server_out_rx) = mpsc::channel::<ServerRequest>(256);
    let mut ping = tokio::time::interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);

    loop {
        let outgoing = tokio::select! {
            frame = server.next() => match frame {
                Some(Ok(frame)) => {
                    match decode_server(&frame) {
                        Some(ServerEvent::ConnectToPeer { username, kind: Some(ConnectionType::Peer), ip, port, token }) => {
                            tracing::trace!(%username, %ip, port, token, "server asks us to connect to peer");
                            let Ok(port) = u16::try_from(port) else { continue };
                            tokio::spawn(connection::pierce(
                                SocketAddr::from((ip, port)),
                                token,
                                username,
                                ctx.clone(),
                                server_out.clone(),
                            ));
                        }
                        Some(ServerEvent::Relogged) => return SessionEnd::Stopped(StopReason::LoggedInElsewhere),
                        Some(other) => tracing::trace!(?other, "server message"),
                        None => {}
                    }
                    continue;
                }
                Some(Err(e)) => return lost(format!("server connection error: {e}")),
                None => return lost("server closed the connection".into()),
            },
            cmd = commands.recv() => match cmd {
                None => return SessionEnd::Shutdown,
                Some(Command::Search { token, query }) => {
                    tracing::debug!(token, %query, "searching");
                    ServerRequest::FileSearch { token, query }
                }
            },
            Some(request) = server_out_rx.recv() => request,
            _ = ping.tick() => ServerRequest::Ping,
        };
        if let Err(e) = server.send(outgoing.encode()).await {
            return lost(format!("server connection error: {e}"));
        }
    }
}

fn decode_server(frame: &[u8]) -> Option<ServerEvent> {
    let decoded = split_code(frame).and_then(|(code, body)| ServerEvent::decode(code, body));
    match decoded {
        Ok(event) => Some(event),
        Err(error) => {
            tracing::debug!(%error, "undecodable server message");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_secs(5);
        assert_eq!(backoff(base, 1), Duration::from_secs(5));
        assert_eq!(backoff(base, 2), Duration::from_secs(10));
        assert_eq!(backoff(base, 4), Duration::from_secs(40));
        assert_eq!(backoff(base, 50), Duration::from_secs(300));
    }

    fn response(token: u32, files: usize) -> SearchResponse {
        SearchResponse {
            username: "peer".into(),
            token,
            files: (0..files)
                .map(|i| crate::peer::SharedFile {
                    path: format!("a\\{i}.flac"),
                    size: 1,
                    extension: "flac".into(),
                    bitrate_kbps: None,
                    duration_secs: None,
                    vbr: false,
                    sample_rate: None,
                    bit_depth: None,
                })
                .collect(),
            free_slot: true,
            avg_speed: 0,
            queue_length: 0,
            private_files: vec![],
        }
    }

    #[tokio::test]
    async fn registry_routes_by_token_and_skips_empty() {
        let registry = Registry::default();
        let (tx, mut rx) = mpsc::channel(4);
        registry.insert(7, tx);
        registry.deliver(response(7, 0));
        registry.deliver(response(8, 1));
        registry.deliver(response(7, 2));
        assert_eq!(rx.recv().await.unwrap().files.len(), 2);
        assert!(rx.try_recv().is_err());
        registry.remove(7);
        registry.deliver(response(7, 1));
        assert!(rx.try_recv().is_err());
    }
}
