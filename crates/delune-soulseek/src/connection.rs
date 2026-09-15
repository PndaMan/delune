//! Peer connections.
//!
//! Every conversation with another user happens over a peer ("P") connection, and
//! every file arrives over a file ("F") connection. Connections get established in
//! one of two ways:
//!
//! 1. **Direct**: one side connects to the other's listening port and opens with
//!    `PeerInit`.
//! 2. **Indirect**: when the direct attempt can't get through (usually NAT), the
//!    initiator asks the server to pass a token to the other side, which connects
//!    back and opens with `PierceFirewall` carrying that token.
//!
//! [`connect_peer`] runs both at once, as modern clients do, and returns whichever
//! connects first. Established P connections are shared: one per user, reused for
//! every message until the peer goes quiet. Every read has a timeout and P
//! connections hold a semaphore permit, so misbehaving peers can't exhaust sockets
//! or memory.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, oneshot, watch};
use tokio::time::timeout;
use tokio_util::codec::Framed;

use crate::client::{ChatEvent, Registry};
use crate::frame::{FrameCodec, MAX_PEER_FRAME, split_code};
use crate::peer::{PeerInit, PeerMessage, SearchResponse, code};
use crate::server::{ConnectionType, ServerRequest, UserPresence};
use crate::shares::{FolderContents, SharedFileList, UserInfo};
use crate::transfer::{self, Transfers};
use crate::upload::{self, Uploads};

const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Close a peer connection after this long without a message.
const PEER_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long [`connect_peer`] waits for either connection path to succeed.
const PEER_ESTABLISH_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) type PeerStream = Framed<TcpStream, FrameCodec>;
/// Sends framed messages to one peer.
pub(crate) type PeerSender = mpsc::Sender<BytesMut>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerError {
    #[error("not connected to Soulseek")]
    Offline,
    #[error("couldn't connect to {0}; they may be offline or behind a firewall")]
    Unreachable(String),
    #[error("{0} didn't answer in time")]
    TimedOut(String),
    #[error("there's no Soulseek user called {0}")]
    NoSuchUser(String),
}

/// Callers waiting for an answer keyed by `K` (a username or a token).
#[derive(Debug)]
pub(crate) struct Waiters<K, V>(Mutex<HashMap<K, Vec<oneshot::Sender<V>>>>);

impl<K, V> Default for Waiters<K, V> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

impl<K: std::hash::Hash + Eq, V: Clone> Waiters<K, V> {
    pub fn register(&self, key: K) -> oneshot::Receiver<V> {
        let (tx, rx) = oneshot::channel();
        lock(&self.0).entry(key).or_default().push(tx);
        rx
    }

    /// Hand `value` to everyone waiting on `key`. Returns whether anyone was.
    pub fn deliver(&self, key: &K, value: V) -> bool {
        let waiting = lock(&self.0).remove(key).unwrap_or_default();
        let any = !waiting.is_empty();
        for tx in waiting {
            let _ = tx.send(value.clone());
        }
        any
    }
}

/// State shared by the session, every connection task and every transfer.
#[derive(Debug)]
pub(crate) struct Shared {
    pub own_username: String,
    pub registry: Registry,
    pub transfers: Transfers,
    permits: Arc<Semaphore>,
    tokens: AtomicU32,
    /// Outbox of the current server session, when there is one.
    server: Mutex<Option<mpsc::Sender<ServerRequest>>>,
    peers: Mutex<HashMap<String, PeerSender>>,
    peer_waiters: Mutex<HashMap<String, Vec<oneshot::Sender<PeerSender>>>>,
    /// Tokens we sent in `ConnectToPeer`, awaiting a `PierceFirewall` from that user.
    pending_indirect: Mutex<HashMap<u32, String>>,
    address_waiters: Mutex<HashMap<String, Vec<oneshot::Sender<SocketAddr>>>>,
    pub share_lists: Waiters<String, Arc<SharedFileList>>,
    pub user_infos: Waiters<String, UserInfo>,
    pub folders: Waiters<u32, Arc<FolderContents>>,
    pub presences: Waiters<String, UserPresence>,
    /// What we share, who's downloading it, and who's waiting.
    pub uploads: Uploads,
    /// Private messages and room activity, for whoever is listening.
    pub chat: broadcast::Sender<ChatEvent>,
    /// Whether we have a parent in the distributed search network.
    pub distributed_parent: AtomicBool,
    /// Whether we're trying possible parents right now.
    pub distributed_connecting: AtomicBool,
    /// Bumped to drop the current distributed parent.
    pub distributed_reset: watch::Sender<u64>,
    /// Seconds between wishlist searches, as the server says.
    pub wishlist_interval: AtomicU32,
    /// Rooms to be in, rejoined after every reconnect.
    pub rooms: Mutex<std::collections::BTreeSet<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub fn new(own_username: String, max_peer_connections: usize, first_token: u32) -> Self {
        Self {
            own_username,
            registry: Registry::default(),
            transfers: Transfers::default(),
            permits: Arc::new(Semaphore::new(max_peer_connections)),
            tokens: AtomicU32::new(first_token),
            server: Mutex::default(),
            peers: Mutex::default(),
            peer_waiters: Mutex::default(),
            pending_indirect: Mutex::default(),
            address_waiters: Mutex::default(),
            share_lists: Waiters::default(),
            user_infos: Waiters::default(),
            folders: Waiters::default(),
            presences: Waiters::default(),
            uploads: Uploads::default(),
            chat: broadcast::channel(512).0,
            rooms: Mutex::default(),
            wishlist_interval: AtomicU32::new(12 * 60),
            distributed_parent: AtomicBool::new(false),
            distributed_connecting: AtomicBool::new(false),
            distributed_reset: watch::channel(0).0,
        }
    }

    pub fn next_token(&self) -> u32 {
        self.tokens.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_server(&self, outbox: Option<mpsc::Sender<ServerRequest>>) {
        *lock(&self.server) = outbox;
    }

    pub fn server(&self) -> Option<mpsc::Sender<ServerRequest>> {
        lock(&self.server).clone()
    }

    fn peer(&self, username: &str) -> Option<PeerSender> {
        lock(&self.peers).get(username).filter(|tx| !tx.is_closed()).cloned()
    }

    fn register_peer(&self, username: &str, tx: &PeerSender) {
        lock(&self.peers).insert(username.to_owned(), tx.clone());
        for waiter in lock(&self.peer_waiters).remove(username).unwrap_or_default() {
            let _ = waiter.send(tx.clone());
        }
    }

    fn unregister_peer(&self, username: &str, tx: &PeerSender) {
        let mut peers = lock(&self.peers);
        if peers.get(username).is_some_and(|current| current.same_channel(tx)) {
            peers.remove(username);
        }
    }

    pub fn deliver_address(&self, username: &str, addr: SocketAddr) {
        for waiter in lock(&self.address_waiters).remove(username).unwrap_or_default() {
            let _ = waiter.send(addr);
        }
    }
}

/// Accept incoming connections until the task is aborted.
pub(crate) async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                tokio::spawn(handle_incoming(stream, addr, shared.clone()));
            }
            Err(error) => {
                // Usually file-descriptor exhaustion; pause instead of spinning.
                tracing::warn!(%error, "failed to accept peer connection");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn handle_incoming(stream: TcpStream, addr: SocketAddr, shared: Arc<Shared>) {
    let mut conn = Framed::new(stream, FrameCodec::new(MAX_PEER_FRAME));
    let Ok(Some(Ok(first))) = timeout(PEER_IDLE_TIMEOUT, conn.next()).await else { return };

    match PeerInit::decode(&first) {
        Ok(PeerInit::PeerInit { username, kind, .. }) if kind == "P" => {
            let Ok(permit) = shared.permits.clone().try_acquire_owned() else {
                tracing::debug!(%addr, "too many peer connections; refusing");
                return;
            };
            tracing::trace!(%addr, %username, "direct peer connection");
            run_peer(username, conn, shared, permit).await;
        }
        Ok(PeerInit::PeerInit { username, kind, .. }) if kind == "F" => {
            tracing::trace!(%addr, %username, "direct file connection");
            transfer::accept_file_connection(conn, &shared).await;
        }
        Ok(PeerInit::PierceFirewall { token }) => {
            let username = lock(&shared.pending_indirect).remove(&token);
            if let Some(username) = username {
                let Ok(permit) = shared.permits.clone().try_acquire_owned() else { return };
                tracing::trace!(%addr, %username, "peer answered our indirect request");
                run_peer(username, conn, shared, permit).await;
            } else {
                let parts = conn.into_parts();
                if !upload::on_pierced_file_connection(&shared, token, parts.io, parts.read_buf) {
                    tracing::trace!(%addr, token, "PierceFirewall with unknown token");
                }
            }
        }
        // We don't accept distributed children.
        Ok(other) => tracing::trace!(%addr, ?other, "ignoring connection"),
        Err(error) => tracing::debug!(%addr, %error, "bad peer init"),
    }
}

/// Answer a `ConnectToPeer` from the server by dialling the peer ourselves.
pub(crate) async fn pierce(addr: SocketAddr, token: u32, username: String, kind: ConnectionType, shared: Arc<Shared>) {
    let permit = match kind {
        ConnectionType::Peer => match shared.permits.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => return,
        },
        // File connections carry downloads we asked for; never refuse them.
        _ => None,
    };

    let Ok(Ok(stream)) = timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(addr)).await else {
        tracing::trace!(%addr, %username, ?kind, "can't reach peer");
        if let Some(server) = shared.server() {
            let _ = server.try_send(ServerRequest::CantConnectToPeer { token, username });
        }
        return;
    };
    let mut conn = Framed::new(stream, FrameCodec::new(MAX_PEER_FRAME));
    if conn.send(PeerInit::PierceFirewall { token }.encode()).await.is_err() {
        return;
    }
    match (kind, permit) {
        (ConnectionType::Peer, Some(permit)) => run_peer(username, conn, shared, permit).await,
        (ConnectionType::File, _) => transfer::accept_file_connection(conn, &shared).await,
        _ => {}
    }
}

/// Ask the server where `username` listens. The answer arrives on the receiver.
pub(crate) fn request_address(shared: &Shared, username: &str) -> oneshot::Receiver<SocketAddr> {
    let (tx, rx) = oneshot::channel();
    lock(&shared.address_waiters).entry(username.to_owned()).or_default().push(tx);
    if let Some(server) = shared.server() {
        let _ = server.try_send(ServerRequest::GetPeerAddress { username: username.to_owned() });
    }
    rx
}

/// Get a connection to `username`, reusing an open one or establishing a new one
/// directly and indirectly at the same time.
pub(crate) async fn connect_peer(shared: &Arc<Shared>, username: &str) -> Result<PeerSender, PeerError> {
    if let Some(tx) = shared.peer(username) {
        return Ok(tx);
    }
    let server = shared.server().ok_or(PeerError::Offline)?;

    let (ready_tx, ready_rx) = oneshot::channel();
    let already_connecting = {
        let mut waiters = lock(&shared.peer_waiters);
        let queue = waiters.entry(username.to_owned()).or_default();
        queue.push(ready_tx);
        queue.len() > 1
    };
    // Another download is already connecting to this user: wait for that attempt
    // instead of opening a second connection.
    if already_connecting {
        return match timeout(PEER_ESTABLISH_TIMEOUT, ready_rx).await {
            Ok(Ok(tx)) => Ok(tx),
            _ => Err(PeerError::Unreachable(username.to_owned())),
        };
    }

    let token = shared.next_token();
    lock(&shared.pending_indirect).insert(token, username.to_owned());
    let (address_tx, address_rx) = oneshot::channel();
    lock(&shared.address_waiters).entry(username.to_owned()).or_default().push(address_tx);

    let _ = server.send(ServerRequest::GetPeerAddress { username: username.to_owned() }).await;
    let _ = server
        .send(ServerRequest::ConnectToPeer { token, username: username.to_owned(), kind: ConnectionType::Peer })
        .await;

    // The direct attempt runs on its own: if it connects after we've given up, the
    // connection is still registered and reused next time.
    {
        let shared = shared.clone();
        let username = username.to_owned();
        tokio::spawn(async move {
            let Ok(Ok(addr)) = timeout(PEER_CONNECT_TIMEOUT, address_rx).await else { return };
            if addr.port() == 0 || addr.ip().is_unspecified() {
                return;
            }
            let Ok(Ok(stream)) = timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(addr)).await else { return };
            let mut conn = Framed::new(stream, FrameCodec::new(MAX_PEER_FRAME));
            let init = PeerInit::PeerInit { username: shared.own_username.clone(), kind: "P".into(), token: 0 };
            if conn.send(init.encode()).await.is_err() {
                return;
            }
            let Ok(permit) = shared.permits.clone().try_acquire_owned() else { return };
            tracing::trace!(%addr, %username, "connected to peer directly");
            run_peer(username, conn, shared, permit).await;
        });
    }

    let result = timeout(PEER_ESTABLISH_TIMEOUT, ready_rx).await;
    lock(&shared.pending_indirect).remove(&token);
    if result.is_err() {
        // Give up for everyone waiting on this attempt, so the next try starts fresh.
        lock(&shared.peer_waiters).remove(username);
    }
    match result {
        Ok(Ok(tx)) => Ok(tx),
        _ => Err(PeerError::Unreachable(username.to_owned())),
    }
}

/// Serve one P connection: register it for reuse, write queued messages, and route
/// everything the peer sends.
async fn run_peer(username: String, conn: PeerStream, shared: Arc<Shared>, _permit: OwnedSemaphorePermit) {
    let (mut sink, mut stream) = conn.split();
    let (tx, mut rx) = mpsc::channel::<BytesMut>(32);
    shared.register_peer(&username, &tx);

    let writer = async {
        while let Some(message) = rx.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    };
    let reader = async {
        while let Ok(Some(Ok(frame))) = timeout(PEER_IDLE_TIMEOUT, stream.next()).await {
            let Ok((message_code, body)) = split_code(&frame) else { break };
            if route_browse_reply(&shared, &username, message_code, body) {
                continue;
            }
            if message_code == code::SEARCH_RESPONSE {
                match SearchResponse::decode(body) {
                    Ok(response) => {
                        tracing::trace!(%username, token = response.token, files = response.files.len(), "search response");
                        shared.registry.deliver(response);
                    }
                    Err(error) => tracing::debug!(%username, %error, "bad search response"),
                }
                continue;
            }
            match PeerMessage::decode(message_code, body) {
                Ok(Some(message)) => {
                    tracing::debug!(%username, ?message, "peer message");
                    answer_browse_request(&shared, &message, &tx);
                    if !upload::on_peer_message(&shared, &username, &message, &tx) {
                        shared.transfers.on_peer_message(&username, message, &tx);
                    }
                }
                Ok(None) => tracing::trace!(%username, message_code, "unhandled peer message"),
                Err(error) => tracing::debug!(%username, message_code, %error, "bad peer message"),
            }
        }
    };

    tokio::select! {
        () = writer => {},
        () = reader => {},
    }
    shared.unregister_peer(&username, &tx);
}

/// Answers to our browse and profile requests. Returns whether `message_code` was one.
fn route_browse_reply(shared: &Shared, username: &str, message_code: u32, body: &[u8]) -> bool {
    match message_code {
        code::SHARED_FILE_LIST_RESPONSE => match SharedFileList::decode(body) {
            Ok(list) => {
                tracing::debug!(%username, files = list.file_count(), "share list");
                shared.share_lists.deliver(&username.to_owned(), Arc::new(list));
            }
            Err(error) => tracing::debug!(%username, %error, "bad share list"),
        },
        code::USER_INFO_RESPONSE => match UserInfo::decode(body) {
            Ok(info) => {
                shared.user_infos.deliver(&username.to_owned(), info);
            }
            Err(error) => tracing::debug!(%username, %error, "bad user info"),
        },
        code::FOLDER_CONTENTS_RESPONSE => match FolderContents::decode(body) {
            Ok(contents) => {
                let token = contents.token;
                shared.folders.deliver(&token, Arc::new(contents));
            }
            Err(error) => tracing::debug!(%username, %error, "bad folder contents"),
        },
        _ => return false,
    }
    true
}

/// Someone is browsing us or asking who we are.
fn answer_browse_request(shared: &Shared, message: &PeerMessage, reply: &PeerSender) {
    let response = match message {
        PeerMessage::SharedFileListRequest => shared.uploads.index().list().encode(),
        PeerMessage::FolderContentsRequest { token, folder } => {
            let ours = shared.uploads.index().list();
            let prefix = format!("{folder}\\");
            let directories =
                ours.directories.iter().filter(|d| d.path == *folder || d.path.starts_with(&prefix)).cloned().collect();
            FolderContents { token: *token, folder: folder.clone(), directories }.encode()
        }
        PeerMessage::UserInfoRequest => {
            let (slots_free, queue_size) = shared.uploads.availability();
            UserInfo { description: "delune".into(), slots_free, queue_size, ..UserInfo::default() }.encode()
        }
        _ => return,
    };
    let _ = reply.try_send(response);
}
