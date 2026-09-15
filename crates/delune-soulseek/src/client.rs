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
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::{Instant, sleep, timeout};
use tokio_util::codec::Framed;

use crate::connection::{self, PeerError, Shared};
use crate::frame::{FrameCodec, MAX_SERVER_FRAME, split_code};
use crate::limiter::{DEFAULT_MAX_SEARCHES, DEFAULT_WINDOW, SearchLimiter};
use crate::peer::{PeerMessage, SearchResponse};
use crate::server::{
    ConnectionType, LoginRejection, RoomMember, RoomSummary, ServerEvent, ServerRequest, Status, UserPresence,
};
use crate::shares::{FolderContents, SharedFileList, UserInfo};
use crate::transfer::{self, Download, DownloadRequest};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const PING_INTERVAL: Duration = Duration::from_secs(300);
const BROWSE_TIMEOUT: Duration = Duration::from_secs(90);
const USER_INFO_TIMEOUT: Duration = Duration::from_secs(20);

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

/// Chat activity on the account: private messages and the rooms it's in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    PrivateMessage { timestamp: u32, username: String, message: String },
    JoinedRoom { room: String, members: Vec<RoomMember> },
    LeftRoom { room: String },
    RoomMessage { room: String, username: String, message: String },
    UserJoinedRoom { room: String, member: RoomMember },
    UserLeftRoom { room: String, username: String },
    RoomList(Vec<RoomSummary>),
    UserStatus { username: String, status: Status },
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
    shared: Arc<Shared>,
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
        let shared = Arc::new(Shared::new(config.username.clone(), config.max_peer_connections, initial_token()));
        let inner = Arc::new(Inner {
            commands,
            state,
            limiter: tokio::sync::Mutex::new(SearchLimiter::new(config.max_searches, config.search_window)),
            shared: shared.clone(),
            search_timeout: config.search_timeout,
        });
        tokio::spawn(supervise(config, command_rx, state_tx, shared));
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

        let token = self.inner.shared.next_token();
        let (tx, rx) = mpsc::channel(256);
        self.inner.shared.registry.insert(token, tx);
        let guard = SearchGuard { token, shared: self.inner.shared.clone() };

        self.inner
            .commands
            .send(Command::Search { token, query: query.to_owned() })
            .await
            .map_err(|_| Error::Closed)?;

        Ok(Search { token, rx, deadline: Instant::now() + self.inner.search_timeout, _guard: guard })
    }

    /// Download one file from a peer. Progress is reported through the returned
    /// [`Download`]; connection problems and dropped transfers are retried.
    #[must_use]
    pub fn download(&self, request: DownloadRequest) -> Download {
        transfer::start(self.inner.shared.clone(), request)
    }

    /// Everything `username` shares. Large libraries can take a minute to arrive.
    ///
    /// # Errors
    ///
    /// When we're offline, the user can't be reached, or they don't answer in time.
    pub async fn browse(&self, username: &str) -> Result<Arc<SharedFileList>, PeerError> {
        let shared = &self.inner.shared;
        let answer = shared.share_lists.register(username.to_owned());
        let peer = connection::connect_peer(shared, username).await?;
        peer.send(PeerMessage::SharedFileListRequest.encode())
            .await
            .map_err(|_| PeerError::Unreachable(username.into()))?;
        timeout(BROWSE_TIMEOUT, answer)
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| PeerError::TimedOut(username.into()))
    }

    /// One folder of `username`'s shares, with its subfolders.
    ///
    /// # Errors
    ///
    /// As for [`Client::browse`].
    pub async fn folder_contents(&self, username: &str, folder: &str) -> Result<Arc<FolderContents>, PeerError> {
        let shared = &self.inner.shared;
        let token = shared.next_token();
        let answer = shared.folders.register(token);
        let peer = connection::connect_peer(shared, username).await?;
        let request = PeerMessage::FolderContentsRequest { token, folder: folder.to_owned() };
        peer.send(request.encode()).await.map_err(|_| PeerError::Unreachable(username.into()))?;
        timeout(BROWSE_TIMEOUT, answer)
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| PeerError::TimedOut(username.into()))
    }

    /// `username`'s profile: description, picture and upload slots.
    ///
    /// # Errors
    ///
    /// As for [`Client::browse`].
    pub async fn user_info(&self, username: &str) -> Result<UserInfo, PeerError> {
        let shared = &self.inner.shared;
        let answer = shared.user_infos.register(username.to_owned());
        let peer = connection::connect_peer(shared, username).await?;
        peer.send(PeerMessage::UserInfoRequest.encode()).await.map_err(|_| PeerError::Unreachable(username.into()))?;
        timeout(USER_INFO_TIMEOUT, answer)
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| PeerError::TimedOut(username.into()))
    }

    /// Whether `username` exists and is online, with the server's stats for them.
    ///
    /// # Errors
    ///
    /// When we're offline or the server doesn't answer.
    pub async fn user_presence(&self, username: &str) -> Result<UserPresence, PeerError> {
        let shared = &self.inner.shared;
        let server = shared.server().ok_or(PeerError::Offline)?;
        let answer = shared.presences.register(username.to_owned());
        server
            .send(ServerRequest::WatchUser { username: username.to_owned() })
            .await
            .map_err(|_| PeerError::Offline)?;
        let presence = timeout(USER_INFO_TIMEOUT, answer)
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| PeerError::TimedOut("The Soulseek server".into()))?;
        // One answer is all we want; don't keep receiving their status changes.
        let _ = server.send(ServerRequest::UnwatchUser { username: username.to_owned() }).await;
        Ok(presence)
    }

    /// Chat activity from now on. Slow listeners miss old events rather than stall the client.
    #[must_use]
    pub fn chat_events(&self) -> broadcast::Receiver<ChatEvent> {
        self.inner.shared.chat.subscribe()
    }

    fn to_server(&self, request: ServerRequest) -> Result<(), Error> {
        self.ensure_online()?;
        let server = self.inner.shared.server().ok_or(Error::Closed)?;
        server.try_send(request).map_err(|_| Error::Closed)
    }

    /// Send a private message.
    ///
    /// # Errors
    ///
    /// When offline, or the message is empty.
    pub fn send_message(&self, username: &str, message: &str) -> Result<(), Error> {
        if message.trim().is_empty() {
            return Err(Error::EmptyQuery);
        }
        self.to_server(ServerRequest::MessageUser { username: username.to_owned(), message: message.to_owned() })
    }

    /// Join a chat room, now and after every reconnect.
    ///
    /// # Errors
    ///
    /// When offline.
    pub fn join_room(&self, room: &str) -> Result<(), Error> {
        self.inner.shared.rooms.lock().unwrap_or_else(PoisonError::into_inner).insert(room.to_owned());
        self.to_server(ServerRequest::JoinRoom { room: room.to_owned() })
    }

    /// Leave a chat room.
    ///
    /// # Errors
    ///
    /// When offline (the room is still forgotten).
    pub fn leave_room(&self, room: &str) -> Result<(), Error> {
        self.inner.shared.rooms.lock().unwrap_or_else(PoisonError::into_inner).remove(room);
        self.to_server(ServerRequest::LeaveRoom { room: room.to_owned() })
    }

    /// Say something in a room we're in.
    ///
    /// # Errors
    ///
    /// When offline, or the message is empty.
    pub fn say(&self, room: &str, message: &str) -> Result<(), Error> {
        if message.trim().is_empty() {
            return Err(Error::EmptyQuery);
        }
        self.to_server(ServerRequest::SayChatroom { room: room.to_owned(), message: message.to_owned() })
    }

    /// Ask for the public room list; it arrives as [`ChatEvent::RoomList`].
    ///
    /// # Errors
    ///
    /// When offline.
    pub fn request_room_list(&self) -> Result<(), Error> {
        self.to_server(ServerRequest::RoomList)
    }

    /// Replace what we answer to people browsing us.
    pub fn set_shares(&self, shares: SharedFileList) {
        *self.inner.shared.own_shares.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(shares);
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
    shared: Arc<Shared>,
}

impl Drop for SearchGuard {
    fn drop(&mut self) {
        self.shared.registry.remove(self.token);
    }
}

/// Routes search responses from peer connections to the search that asked.
#[derive(Debug, Clone, Default)]
pub(crate) struct Registry(Arc<Mutex<HashMap<u32, mpsc::Sender<SearchResponse>>>>);

impl Registry {
    pub(crate) fn insert(&self, token: u32, tx: mpsc::Sender<SearchResponse>) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(token, tx);
    }

    pub(crate) fn remove(&self, token: u32) {
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
    shared: Arc<Shared>,
) {
    let (listen_port, listener_task) = match config.listen_port {
        Some(port) => match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await {
            Ok(listener) => {
                let port = listener.local_addr().ok().map(|a| a.port());
                (port, Some(tokio::spawn(connection::accept_loop(listener, shared.clone()))))
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
        let end = run_session(&config, listen_port, &mut commands, &state, &shared).await;
        shared.set_server(None);
        match end {
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
    shared: &Arc<Shared>,
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
        ServerRequest::RoomList,
    ];
    let rooms: Vec<String> = shared.rooms.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect();
    greeting.extend(rooms.into_iter().map(|room| ServerRequest::JoinRoom { room }));
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

    // Connection and transfer tasks talk to the server through this outbox.
    let (server_out, mut server_out_rx) = mpsc::channel::<ServerRequest>(256);
    shared.set_server(Some(server_out));
    let mut ping = tokio::time::interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);

    loop {
        let outgoing = tokio::select! {
            frame = server.next() => match frame {
                Some(Ok(frame)) => {
                    if let Some(event) = decode_server(&frame)
                        && let Some(end) = on_server_event(event, shared)
                    {
                        return end;
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

/// Act on one server message during a session. Returns how the session ends, if it does.
fn on_server_event(event: ServerEvent, shared: &Arc<Shared>) -> Option<SessionEnd> {
    let chat = |event: ChatEvent| {
        let _ = shared.chat.send(event);
    };
    match event {
        ServerEvent::ConnectToPeer {
            username,
            kind: Some(kind @ (ConnectionType::Peer | ConnectionType::File)),
            ip,
            port,
            token,
        } => {
            tracing::trace!(%username, %ip, port, token, ?kind, "server asks us to connect to peer");
            if let Ok(port) = u16::try_from(port) {
                tokio::spawn(connection::pierce(SocketAddr::from((ip, port)), token, username, kind, shared.clone()));
            }
        }
        ServerEvent::PeerAddress { username, ip, port } => {
            if let Ok(port) = u16::try_from(port) {
                shared.deliver_address(&username, SocketAddr::from((ip, port)));
            }
        }
        ServerEvent::Relogged => return Some(SessionEnd::Stopped(StopReason::LoggedInElsewhere)),
        ServerEvent::WatchedUser(presence) => {
            let username = presence.username.clone();
            shared.presences.deliver(&username, presence);
        }
        ServerEvent::PrivateMessage { id, timestamp, username, message, .. } => {
            // Acknowledge, or the server keeps resending it.
            if let Some(outbox) = shared.server() {
                let _ = outbox.try_send(ServerRequest::MessageAcked { id });
            }
            chat(ChatEvent::PrivateMessage { timestamp, username, message });
        }
        ServerEvent::JoinedRoom { room, members } => chat(ChatEvent::JoinedRoom { room, members }),
        ServerEvent::LeftRoom { room } => chat(ChatEvent::LeftRoom { room }),
        ServerEvent::RoomMessage { room, username, message } => {
            chat(ChatEvent::RoomMessage { room, username, message });
        }
        ServerEvent::UserJoinedRoom { room, member } => chat(ChatEvent::UserJoinedRoom { room, member }),
        ServerEvent::UserLeftRoom { room, username } => chat(ChatEvent::UserLeftRoom { room, username }),
        ServerEvent::RoomList(rooms) => chat(ChatEvent::RoomList(rooms)),
        ServerEvent::UserStatus { username, status, .. } => chat(ChatEvent::UserStatus { username, status }),
        other => tracing::trace!(?other, "server message"),
    }
    None
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
