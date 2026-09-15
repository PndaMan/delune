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
//! delune is a leaf: it doesn't accept children, so it never relays searches to
//! anyone. That keeps its bandwidth use predictable on a home connection while still
//! making its shares findable. The network works as long as faster clients relay.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

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

/// Tell the server whether we need a parent, and that we don't take children.
pub(crate) fn greeting(shared: &Shared) -> Vec<ServerRequest> {
    let has_parent = shared.distributed_parent.load(Ordering::Relaxed);
    vec![ServerRequest::HaveNoParent(!has_parent), ServerRequest::AcceptChildren(false)]
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
    if let Some(server) = shared.server() {
        let our_level = u32::try_from(level.saturating_add(1)).unwrap_or(1);
        let _ = server.try_send(ServerRequest::HaveNoParent(false));
        let _ = server.try_send(ServerRequest::BranchLevel(our_level));
        let _ = server.try_send(ServerRequest::BranchRoot(root));
    }
    let mut reset = shared.distributed_reset.subscribe();
    loop {
        let frame = tokio::select! {
            frame = timeout(PARENT_IDLE, conn.next()) => frame,
            _ = reset.changed() => break,
        };
        let Ok(Some(Ok(frame))) = frame else { break };
        match DistributedMessage::decode(&frame) {
            Ok(DistributedMessage::Search { username: searcher, token, query }) => {
                answer(&shared, searcher, token, &query);
            }
            Ok(DistributedMessage::BranchRoot(root)) => {
                if let Some(server) = shared.server() {
                    let _ = server.try_send(ServerRequest::BranchRoot(root));
                }
            }
            Ok(DistributedMessage::BranchLevel(level)) => {
                if let Some(server) = shared.server() {
                    let _ = server
                        .try_send(ServerRequest::BranchLevel(u32::try_from(level.saturating_add(1)).unwrap_or(1)));
                }
            }
            _ => {}
        }
    }
    tracing::info!(parent = %username, "left the distributed search network; looking for a new parent");
    shared.distributed_parent.store(false, Ordering::Relaxed);
    if let Some(server) = shared.server() {
        let _ = server.try_send(ServerRequest::HaveNoParent(true));
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
