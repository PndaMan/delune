//! Downloads.
//!
//! A download is a conversation followed by a byte stream:
//!
//! ```text
//!  us                                   peer
//!  ── P ─ QueueUpload(file) ─────────────▶
//!  ◀───── PlaceInQueueResponse(place) ───      (optional, any number of times)
//!  ◀───── TransferRequest(token, size) ──      when an upload slot frees up
//!  ── P ─ TransferResponse(token, ok) ───▶
//!  ◀══ F ═ PeerInit/PierceFirewall ══════      peer opens a file connection
//!  ◀══ F ═ token (u32, unframed) ════════
//!  ══ F ═ offset (u64, unframed) ════════▶      bytes we already have, for resuming
//!  ◀══ F ═ file bytes ═══════════════════
//! ```
//!
//! Bytes are written to `<dest>.part` and renamed to `<dest>` only once the expected
//! size has arrived, so a file at `dest` is always complete. A dropped connection
//! re-queues the file and resumes from what's already on disk.
//!
//! File connection messages are *not* length-prefixed. The frame codec used to read
//! the init message may already have buffered some of what follows, so that buffer
//! is carried into the raw phase instead of being lost.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::{Buf, BytesMut};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, timeout};

use crate::connection::{self, PeerSender, PeerStream, Shared};
use crate::peer::{PeerMessage, direction};

/// Give up on a file after this many connection attempts or dropped transfers.
const MAX_ATTEMPTS: u32 = 4;
/// After accepting a transfer, how long to wait for the file connection.
const FILE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(60);
/// A stalled transfer with no bytes for this long is dropped and re-queued.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const RETRY_DELAY: Duration = Duration::from_secs(5);
/// How long to leave a peer alone after it turns us away for being over its limits.
const QUEUE_FULL_DELAY: Duration = Duration::from_secs(90);
/// Give up on a full queue after this many waits (about half an hour).
const MAX_QUEUE_WAITS: u32 = 20;
/// Files of one job spread their waits over this window so they don't all ask at once.
const QUEUE_FULL_SPREAD: Duration = Duration::from_secs(30);

/// What to download and where to put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub username: String,
    /// The full shared path, exactly as it appeared in the search response.
    pub filename: String,
    /// Where the finished file goes. Parent directories are created.
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Connecting,
    /// Waiting in the peer's upload queue. `place` when the peer reported one.
    Queued {
        place: Option<u32>,
    },
    /// The peer is about to send; waiting for the file connection.
    Starting {
        size: Option<u64>,
    },
    Transferring {
        bytes: u64,
        size: u64,
    },
    Completed {
        bytes: u64,
    },
    Failed {
        reason: String,
    },
    Cancelled,
}

impl DownloadState {
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled)
    }
}

/// A running download. Dropping the handle doesn't cancel it; call [`Download::cancel`].
#[derive(Debug, Clone)]
pub struct Download {
    state: watch::Receiver<DownloadState>,
    cancel: mpsc::Sender<()>,
}

impl Download {
    #[must_use]
    pub fn state(&self) -> watch::Receiver<DownloadState> {
        self.state.clone()
    }

    /// Wait until the download completes, fails or is cancelled.
    pub async fn finished(&self) -> DownloadState {
        let mut state = self.state.clone();
        state.wait_for(DownloadState::is_finished).await.map_or(DownloadState::Cancelled, |s| s.clone())
    }

    pub fn cancel(&self) {
        let _ = self.cancel.try_send(());
    }
}

pub(crate) fn start(shared: Arc<Shared>, request: DownloadRequest) -> Download {
    let (state_tx, state) = watch::channel(DownloadState::Connecting);
    let (cancel, cancel_rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let key = (request.username.clone(), request.filename.clone());
        let events = shared.transfers.register(&key);
        let outcome = run(&shared, &request, events, &state_tx, cancel_rx).await;
        shared.transfers.unregister(&key);
        tracing::info!(username = %request.username, file = %request.filename, ?outcome, "download finished");
        state_tx.send_replace(outcome);
    });
    Download { state, cancel }
}

#[derive(Debug)]
enum Event {
    Place(u32),
    Denied(String),
    UploadFailed,
    Request { size: Option<u64> },
    File(FileConnection),
}

#[derive(Debug)]
pub(crate) struct FileConnection {
    stream: TcpStream,
    /// Bytes already read past the transfer token.
    buffered: BytesMut,
}

type Key = (String, String);

/// Routes peer messages and file connections to the download they belong to.
#[derive(Debug, Default)]
pub(crate) struct Transfers {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    by_key: HashMap<Key, mpsc::Sender<Event>>,
    by_token: HashMap<u32, Key>,
}

impl Transfers {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn register(&self, key: &Key) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel(16);
        self.lock().by_key.insert(key.clone(), tx);
        rx
    }

    fn unregister(&self, key: &Key) {
        let mut inner = self.lock();
        inner.by_key.remove(key);
        inner.by_token.retain(|_, k| k != key);
    }

    fn send(&self, key: &Key, event: Event) {
        if let Some(tx) = self.lock().by_key.get(key) {
            let _ = tx.try_send(event);
        }
    }

    /// Handle a message a peer sent us, replying on the same connection when needed.
    pub(crate) fn on_peer_message(&self, username: &str, message: PeerMessage, reply: &PeerSender) {
        let key = |filename: String| (username.to_owned(), filename);
        match message {
            PeerMessage::TransferRequest { direction: direction::UPLOAD, token, filename, size } => {
                let key = key(filename);
                let wanted = self.lock().by_key.contains_key(&key);
                let response = if wanted {
                    self.lock().by_token.insert(token, key.clone());
                    self.send(&key, Event::Request { size });
                    PeerMessage::TransferResponse { token, allowed: true, reason: None }
                } else {
                    PeerMessage::TransferResponse { token, allowed: false, reason: Some("Cancelled".into()) }
                };
                let _ = reply.try_send(response.encode());
            }
            // Someone wants to download from us. Sharing isn't implemented yet.
            PeerMessage::TransferRequest { token, .. } => {
                let response =
                    PeerMessage::TransferResponse { token, allowed: false, reason: Some("File not shared.".into()) };
                let _ = reply.try_send(response.encode());
            }
            PeerMessage::QueueUpload { filename } => {
                let _ =
                    reply.try_send(PeerMessage::UploadDenied { filename, reason: "File not shared.".into() }.encode());
            }
            PeerMessage::PlaceInQueueResponse { filename, place } => self.send(&key(filename), Event::Place(place)),
            PeerMessage::UploadDenied { filename, reason } => self.send(&key(filename), Event::Denied(reason)),
            PeerMessage::UploadFailed { filename } => self.send(&key(filename), Event::UploadFailed),
            PeerMessage::TransferResponse { .. }
            | PeerMessage::PlaceInQueueRequest { .. }
            | PeerMessage::SharedFileListRequest
            | PeerMessage::UserInfoRequest
            | PeerMessage::FolderContentsRequest { .. } => {}
        }
    }

    fn on_file_connection(&self, token: u32, conn: FileConnection) {
        let key = self.lock().by_token.remove(&token);
        if let Some(key) = key {
            self.send(&key, Event::File(conn));
        } else {
            tracing::debug!(token, "file connection for an unknown transfer");
        }
    }
}

/// After a file connection's init message: read the transfer token and hand the
/// connection to its download.
pub(crate) async fn accept_file_connection(conn: PeerStream, shared: &Shared) {
    let parts = conn.into_parts();
    let (mut stream, mut buffered) = (parts.io, parts.read_buf);
    while buffered.len() < 4 {
        let mut chunk = [0u8; 4];
        match timeout(FILE_CONNECTION_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => buffered.extend_from_slice(&chunk[..n]),
            _ => return,
        }
    }
    let token = buffered.get_u32_le();
    tracing::debug!(token, buffered = buffered.len(), "file connection ready");
    shared.transfers.on_file_connection(token, FileConnection { stream, buffered });
}

async fn run(
    shared: &Arc<Shared>,
    request: &DownloadRequest,
    mut events: mpsc::Receiver<Event>,
    state: &watch::Sender<DownloadState>,
    mut cancel: mpsc::Receiver<()>,
) -> DownloadState {
    let username = &request.username;
    let mut last_error = String::new();
    let mut tries = Tries::spread_over(QUEUE_FULL_SPREAD, &request.filename);

    'attempts: while let Some(delay) = tries.pause() {
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            _ = cancel.recv() => return DownloadState::Cancelled,
        }
        state.send_replace(DownloadState::Connecting);

        let peer = tokio::select! {
            peer = connection::connect_peer(shared, username) => peer,
            _ = cancel.recv() => return DownloadState::Cancelled,
        };
        let peer = match peer {
            Ok(peer) => peer,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        if peer.send(PeerMessage::QueueUpload { filename: request.filename.clone() }.encode()).await.is_err() {
            last_error = format!("the connection to {username} closed");
            continue;
        }
        state.send_replace(DownloadState::Queued { place: None });

        let mut size = None;
        let mut deadline: Option<Instant> = None;
        loop {
            let wait_for_file = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            let event = tokio::select! {
                event = events.recv() => event,
                () = wait_for_file => {
                    last_error = format!("{username} accepted but never started sending");
                    continue 'attempts;
                }
                _ = cancel.recv() => return DownloadState::Cancelled,
            };
            match event {
                None => return DownloadState::Failed { reason: "the download was abandoned".into() },
                Some(Event::Place(place)) => {
                    state.send_replace(DownloadState::Queued { place: Some(place) });
                }
                Some(Event::Denied(reason)) if reason == "Queued" => {
                    state.send_replace(DownloadState::Queued { place: None });
                }
                Some(Event::Denied(reason)) if is_over_their_limit(&reason) => {
                    last_error = format!("{username} declined: {}", reason.trim_end_matches('.'));
                    // Their queue, not our file: keep the download alive and ask again later.
                    state.send_replace(DownloadState::Queued { place: None });
                    tries.over_limit = true;
                    continue 'attempts;
                }
                Some(Event::Denied(reason)) => {
                    return DownloadState::Failed {
                        reason: format!("{username} declined: {}", reason.trim_end_matches('.')),
                    };
                }
                Some(Event::UploadFailed) => {
                    last_error = format!("{username}'s upload failed");
                    continue 'attempts;
                }
                Some(Event::Request { size: announced }) => {
                    size = announced;
                    deadline = Some(Instant::now() + FILE_CONNECTION_TIMEOUT);
                    state.send_replace(DownloadState::Starting { size });
                }
                Some(Event::File(conn)) => {
                    match receive(&shared.download_cap, conn, &request.destination, size, state, &mut cancel).await {
                        Ok(bytes) => return DownloadState::Completed { bytes },
                        Err(ReceiveError::Cancelled) => return DownloadState::Cancelled,
                        Err(ReceiveError::Io(error))
                            if error.kind() != std::io::ErrorKind::UnexpectedEof
                                && error.kind() != std::io::ErrorKind::TimedOut
                                && error.kind() != std::io::ErrorKind::ConnectionReset =>
                        {
                            return DownloadState::Failed { reason: format!("couldn't save the file: {error}") };
                        }
                        Err(ReceiveError::Io(error)) => {
                            last_error = format!("the transfer from {username} dropped ({error})");
                            continue 'attempts;
                        }
                    }
                }
            }
        }
    }

    DownloadState::Failed { reason: format!("gave up after {} tries: {last_error}", tries.spent()) }
}

/// What is left of one file's patience: connection attempts, plus a separate budget of
/// longer waits for a peer that is over its own limits, since that is not our fault.
#[derive(Default)]
struct Tries {
    total: u32,
    attempts: u32,
    waits: u32,
    /// Set when the last try ended in the peer saying it is over its limits.
    over_limit: bool,
    /// Added to every wait, so the files of one job don't all ask again in the same tick.
    stagger: Duration,
}

impl Tries {
    /// Patience for one file, its waits nudged apart from the other files of the job by
    /// up to `window`, picked from the name so each file keeps its own offset.
    fn spread_over(window: Duration, filename: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        filename.hash(&mut hasher);
        let window = u64::try_from(window.as_millis()).unwrap_or(u64::MAX).max(1);
        Self { stagger: Duration::from_millis(hasher.finish() % window), ..Self::default() }
    }

    /// How long to wait before the next try, or `None` once the patience is spent.
    fn pause(&mut self) -> Option<Duration> {
        self.total += 1;
        if std::mem::take(&mut self.over_limit) {
            self.waits += 1;
            return (self.waits <= MAX_QUEUE_WAITS).then(|| QUEUE_FULL_DELAY + self.stagger);
        }
        self.attempts += 1;
        if self.attempts > MAX_ATTEMPTS {
            return None;
        }
        Some(if self.attempts > 1 { RETRY_DELAY } else { Duration::ZERO })
    }

    /// How many tries were made, for the message when we give up.
    fn spent(&self) -> u32 {
        self.total - 1
    }
}

/// Whether a refusal means "not now" rather than "not ever": the peer is over the files
/// or megabytes it lets one person queue, so the same request works later. Clients word
/// this differently, so match the phrase rather than one client's exact sentence.
fn is_over_their_limit(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    ["too many", "queue full", "queue is full", "limit reached"].iter().any(|r| reason.contains(r))
}

#[derive(Debug)]
enum ReceiveError {
    Cancelled,
    Io(std::io::Error),
}

impl From<std::io::Error> for ReceiveError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    destination.with_file_name(name)
}

async fn receive(
    shared_cap: &Arc<crate::pacing::SpeedCap>,
    mut conn: FileConnection,
    destination: &Path,
    size: Option<u64>,
    state: &watch::Sender<DownloadState>,
    cancel: &mut mpsc::Receiver<()>,
) -> Result<u64, ReceiveError> {
    let Some(size) = size else {
        return Err(ReceiveError::Io(std::io::Error::other("the peer didn't say how big the file is")));
    };
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await?;
    }
    let part = part_path(destination);
    let file = OpenOptions::new().create(true).append(true).open(&part).await?;
    let mut received = file.metadata().await?.len();
    if received > size {
        file.set_len(0).await?;
        received = 0;
    }
    // Write in large batches: a disk write per network read is slow on NAS mounts.
    let mut file = BufWriter::with_capacity(1 << 20, file);

    conn.stream.write_all(&received.to_le_bytes()).await?;
    tracing::debug!(destination = %destination.display(), offset = received, size, "receiving file");

    let take = usize::try_from((size - received).min(conn.buffered.len() as u64)).unwrap_or(0);
    if take > 0 {
        file.write_all(&conn.buffered[..take]).await?;
        received += take as u64;
    }

    let mut buf = vec![0u8; 256 * 1024];
    let mut last_report = Instant::now();
    let mut pace = shared_cap.start();
    state.send_replace(DownloadState::Transferring { bytes: received, size });
    while received < size {
        let want = usize::try_from((size - received).min(buf.len() as u64)).unwrap_or(buf.len());
        let read = tokio::select! {
            read = timeout(STALL_TIMEOUT, conn.stream.read(&mut buf[..want])) => read,
            _ = cancel.recv() => {
                file.flush().await?;
                return Err(ReceiveError::Cancelled);
            }
        };
        let n = match read {
            Ok(Ok(0)) => return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into()),
            Ok(Ok(n)) => n,
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => return Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into()),
        };
        file.write_all(&buf[..n]).await?;
        received += n as u64;
        pace.record(n).await;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            state.send_replace(DownloadState::Transferring { bytes: received, size });
            last_report = Instant::now();
        }
    }

    file.flush().await?;
    let file = file.into_inner();
    file.sync_all().await?;
    drop(file);
    fs::rename(&part, destination).await?;
    // Closing the connection is how the downloader says "done".
    drop(conn);
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_files_sit_next_to_the_destination() {
        assert_eq!(
            part_path(Path::new("/music/staging/01 - Airbag.flac")),
            PathBuf::from("/music/staging/01 - Airbag.flac.part")
        );
    }

    #[test]
    fn finished_states() {
        assert!(DownloadState::Completed { bytes: 1 }.is_finished());
        assert!(DownloadState::Failed { reason: String::new() }.is_finished());
        assert!(!DownloadState::Queued { place: Some(3) }.is_finished());
    }

    #[test]
    fn connection_attempts_run_out() {
        let mut tries = Tries::default();
        assert_eq!(tries.pause(), Some(Duration::ZERO));
        for _ in 1..MAX_ATTEMPTS {
            assert_eq!(tries.pause(), Some(RETRY_DELAY));
        }
        assert_eq!(tries.pause(), None);
        assert_eq!(tries.spent(), MAX_ATTEMPTS);
    }

    #[test]
    fn waiting_out_a_full_queue_does_not_spend_attempts() {
        let mut tries = Tries::default();
        assert_eq!(tries.pause(), Some(Duration::ZERO));
        for _ in 0..MAX_QUEUE_WAITS {
            tries.over_limit = true;
            assert_eq!(tries.pause(), Some(QUEUE_FULL_DELAY));
        }
        tries.over_limit = true;
        assert_eq!(tries.pause(), None);
        assert_eq!(tries.spent(), MAX_QUEUE_WAITS + 1);
        assert_eq!(tries.attempts, 1);
    }

    #[test]
    fn files_of_a_job_wait_out_of_step() {
        let one = Tries::spread_over(QUEUE_FULL_SPREAD, "Music/01 - Featherfall.flac");
        let two = Tries::spread_over(QUEUE_FULL_SPREAD, "Music/02 - Watcher.flac");
        assert_ne!(one.stagger, two.stagger);
        assert!(one.stagger < QUEUE_FULL_SPREAD && two.stagger < QUEUE_FULL_SPREAD);
    }

    #[test]
    fn other_wordings_for_a_full_queue_count_too() {
        assert!(is_over_their_limit("Too many megabytes queued"));
        assert!(is_over_their_limit("User queue is full."));
        assert!(is_over_their_limit("Upload limit reached"));
    }

    #[test]
    fn limit_refusals_are_worth_waiting_out() {
        assert!(is_over_their_limit("Too many megabytes."));
        assert!(is_over_their_limit("Too many files"));
        assert!(is_over_their_limit("queue full"));
    }

    #[test]
    fn other_refusals_are_final() {
        assert!(!is_over_their_limit("File not shared."));
        assert!(!is_over_their_limit("Banned"));
        assert!(!is_over_their_limit("Cancelled"));
    }
}
