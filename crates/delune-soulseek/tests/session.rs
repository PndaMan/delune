//! End-to-end client tests against an in-process fake Soulseek server and fake peers.
//!
//! These exercise the real client over real TCP sockets on localhost: login, the
//! post-login handshake, searching, results arriving over both a direct peer
//! connection and a firewall-pierced one, login rejection, and reconnecting.

use std::net::Ipv4Addr;
use std::time::Duration;

use bytes::Bytes;
use delune_soulseek::client::{Client, Config, Error, SessionState, StopReason};
use delune_soulseek::frame::{FrameCodec, split_code};
use delune_soulseek::peer::{PeerInit, SearchResponse, SharedFile};
use delune_soulseek::server::{LoginRejection, code};
use delune_soulseek::wire::{Reader, Writer};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_util::codec::Framed;

type Conn = Framed<TcpStream, FrameCodec>;

const WAIT: Duration = Duration::from_secs(5);

fn framed(stream: TcpStream) -> Conn {
    Framed::new(stream, FrameCodec::new(1 << 20))
}

async fn next_frame(conn: &mut Conn) -> Bytes {
    timeout(WAIT, conn.next()).await.expect("timed out waiting for a frame").expect("connection closed").unwrap()
}

/// Read server-bound frames until one has `wanted` code; return its body.
async fn expect_code(conn: &mut Conn, wanted: u32) -> Vec<u8> {
    loop {
        let frame = next_frame(conn).await;
        let (code, body) = split_code(&frame).unwrap();
        if code == wanted {
            return body.to_vec();
        }
    }
}

fn login_ok() -> bytes::BytesMut {
    let mut w = Writer::new();
    w.bool(true).string("Welcome to the fake server").ip(Ipv4Addr::new(203, 0, 113, 7)).string("hash").bool(false);
    w.finish(code::LOGIN)
}

fn config(server: &TcpListener) -> Config {
    let mut config = Config::new("delune-test", "secret");
    config.server = server.local_addr().unwrap().to_string();
    config.listen_port = Some(0);
    config.search_timeout = Duration::from_secs(3);
    config.reconnect_base = Duration::from_millis(50);
    config
}

async fn wait_for(client: &Client, pred: impl Fn(&SessionState) -> bool) -> SessionState {
    let mut state = client.state();
    timeout(WAIT, state.wait_for(|s| pred(s))).await.expect("timed out waiting for state").unwrap().clone()
}

fn album_response(username: &str, token: u32) -> SearchResponse {
    SearchResponse {
        username: username.into(),
        token,
        files: vec![SharedFile {
            path: format!(r"@@{username}\Radiohead\1997 - OK Computer\01 - Airbag.flac"),
            size: 30_000_000,
            extension: "flac".into(),
            bitrate_kbps: None,
            duration_secs: Some(284),
            vbr: false,
            sample_rate: Some(44_100),
            bit_depth: Some(16),
        }],
        free_slot: true,
        avg_speed: 900_000,
        queue_length: 0,
        private_files: vec![],
    }
}

#[tokio::test]
async fn login_search_and_receive_from_direct_and_pierced_peers() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(config(&server));

    let (socket, _) = timeout(WAIT, server.accept()).await.unwrap().unwrap();
    let mut conn = framed(socket);

    // Login carries our username and password.
    let login = expect_code(&mut conn, code::LOGIN).await;
    let mut r = Reader::new(&login);
    assert_eq!(r.string().unwrap(), "delune-test");
    assert_eq!(r.string().unwrap(), "secret");
    conn.send(login_ok()).await.unwrap();

    // After login the client announces its listening port.
    let port = Reader::new(&expect_code(&mut conn, code::SET_WAIT_PORT).await).u32().unwrap();
    let port = u16::try_from(port).unwrap();
    assert_ne!(port, 0);

    let state = wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;
    assert_eq!(
        state,
        SessionState::Online { public_ip: Ipv4Addr::new(203, 0, 113, 7), listen_port: Some(port), supporter: false }
    );

    let mut search = client.search("  radiohead ok computer ").await.unwrap();
    let body = expect_code(&mut conn, code::FILE_SEARCH).await;
    let mut r = Reader::new(&body);
    let token = r.u32().unwrap();
    assert_eq!(token, search.token());
    assert_eq!(r.string().unwrap(), "radiohead ok computer");

    // Peer 1 reaches our listening port directly.
    let direct = tokio::spawn(async move {
        let mut peer = framed(TcpStream::connect(("127.0.0.1", port)).await.unwrap());
        peer.send(PeerInit::PeerInit { username: "direct".into(), kind: "P".into(), token: 0 }.encode()).await.unwrap();
        peer.send(album_response("direct", token).encode()).await.unwrap();
        // Also send a response for someone else's search: it must be ignored.
        peer.send(album_response("direct", token.wrapping_add(1000)).encode()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    });

    // Peer 2 is firewalled: the server tells us to connect to it instead.
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_port = peer_listener.local_addr().unwrap().port();
    let mut connect = Writer::new();
    connect.string("pierced").string("P").ip(Ipv4Addr::LOCALHOST).u32(u32::from(peer_port)).u32(4242).bool(false);
    conn.send(connect.finish(code::CONNECT_TO_PEER)).await.unwrap();

    let pierced = tokio::spawn(async move {
        let (socket, _) = timeout(WAIT, peer_listener.accept()).await.unwrap().unwrap();
        let mut peer = framed(socket);
        let init = PeerInit::decode(&next_frame(&mut peer).await).unwrap();
        assert_eq!(init, PeerInit::PierceFirewall { token: 4242 }, "must echo the server's token");
        peer.send(album_response("pierced", token).encode()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    });

    let mut users = Vec::new();
    while users.len() < 2 {
        let response = timeout(WAIT, search.next()).await.unwrap().expect("search ended early");
        assert_eq!(response.token, token);
        assert_eq!(response.files[0].quality().unwrap().to_string(), "FLAC 16/44.1");
        users.push(response.username);
    }
    users.sort();
    assert_eq!(users, ["direct", "pierced"]);

    direct.await.unwrap();
    pierced.await.unwrap();
}

#[tokio::test]
async fn unreachable_peer_is_reported_to_the_server() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(config(&server));
    let (socket, _) = server.accept().await.unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    conn.send(login_ok()).await.unwrap();
    wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;

    // Port 1 on localhost refuses connections.
    let mut connect = Writer::new();
    connect.string("ghost").string("P").ip(Ipv4Addr::LOCALHOST).u32(1).u32(99).bool(false);
    conn.send(connect.finish(code::CONNECT_TO_PEER)).await.unwrap();

    let body = expect_code(&mut conn, code::CANT_CONNECT_TO_PEER).await;
    let mut r = Reader::new(&body);
    assert_eq!(r.u32().unwrap(), 99);
    assert_eq!(r.string().unwrap(), "ghost");
}

#[tokio::test]
async fn rejected_login_stops_without_retrying() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(config(&server));

    let (socket, _) = server.accept().await.unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    let mut w = Writer::new();
    w.bool(false).string("INVALIDPASS");
    conn.send(w.finish(code::LOGIN)).await.unwrap();

    let state = wait_for(&client, |s| matches!(s, SessionState::Stopped(_))).await;
    assert_eq!(state, SessionState::Stopped(StopReason::LoginRejected(LoginRejection::InvalidPassword)));
    assert!(matches!(client.search("anything").await, Err(Error::NotOnline(_))));

    // No second login attempt.
    assert!(timeout(Duration::from_millis(400), server.accept()).await.is_err());
}

#[tokio::test]
async fn reconnects_after_the_server_drops_us() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(config(&server));

    let (socket, _) = server.accept().await.unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    conn.send(login_ok()).await.unwrap();
    wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;

    drop(conn);
    wait_for(&client, |s| matches!(s, SessionState::Reconnecting { .. })).await;
    assert!(matches!(client.search("offline").await, Err(Error::NotOnline(_))));

    let (socket, _) = timeout(WAIT, server.accept()).await.expect("client should reconnect").unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    conn.send(login_ok()).await.unwrap();
    wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;
}

#[tokio::test]
async fn logged_in_elsewhere_signs_back_in_later() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(Config { relogin_after: Duration::from_millis(300), ..config(&server) });
    let (socket, _) = server.accept().await.unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    conn.send(login_ok()).await.unwrap();
    wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;

    conn.send(Writer::new().finish(code::RELOGGED)).await.unwrap();
    let state = wait_for(&client, |s| matches!(s, SessionState::Reconnecting { .. })).await;
    assert!(
        matches!(&state, SessionState::Reconnecting { reason, retry_in } if reason.contains("another client") && *retry_in == Duration::from_millis(300)),
        "{state:?}"
    );

    // Not stopped for good: it signs back in by itself.
    let (socket, _) = timeout(WAIT, server.accept()).await.expect("client should sign back in").unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    conn.send(login_ok()).await.unwrap();
    wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;
}

#[tokio::test]
async fn remembers_what_the_server_excludes_from_search() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(config(&server));
    let (socket, _) = server.accept().await.unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    conn.send(login_ok()).await.unwrap();
    wait_for(&client, |s| matches!(s, SessionState::Online { .. })).await;
    assert_eq!(client.excluded_phrase("some artist"), None);

    let mut w = Writer::new();
    w.u32(1).string("Some Artist");
    conn.send(w.finish(code::EXCLUDED_SEARCH_PHRASES)).await.unwrap();
    timeout(WAIT, async {
        while client.excluded_phrase("x").is_none() && client.excluded_phrase("SOME ARTIST live").is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the phrase list arrives");
    assert_eq!(client.excluded_phrase("SOME ARTIST live").as_deref(), Some("some artist"));
    assert_eq!(client.excluded_phrase("another artist"), None);
}

#[tokio::test]
async fn empty_queries_are_rejected_locally() {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::start(config(&server));
    assert_eq!(client.search("   ").await.unwrap_err(), Error::EmptyQuery);
}
