//! The distributed search network, as a leaf.
//!
//! Most searches don't come from the server. The server hands them to "branch
//! roots", who pass them down a tree of clients. To be found by other people's
//! searches, a client joins that tree: the server suggests possible parents
//! (users faster than us), we connect to them over `D` connections, and the first
//! one that sends a search becomes our parent. From then on every search in our
//! branch arrives from it, and we answer the ones our shares match, directly to the
//! person searching.
//!
//! By default delune is a leaf: it doesn't accept children, so it never relays
//! searches to anyone. That keeps its bandwidth use predictable on a home connection
//! while still making its shares findable. People can let it relay: then, while it
//! has a parent, other clients may join below it as children, it tells them where
//! they sit in the tree, and it passes every search from its parent down to them.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::codec::Framed;

use crate::connection::Shared;
use crate::frame::{FrameCodec, MAX_PEER_FRAME};
use crate::peer::PeerInit;
use crate::server::ServerRequest;
use crate::upload;
use crate::wire::{DecodeError, Reader};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A parent that goes quiet this long is replaced.
const PARENT_IDLE: Duration = Duration::from_secs(5 * 60);
/// How long a candidate has to send its first search before we give up on it.
const ADOPT_TIMEOUT: Duration = Duration::from_secs(60);
/// Searches queued for a child before it's considered too slow to keep up.
const CHILD_QUEUE: usize = 256;

pub mod code {
    pub const PING: u8 = 0;
    pub const SEARCH: u8 = 3;
    pub const BRANCH_LEVEL: u8 = 4;
    pub const BRANCH_ROOT: u8 = 5;
    pub const EMBEDDED: u8 = 93;
}

/// A message from a distributed parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributedMessage {
    Ping,
    Search {
        username: String,
        token: u32,
        query: String,
    },
    BranchLevel(i32),
    BranchRoot(String),
    /// Anything else, including embedded non-search messages.
    Other(u8),
}

impl DistributedMessage {
    /// Decode a frame payload: a one-byte code, then the message.
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(payload);
        let message_code = r.u8()?;
        Self::decode_body(message_code, r.bytes(r.remaining())?)
    }

    /// Decode a message body whose code is already known.
    pub fn decode_body(message_code: u8, body: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(body);
        Ok(match message_code {
            code::PING => Self::Ping,
            code::SEARCH => {
                let _identifier = r.u32()?;
                Self::Search { username: r.string()?, token: r.u32()?, query: r.string()? }
            }
            code::BRANCH_LEVEL => Self::BranchLevel(i32::from_le_bytes(r.u32()?.to_le_bytes())),
            code::BRANCH_ROOT => Self::BranchRoot(r.string()?),
            code::EMBEDDED => {
                let inner = r.u8()?;
                return Self::decode_body(inner, r.bytes(r.remaining())?);
            }
            other => Self::Other(other),
        })
    }
}

/// Where we sit in the tree while we have a parent, and the children below us.
#[derive(Debug, Default)]
pub(crate) struct Branch {
    /// Our level (our parent's plus one) and the branch root, while we have a parent.
    pub place: Option<(u32, String)>,
    /// Each child's outbox of framed messages.
    pub children: HashMap<String, mpsc::Sender<Bytes>>,
}

/// Whether we'd take a child now: relaying is on, we have a parent, and there's room.
fn taking_children(shared: &Shared, branch: &Branch) -> bool {
    let max = shared.max_children.load(Ordering::Relaxed);
    max > 0 && branch.place.is_some() && branch.children.len() < max
}

/// Tell the server whether we need a parent, and whether we take children.
pub(crate) fn greeting(shared: &Shared) -> Vec<ServerRequest> {
    let has_parent = shared.distributed_parent.load(Ordering::Relaxed);
    let accept = taking_children(shared, &shared.branch());
    vec![ServerRequest::HaveNoParent(!has_parent), ServerRequest::AcceptChildren(accept)]
}

/// Frame a distributed message payload (code byte and body) for sending.
fn framed(payload: &[u8]) -> Bytes {
    let mut frame = BytesMut::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&u32::try_from(payload.len()).unwrap_or(u32::MAX).to_le_bytes());
    frame.extend_from_slice(payload);
    frame.freeze()
}

fn level_message(level: u32) -> Bytes {
    let mut payload = vec![code::BRANCH_LEVEL];
    payload.extend_from_slice(&level.to_le_bytes());
    framed(&payload)
}

fn root_message(root: &str) -> Bytes {
    let mut payload = vec![code::BRANCH_ROOT];
    payload.extend_from_slice(&u32::try_from(root.len()).unwrap_or(u32::MAX).to_le_bytes());
    payload.extend_from_slice(root.as_bytes());
    framed(&payload)
}

/// Send `frame` to every child, dropping children that have gone away.
fn to_children(shared: &Shared, frame: &Bytes) {
    let mut branch = shared.branch();
    branch.children.retain(|username, child| match child.try_send(frame.clone()) {
        Ok(()) => true,
        // A slow child misses this search rather than holding up the others.
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::trace!(%username, "distributed child is behind; skipping a message");
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    });
}

/// Tell the server whether we take children, after something changed.
fn announce_children(shared: &Shared) {
    let accept = taking_children(shared, &shared.branch());
    if let Some(server) = shared.server() {
        let _ = server.try_send(ServerRequest::AcceptChildren(accept));
    }
}

/// Settings changed: take up to `max` children (none turns relaying off).
pub(crate) fn set_max_children(shared: &Shared, max: usize) {
    shared.max_children.store(max, Ordering::Relaxed);
    if max == 0 {
        shared.branch().children.clear();
    }
    announce_children(shared);
}

/// A client connected to be our child, directly or by answering our pierce.
pub(crate) async fn run_child(shared: Arc<Shared>, username: String, mut conn: Framed<TcpStream, FrameCodec>) {
    let (tx, mut rx) = mpsc::channel::<Bytes>(CHILD_QUEUE);
    let place = {
        let mut branch = shared.branch();
        if !taking_children(&shared, &branch) && !branch.children.contains_key(&username) {
            tracing::debug!(%username, "not taking distributed children right now");
            return;
        }
        branch.children.insert(username.clone(), tx.clone());
        branch.place.clone()
    };
    // Only the branch holds a sender, so clearing it disconnects this child.
    let ours = tx.downgrade();
    drop(tx);
    let Some((level, root)) = place else { return };
    tracing::debug!(child = %username, "distributed child joined");
    announce_children(&shared);

    let greeting = [level_message(level), root_message(&root)];
    let mut greeted = true;
    for frame in greeting {
        greeted &= conn.send(BytesMut::from(&frame[..])).await.is_ok();
    }
    if !greeted {
        shared.branch().children.remove(&username);
        return;
    }
    loop {
        tokio::select! {
            outgoing = rx.recv() => {
                // No sender left: we lost our parent or stopped relaying.
                let Some(frame) = outgoing else { break };
                if conn.send(BytesMut::from(&frame[..])).await.is_err() {
                    break;
                }
            }
            incoming = conn.next() => {
                // Children have nothing to tell us; this just notices them leaving.
                if !matches!(incoming, Some(Ok(_))) {
                    break;
                }
            }
        }
    }
    {
        let mut branch = shared.branch();
        let still_ours =
            ours.upgrade().is_some_and(|tx| branch.children.get(&username).is_some_and(|c| c.same_channel(&tx)));
        if still_ours {
            branch.children.remove(&username);
        }
    }
    tracing::debug!(child = %username, "distributed child left");
    announce_children(&shared);
}

/// The server suggested parents: try them all at once and keep the first that
/// relays a search. Does nothing if we already have a parent or share nothing.
pub(crate) fn on_possible_parents(shared: &Arc<Shared>, candidates: Vec<(String, Ipv4Addr, u32)>) {
    if shared.distributed_parent.load(Ordering::Relaxed) || shared.uploads.index().file_count() == 0 {
        return;
    }
    if shared.distributed_connecting.swap(true, Ordering::Relaxed) {
        return;
    }
    let shared = shared.clone();
    tokio::spawn(async move {
        let (adopted_tx, mut adopted_rx) = mpsc::channel::<(String, i32, String, Framed<TcpStream, FrameCodec>)>(1);
        let mut attempts = tokio::task::JoinSet::new();
        for (username, ip, port) in candidates {
            let Ok(port) = u16::try_from(port) else { continue };
            let (shared, adopted_tx) = (shared.clone(), adopted_tx.clone());
            attempts.spawn(async move {
                if let Some(parent) = try_parent(&shared, username, SocketAddr::from((ip, port))).await {
                    let _ = adopted_tx.send(parent).await;
                }
            });
        }
        drop(adopted_tx);
        let adopted = adopted_rx.recv().await;
        attempts.abort_all();
        shared.distributed_connecting.store(false, Ordering::Relaxed);
        if let Some((username, level, root, conn)) = adopted {
            run_parent(shared, username, level, root, conn).await;
        }
    });
}

/// Connect to a candidate and wait for branch details and a first search.
async fn try_parent(
    shared: &Arc<Shared>,
    username: String,
    addr: SocketAddr,
) -> Option<(String, i32, String, Framed<TcpStream, FrameCodec>)> {
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await.ok()?.ok()?;
    let mut conn = Framed::new(stream, FrameCodec::new(MAX_PEER_FRAME));
    let init = PeerInit::PeerInit { username: shared.own_username.clone(), kind: "D".into(), token: 0 };
    conn.send(init.encode()).await.ok()?;

    let (mut level, mut root) = (None, None);
    let wait = async {
        while let Some(Ok(frame)) = conn.next().await {
            match DistributedMessage::decode(&frame) {
                Ok(DistributedMessage::BranchLevel(l)) => {
                    level = Some(l);
                    if l == 0 {
                        root = Some(username.clone());
                    }
                }
                Ok(DistributedMessage::BranchRoot(r)) => root = Some(r),
                Ok(DistributedMessage::Search { username: searcher, token, query }) if level.is_some() => {
                    answer(shared, searcher, token, &query);
                    return true;
                }
                _ => {}
            }
        }
        false
    };
    let adopted = timeout(ADOPT_TIMEOUT, wait).await.unwrap_or(false);
    if !adopted {
        return None;
    }
    let level = level.unwrap_or(0);
    let root = root.unwrap_or_else(|| username.clone());
    Some((username, level, root, conn))
}

/// Read searches from our parent until it goes away, then ask for a new one.
async fn run_parent(
    shared: Arc<Shared>,
    username: String,
    level: i32,
    root: String,
    mut conn: Framed<TcpStream, FrameCodec>,
) {
    tracing::info!(parent = %username, level, %root, "joined the distributed search network");
    shared.distributed_parent.store(true, Ordering::Relaxed);
    let our_level = u32::try_from(level.saturating_add(1)).unwrap_or(1);
    shared.branch().place = Some((our_level, root.clone()));
    if let Some(server) = shared.server() {
        let _ = server.try_send(ServerRequest::HaveNoParent(false));
        let _ = server.try_send(ServerRequest::BranchLevel(our_level));
        let _ = server.try_send(ServerRequest::BranchRoot(root));
    }
    announce_children(&shared);
    let mut reset = shared.distributed_reset.subscribe();
    loop {
        let frame = tokio::select! {
            frame = timeout(PARENT_IDLE, conn.next()) => frame,
            _ = reset.changed() => break,
        };
        let Ok(Some(Ok(frame))) = frame else { break };
        match DistributedMessage::decode(&frame) {
            Ok(DistributedMessage::Search { username: searcher, token, query }) => {
                // Pass it down exactly as it came, then answer it ourselves.
                to_children(&shared, &framed(&frame));
                answer(&shared, searcher, token, &query);
            }
            Ok(DistributedMessage::BranchRoot(root)) => {
                if let Some(place) = &mut shared.branch().place {
                    place.1.clone_from(&root);
                }
                to_children(&shared, &root_message(&root));
                if let Some(server) = shared.server() {
                    let _ = server.try_send(ServerRequest::BranchRoot(root));
                }
            }
            Ok(DistributedMessage::BranchLevel(level)) => {
                let ours = u32::try_from(level.saturating_add(1)).unwrap_or(1);
                if let Some(place) = &mut shared.branch().place {
                    place.0 = ours;
                }
                to_children(&shared, &level_message(ours));
                if let Some(server) = shared.server() {
                    let _ = server.try_send(ServerRequest::BranchLevel(ours));
                }
            }
            _ => {}
        }
    }
    tracing::info!(parent = %username, "left the distributed search network; looking for a new parent");
    shared.distributed_parent.store(false, Ordering::Relaxed);
    {
        // Our children need a new parent too; dropping their outboxes disconnects them.
        let mut branch = shared.branch();
        branch.place = None;
        branch.children.clear();
    }
    if let Some(server) = shared.server() {
        let _ = server.try_send(ServerRequest::HaveNoParent(true));
        let _ = server.try_send(ServerRequest::AcceptChildren(false));
    }
}

/// A search relayed to us, or embedded in a server message when we're a branch root.
pub(crate) fn answer(shared: &Arc<Shared>, username: String, token: u32, query: &str) {
    upload::answer_search(shared, username, token, query);
}

/// Forget our parent (the server asked, or we reconnected) so a new one is found.
pub(crate) fn reset(shared: &Shared) {
    shared.distributed_reset.send_modify(|n| *n += 1);
    shared.distributed_parent.store(false, Ordering::Relaxed);
    let mut branch = shared.branch();
    branch.place = None;
    branch.children.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Writer;

    fn payload(message_code: u8, body: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut w = Writer::new();
        body(&mut w);
        let mut out = vec![message_code];
        out.extend_from_slice(&w.into_body());
        out
    }

    #[test]
    fn decodes_distributed_messages() {
        let search = payload(code::SEARCH, |w| {
            w.u32(49).string("seeker").u32(77).string("talk talk");
        });
        assert_eq!(
            DistributedMessage::decode(&search).unwrap(),
            DistributedMessage::Search { username: "seeker".into(), token: 77, query: "talk talk".into() }
        );
        assert_eq!(
            DistributedMessage::decode(&payload(code::BRANCH_LEVEL, |w| {
                w.u32(2);
            }))
            .unwrap(),
            DistributedMessage::BranchLevel(2)
        );
        assert_eq!(
            DistributedMessage::decode(&payload(code::BRANCH_ROOT, |w| {
                w.string("root");
            }))
            .unwrap(),
            DistributedMessage::BranchRoot("root".into())
        );

        // Older clients wrap searches in an embedded message.
        let mut embedded = vec![code::EMBEDDED];
        embedded.extend_from_slice(&search);
        assert!(matches!(DistributedMessage::decode(&embedded).unwrap(), DistributedMessage::Search { token: 77, .. }));
    }
}
