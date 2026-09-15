//! Peer connection tasks.
//!
//! Search results arrive over peer ("P") connections, which get established in one
//! of two ways:
//!
//! 1. **Direct** — the peer connects to our listening port and opens with
//!    `PeerInit`. Needs our port to be reachable.
//! 2. **Indirect** — the peer couldn't reach us, so it asked the server to tell us to
//!    connect to *them*. We receive `ConnectToPeer`, dial out, and open with
//!    `PierceFirewall` carrying their token. If we can't reach them either, we say
//!    so with `CantConnectToPeer` so they stop waiting.
//!
//! After the init message both paths are identical: read framed peer messages until
//! the peer goes quiet. Every connection holds a semaphore permit, and every read has
//! a timeout, so misbehaving peers can't exhaust sockets or memory.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::codec::Framed;

use crate::client::PeerContext;
use crate::frame::{FrameCodec, MAX_PEER_FRAME, split_code};
use crate::peer::{PeerInit, SearchResponse, code};
use crate::server::ServerRequest;

const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Close a peer connection after this long without a message.
const PEER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

type PeerStream = Framed<TcpStream, FrameCodec>;

/// Accept incoming peer connections until the task is aborted.
pub(crate) async fn accept_loop(listener: TcpListener, ctx: PeerContext) {
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                tokio::spawn(handle_incoming(stream, addr, ctx.clone()));
            }
            Err(error) => {
                // Usually file-descriptor exhaustion; pause instead of spinning.
                tracing::warn!(%error, "failed to accept peer connection");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn handle_incoming(stream: TcpStream, addr: SocketAddr, ctx: PeerContext) {
    let Ok(_permit) = ctx.permits.clone().try_acquire_owned() else {
        tracing::debug!(%addr, "too many peer connections; refusing");
        return;
    };
    let mut peer = Framed::new(stream, FrameCodec::new(MAX_PEER_FRAME));
    let Ok(Some(Ok(first))) = timeout(PEER_IDLE_TIMEOUT, peer.next()).await else { return };

    match PeerInit::decode(&first) {
        Ok(PeerInit::PeerInit { username, kind, .. }) if kind == "P" => {
            tracing::trace!(%addr, %username, "direct peer connection");
            read_messages(peer, &ctx).await;
        }
        // File and distributed connections aren't supported yet; neither is a
        // PierceFirewall we didn't ask for.
        Ok(other) => tracing::trace!(%addr, ?other, "ignoring peer connection"),
        Err(error) => tracing::debug!(%addr, %error, "bad peer init"),
    }
}

/// Answer a `ConnectToPeer` from the server by dialling the peer ourselves.
pub(crate) async fn pierce(
    addr: SocketAddr,
    token: u32,
    username: String,
    ctx: PeerContext,
    server_out: mpsc::Sender<ServerRequest>,
) {
    let Ok(_permit) = ctx.permits.clone().try_acquire_owned() else { return };

    let Ok(Ok(stream)) = timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(addr)).await else {
        tracing::trace!(%addr, %username, "can't reach peer");
        let _ = server_out.try_send(ServerRequest::CantConnectToPeer { token, username });
        return;
    };
    let mut peer = Framed::new(stream, FrameCodec::new(MAX_PEER_FRAME));
    if peer.send(PeerInit::PierceFirewall { token }.encode()).await.is_ok() {
        read_messages(peer, &ctx).await;
    }
}

async fn read_messages(mut peer: PeerStream, ctx: &PeerContext) {
    while let Ok(Some(Ok(frame))) = timeout(PEER_IDLE_TIMEOUT, peer.next()).await {
        let Ok((message_code, body)) = split_code(&frame) else { return };
        if message_code == code::SEARCH_RESPONSE {
            match SearchResponse::decode(body) {
                Ok(response) => {
                    tracing::trace!(username = %response.username, token = response.token, files = response.files.len(), "search response");
                    ctx.registry.deliver(response);
                }
                Err(error) => tracing::debug!(%error, "bad search response"),
            }
        }
    }
}
