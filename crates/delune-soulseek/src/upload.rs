//! Uploads: sending what we share to the people who ask.
//!
//! ```text
//!  peer                                  us
//!  ── P ─ QueueUpload(file) ─────────────▶      queued if we share it
//!  ◀───── PlaceInQueueResponse(place) ───      when they ask where they are
//!  ◀───── TransferRequest(token, size) ──      when an upload slot frees up
//!  ── P ─ TransferResponse(token, ok) ───▶
//!  ◀══ F ═ PeerInit or PierceFirewall ═══      we open a file connection
//!  ◀══ F ═ token (u32, unframed) ════════
//!  ══ F ═ offset (u64, unframed) ════════▶
//!  ◀══ F ═ file bytes ═══════════════════      until they close the connection
//! ```
//!
//! Only files in the [`ShareIndex`] can be sent, looked up by their virtual path, so
//! a request can never reach anything else on disk. Uploads run a few at a time
//! (one per person), and everything waiting is visible to the app, which can cancel
//! uploads or refuse people outright.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::BytesMut;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Notify, oneshot, watch};
use tokio::time::{Instant, sleep, timeout};

use crate::connection::{self, PeerSender, Shared};
use crate::peer::{PeerInit, PeerMessage, SearchResponse, direction};
use crate::server::{ConnectionType, ServerRequest};
use crate::sharing::ShareIndex;

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
const FILE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const OFFSET_TIMEOUT: Duration = Duration::from_secs(30);
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);
const KEEP_FINISHED: usize = 200;

/// How generous to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadLimits {
    /// Uploads running at once (never more than one per person).
    pub slots: usize,
    /// Files one person may have waiting.
    pub queue_per_user: usize,
    /// Overall upload speed cap in bytes per second, shared by all uploads.
    pub bytes_per_second: Option<u64>,
    /// Refuse people who share nothing themselves.
    pub refuse_leechers: bool,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self { slots: 3, queue_per_user: 200, bytes_per_second: None, refuse_leechers: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadState {
    Queued,
    Connecting,
    Transferring { bytes: u64 },
    Completed { bytes: u64 },
    Failed { reason: String },
    Cancelled,
}

impl UploadState {
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled)
    }
}

/// One upload, as the app sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadInfo {
    pub id: u64,
    pub username: String,
    /// Virtual path, as they asked for it.
    pub filename: String,
    pub size: u64,
    pub state: UploadState,
    /// Unix seconds.
    pub queued_at: u64,
    /// Average speed so far, bytes per second.
    pub speed: u64,
}

#[derive(Debug)]
struct Entry {
    info: UploadInfo,
    disk_path: PathBuf,
    cancel: watch::Sender<bool>,
}

#[derive(Debug, Default)]
struct Inner {
    index: Arc<ShareIndex>,
    limits: UploadLimits,
    entries: VecDeque<Entry>,
    next_id: u64,
    banned: HashSet<String>,
    /// Answers to our `TransferRequest`s, by token.
    responses: HashMap<u32, oneshot::Sender<(bool, Option<String>)>>,
    /// Incoming `PierceFirewall` file connections for uploads, by token.
    file_connections: HashMap<u32, oneshot::Sender<(TcpStream, BytesMut)>>,
}

/// Every upload, queued, running or recently finished.
#[derive(Debug)]
pub(crate) struct Uploads {
    inner: Mutex<Inner>,
    wake: Notify,
    /// Bumped whenever anything changes, for the app to refresh.
    pub changed: watch::Sender<u64>,
}

impl Default for Uploads {
    fn default() -> Self {
        Self { inner: Mutex::default(), wake: Notify::new(), changed: watch::channel(0).0 }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Uploads {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn touch(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    pub fn index(&self) -> Arc<ShareIndex> {
        self.lock().index.clone()
    }

    pub fn set_index(&self, index: Arc<ShareIndex>) {
        self.lock().index = index;
    }

    pub fn set_limits(&self, limits: UploadLimits) {
        self.lock().limits = limits;
        self.wake.notify_one();
    }

    pub fn set_banned(&self, banned: HashSet<String>) {
        self.lock().banned = banned;
    }

    pub fn snapshot(&self) -> Vec<UploadInfo> {
        self.lock().entries.iter().map(|e| e.info.clone()).collect()
    }

    /// Whether a new request would start straight away, and how many are waiting.
    pub fn availability(&self) -> (bool, u32) {
        let inner = self.lock();
        let running = inner.entries.iter().filter(|e| is_running(&e.info.state)).count();
        let waiting = inner.entries.iter().filter(|e| e.info.state == UploadState::Queued).count();
        (running < inner.limits.slots && waiting == 0, u32::try_from(waiting).unwrap_or(u32::MAX))
    }

    pub fn cancel(&self, id: u64) -> bool {
        let mut inner = self.lock();
        let Some(entry) = inner.entries.iter_mut().find(|e| e.info.id == id) else { return false };
        if entry.info.state.is_finished() {
            return false;
        }
        if entry.info.state == UploadState::Queued {
            entry.info.state = UploadState::Cancelled;
        }
        let _ = entry.cancel.send(true);
        drop(inner);
        self.touch();
        true
    }

    /// Forget finished uploads.
    pub fn clear_finished(&self) {
        self.lock().entries.retain(|e| !e.info.state.is_finished());
        self.touch();
    }

    fn set_state(&self, id: u64, state: UploadState, speed: Option<u64>) {
        let mut inner = self.lock();
        if let Some(entry) = inner.entries.iter_mut().find(|e| e.info.id == id) {
            entry.info.state = state;
            if let Some(speed) = speed {
                entry.info.speed = speed;
            }
        }
        // Keep a short history of finished uploads.
        let finished = inner.entries.iter().filter(|e| e.info.state.is_finished()).count();
        if finished > KEEP_FINISHED
            && let Some(oldest) = inner.entries.iter().position(|e| e.info.state.is_finished())
        {
            inner.entries.remove(oldest);
        }
        drop(inner);
        self.touch();
    }

    fn place(&self, username: &str, filename: &str) -> Option<u32> {
        let inner = self.lock();
        let queued = inner.entries.iter().filter(|e| e.info.state == UploadState::Queued);
        queued
            .enumerate()
            .find(|(_, e)| e.info.username == username && e.info.filename == filename)
            .map(|(i, _)| u32::try_from(i + 1).unwrap_or(u32::MAX))
    }

    /// Someone asked for a file. Returns the denial to send, if any.
    fn enqueue(&self, username: &str, filename: &str) -> Option<&'static str> {
        let mut inner = self.lock();
        if inner.banned.contains(username) {
            return Some("Banned");
        }
        let Some(file) = inner.index.lookup(filename) else { return Some("File not shared.") };
        let (size, disk_path) = (file.file.size, file.disk_path.clone());
        let already = inner
            .entries
            .iter()
            .any(|e| e.info.username == username && e.info.filename == filename && !e.info.state.is_finished());
        if already {
            return None;
        }
        let waiting =
            inner.entries.iter().filter(|e| e.info.username == username && !e.info.state.is_finished()).count();
        if waiting >= inner.limits.queue_per_user {
            return Some("Too many files");
        }
        inner.next_id += 1;
        let info = UploadInfo {
            id: inner.next_id,
            username: username.to_owned(),
            filename: filename.to_owned(),
            size,
            state: UploadState::Queued,
            queued_at: now(),
            speed: 0,
        };
        inner.entries.push_back(Entry { info, disk_path, cancel: watch::channel(false).0 });
        drop(inner);
        self.touch();
        self.wake.notify_one();
        None
    }

    /// Queued uploads that can start now, marked as connecting.
    fn startable(&self) -> Vec<(UploadInfo, PathBuf, watch::Receiver<bool>)> {
        let mut inner = self.lock();
        let slots = inner.limits.slots;
        let mut busy: HashSet<String> =
            inner.entries.iter().filter(|e| is_running(&e.info.state)).map(|e| e.info.username.clone()).collect();
        let mut free = slots.saturating_sub(busy.len());
        let mut ready = Vec::new();
        for entry in &mut inner.entries {
            if free == 0 {
                break;
            }
            if entry.info.state != UploadState::Queued || busy.contains(&entry.info.username) {
                continue;
            }
            entry.info.state = UploadState::Connecting;
            let (cancel, rx) = watch::channel(false);
            entry.cancel = cancel;
            busy.insert(entry.info.username.clone());
            free -= 1;
            ready.push((entry.info.clone(), entry.disk_path.clone(), rx));
        }
        ready
    }
}

const fn is_running(state: &UploadState) -> bool {
    matches!(state, UploadState::Connecting | UploadState::Transferring { .. })
}

/// Handle upload-related messages from a peer. Returns whether the message was one.
pub(crate) fn on_peer_message(shared: &Shared, username: &str, message: &PeerMessage, reply: &PeerSender) -> bool {
    let uploads = &shared.uploads;
    match message {
        PeerMessage::QueueUpload { filename } => {
            if let Some(reason) = uploads.enqueue(username, filename) {
                let denial = PeerMessage::UploadDenied { filename: filename.clone(), reason: reason.to_owned() };
                let _ = reply.try_send(denial.encode());
            }
        }
        PeerMessage::PlaceInQueueRequest { filename } => {
            if let Some(place) = uploads.place(username, filename) {
                let response = PeerMessage::PlaceInQueueResponse { filename: filename.clone(), place };
                let _ = reply.try_send(response.encode());
            }
        }
        // Old clients ask to download with a TransferRequest; queue it like QueueUpload.
        PeerMessage::TransferRequest { direction: direction::DOWNLOAD, token, filename, .. } => {
            let reason = uploads.enqueue(username, filename).unwrap_or("Queued");
            let response =
                PeerMessage::TransferResponse { token: *token, allowed: false, reason: Some(reason.to_owned()) };
            let _ = reply.try_send(response.encode());
        }
        PeerMessage::TransferResponse { token, allowed, reason } => {
            let Some(waiter) = uploads.lock().responses.remove(token) else { return false };
            let _ = waiter.send((*allowed, reason.clone()));
        }
        _ => return false,
    }
    true
}

/// A `PierceFirewall` whose token might belong to one of our uploads.
pub(crate) fn on_pierced_file_connection(shared: &Shared, token: u32, stream: TcpStream, buffered: BytesMut) -> bool {
    let Some(waiter) = shared.uploads.lock().file_connections.remove(&token) else { return false };
    let _ = waiter.send((stream, buffered));
    true
}

/// Answer someone's search from our shares, if anything matches.
pub(crate) fn answer_search(shared: &Arc<Shared>, username: String, token: u32, query: &str) {
    if username == shared.own_username {
        return;
    }
    // The server asks clients not to answer searches for these, nor share files named so.
    if shared.excluded_in(query).is_some() {
        return;
    }
    let mut files = shared.uploads.index().search(query);
    files.retain(|f| shared.excluded_in(&f.path).is_none());
    if files.is_empty() {
        return;
    }
    let (free_slot, queue_length) = shared.uploads.availability();
    let response = SearchResponse {
        username: shared.own_username.clone(),
        token,
        files,
        free_slot,
        avg_speed: 0,
        queue_length,
        private_files: vec![],
    };
    let shared = shared.clone();
    tokio::spawn(async move {
        match connection::connect_peer(&shared, &username).await {
            Ok(peer) => {
                tracing::debug!(%username, files = response.files.len(), "answering search");
                let _ = peer.send(response.encode()).await;
            }
            Err(error) => tracing::trace!(%username, %error, "couldn't deliver search results"),
        }
    });
}

/// Start queued uploads as slots free up. Runs for the life of the client.
pub(crate) async fn schedule(shared: Arc<Shared>) {
    loop {
        for (info, disk_path, cancel) in shared.uploads.startable() {
            let shared = shared.clone();
            tokio::spawn(async move {
                let id = info.id;
                let outcome = run(&shared, &info, &disk_path, cancel).await;
                tracing::info!(username = %info.username, file = %info.filename, ?outcome, "upload finished");
                if let UploadState::Failed { reason } = &outcome
                    && let Ok(Ok(peer)) =
                        timeout(Duration::from_secs(10), connection::connect_peer(&shared, &info.username)).await
                {
                    let message = if reason == LEECHER_REASON {
                        PeerMessage::UploadDenied { filename: info.filename.clone(), reason: "Banned".into() }
                    } else {
                        PeerMessage::UploadFailed { filename: info.filename.clone() }
                    };
                    let _ = peer.send(message.encode()).await;
                }
                shared.uploads.set_state(id, outcome, None);
                shared.uploads.wake.notify_one();
            });
        }
        tokio::select! {
            () = shared.uploads.wake.notified() => {}
            () = sleep(Duration::from_secs(10)) => {}
        }
    }
}

/// Why an upload was refused to someone who shares nothing.
const LEECHER_REASON: &str = "They don't share anything";

async fn run(
    shared: &Arc<Shared>,
    info: &UploadInfo,
    disk_path: &Path,
    mut cancel: watch::Receiver<bool>,
) -> UploadState {
    let work = async {
        if shared.uploads.lock().limits.refuse_leechers
            && let Ok(presence) = connection::presence(shared, &info.username).await
            && presence.exists
            && presence.files == 0
        {
            return Err(LEECHER_REASON.to_owned());
        }
        let peer = connection::connect_peer(shared, &info.username).await.map_err(|e| e.to_string())?;
        let token = shared.next_token();
        let (answer_tx, answer_rx) = oneshot::channel();
        shared.uploads.lock().responses.insert(token, answer_tx);
        let request = PeerMessage::TransferRequest {
            direction: direction::UPLOAD,
            token,
            filename: info.filename.clone(),
            size: Some(info.size),
        };
        peer.send(request.encode()).await.map_err(|_| "they disconnected".to_owned())?;
        let answer = timeout(RESPONSE_TIMEOUT, answer_rx).await;
        shared.uploads.lock().responses.remove(&token);
        match answer {
            Ok(Ok((true, _))) => {}
            Ok(Ok((false, reason))) => return Err(reason.unwrap_or_else(|| "they declined".into())),
            _ => return Err("they didn't answer".into()),
        }

        let (mut stream, buffered) = open_file_connection(shared, &info.username).await?;
        stream.write_all(&token.to_le_bytes()).await.map_err(|e| e.to_string())?;
        let offset = read_offset(&mut stream, buffered).await?;
        send_file(shared, info, disk_path, &mut stream, offset).await
    };

    tokio::select! {
        result = work => match result {
            Ok(bytes) => UploadState::Completed { bytes },
            Err(reason) => UploadState::Failed { reason },
        },
        _ = cancel.wait_for(|c| *c) => UploadState::Cancelled,
    }
}

/// Connect to the downloader for file data: directly, and through the server in case
/// they can't be reached, whichever works first.
async fn open_file_connection(shared: &Arc<Shared>, username: &str) -> Result<(TcpStream, BytesMut), String> {
    let server = shared.server().ok_or("not connected to Soulseek")?;
    let indirect_token = shared.next_token();
    let (pierced_tx, pierced_rx) = oneshot::channel();
    shared.uploads.lock().file_connections.insert(indirect_token, pierced_tx);
    let address = connection::request_address(shared, username);
    let _ = server
        .send(ServerRequest::ConnectToPeer {
            token: indirect_token,
            username: username.to_owned(),
            kind: ConnectionType::File,
        })
        .await;

    let own = shared.own_username.clone();
    let direct = async move {
        let addr: SocketAddr = address.await.map_err(|_| ())?;
        if addr.port() == 0 {
            return Err(());
        }
        let mut stream = TcpStream::connect(addr).await.map_err(|_| ())?;
        let init = PeerInit::PeerInit { username: own, kind: "F".into(), token: 0 };
        stream.write_all(&init.encode()).await.map_err(|_| ())?;
        Ok::<_, ()>((stream, BytesMut::new()))
    };
    let indirect = async move { pierced_rx.await.map_err(|_| ()) };

    let result = timeout(FILE_CONNECT_TIMEOUT, async {
        tokio::pin!(direct, indirect);
        let mut direct_done = false;
        let mut indirect_done = false;
        loop {
            tokio::select! {
                r = &mut direct, if !direct_done => match r {
                    Ok(conn) => return Some(conn),
                    Err(()) => direct_done = true,
                },
                r = &mut indirect, if !indirect_done => match r {
                    Ok(conn) => return Some(conn),
                    Err(()) => indirect_done = true,
                },
            }
            if direct_done && indirect_done {
                return None;
            }
        }
    })
    .await;
    shared.uploads.lock().file_connections.remove(&indirect_token);
    result.ok().flatten().ok_or_else(|| format!("couldn't connect to {username} to send the file"))
}

async fn read_offset(stream: &mut TcpStream, mut buffered: BytesMut) -> Result<u64, String> {
    while buffered.len() < 8 {
        let mut chunk = [0u8; 8];
        match timeout(OFFSET_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => buffered.extend_from_slice(&chunk[..n]),
            _ => return Err("they didn't say where to start".into()),
        }
    }
    let mut offset = [0u8; 8];
    offset.copy_from_slice(&buffered[..8]);
    Ok(u64::from_le_bytes(offset))
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "speeds and pacing are approximate"
)]
async fn send_file(
    shared: &Shared,
    info: &UploadInfo,
    disk_path: &Path,
    stream: &mut TcpStream,
    offset: u64,
) -> Result<u64, String> {
    let mut file = File::open(disk_path).await.map_err(|e| format!("couldn't read the file: {e}"))?;
    let size = file.metadata().await.map_err(|e| e.to_string())?.len();
    // An offset past the end (or Soulseek NS's -1) means start again.
    let offset = if offset > size { 0 } else { offset };
    file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| e.to_string())?;

    let started = Instant::now();
    let mut sent = offset;
    let mut last_report = Instant::now();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut pace = shared.upload_cap.start();
    loop {
        let n = file.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        timeout(STALL_TIMEOUT, stream.write_all(&buffer[..n]))
            .await
            .map_err(|_| "the transfer stalled".to_owned())?
            .map_err(|_| "they disconnected".to_owned())?;
        sent += n as u64;
        pace.record(n).await;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            let speed = ((sent - offset) as f64 / started.elapsed().as_secs_f64().max(0.001)) as u64;
            shared.uploads.set_state(info.id, UploadState::Transferring { bytes: sent }, Some(speed));
        }
    }
    stream.flush().await.map_err(|e| e.to_string())?;

    // The downloader closes the connection once it has everything.
    let mut rest = [0u8; 64];
    let _ = timeout(Duration::from_secs(30), stream.read(&mut rest)).await;

    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let speed = ((sent - offset) as f64 / elapsed) as u32;
    if let Some(server) = shared.server() {
        let _ = server.try_send(ServerRequest::SendUploadSpeed { speed });
    }
    Ok(sent)
}
