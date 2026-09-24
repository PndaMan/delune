//! End-to-end download tests against a fake server and fake uploading peers, over
//! real TCP sockets on localhost.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use delune_soulseek::client::{Client, Config, SessionState};
use delune_soulseek::frame::{FrameCodec, split_code};
use delune_soulseek::peer::{PeerInit, PeerMessage, direction};
use delune_soulseek::server::code;
use delune_soulseek::transfer::{DownloadRequest, DownloadState};
use delune_soulseek::wire::{Reader, Writer};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_util::codec::Framed;

type Conn = Framed<TcpStream, FrameCodec>;
const WAIT: Duration = Duration::from_secs(10);
const FILE: &str = r"@@uploader\Music\Radiohead\OK Computer\02 - Paranoid Android.flac";

fn framed(stream: TcpStream) -> Conn {
    Framed::new(stream, FrameCodec::new(1 << 20))
}

async fn next_frame(conn: &mut Conn) -> Bytes {
    timeout(WAIT, conn.next()).await.expect("timed out waiting for a frame").expect("connection closed").unwrap()
}

async fn expect_code(conn: &mut Conn, wanted: u32) -> Vec<u8> {
    loop {
        let frame = next_frame(conn).await;
        let (message_code, body) = split_code(&frame).unwrap();
        if message_code == wanted {
            return body.to_vec();
        }
    }
}

async fn next_peer_message(conn: &mut Conn) -> PeerMessage {
    let frame = next_frame(conn).await;
    let (message_code, body) = split_code(&frame).unwrap();
    PeerMessage::decode(message_code, body).unwrap().expect("a peer message")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("delune-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample(len: usize) -> Vec<u8> {
    (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect()
}

/// A logged-in client plus the fake server connection and the client's listening port.
async fn online_client() -> (Client, Conn, u16) {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = Config::new("delune-test", "secret");
    config.server = server.local_addr().unwrap().to_string();
    config.listen_port = Some(0);
    let client = Client::start(config);

    let (socket, _) = timeout(WAIT, server.accept()).await.unwrap().unwrap();
    let mut conn = framed(socket);
    expect_code(&mut conn, code::LOGIN).await;
    let mut ok = Writer::new();
    ok.bool(true).string("hi").ip(Ipv4Addr::LOCALHOST).string("hash").bool(false);
    conn.send(ok.finish(code::LOGIN)).await.unwrap();
    let port = u16::try_from(Reader::new(&expect_code(&mut conn, code::SET_WAIT_PORT).await).u32().unwrap()).unwrap();

    let mut state = client.state();
    timeout(WAIT, state.wait_for(|s| matches!(s, SessionState::Online { .. }))).await.unwrap().unwrap();
    // Keep the server connection alive in the background; tests read from it as needed.
    (client, conn, port)
}

/// Answer the client's `GetPeerAddress` with `port` on localhost.
async fn answer_address(server: &mut Conn, port: u16) {
    let body = expect_code(server, code::GET_PEER_ADDRESS).await;
    let username = Reader::new(&body).string().unwrap();
    let mut w = Writer::new();
    w.string(&username).ip(Ipv4Addr::LOCALHOST).u32(u32::from(port)).u32(0).u16(0);
    server.send(w.finish(code::GET_PEER_ADDRESS)).await.unwrap();
}

/// Accept the client's direct P connection, check its handshake, and take the queue request.
async fn accept_queue_request(listener: &TcpListener) -> Conn {
    let (socket, _) = timeout(WAIT, listener.accept()).await.unwrap().unwrap();
    let mut peer = framed(socket);
    let init = PeerInit::decode(&next_frame(&mut peer).await).unwrap();
    assert_eq!(init, PeerInit::PeerInit { username: "delune-test".into(), kind: "P".into(), token: 0 });
    assert_eq!(next_peer_message(&mut peer).await, PeerMessage::QueueUpload { filename: FILE.into() });
    peer
}

/// Tell the client we're ready to upload and check it accepts.
async fn request_transfer(peer: &mut Conn, token: u32, size: u64) {
    peer.send(
        PeerMessage::TransferRequest { direction: direction::UPLOAD, token, filename: FILE.into(), size: Some(size) }
            .encode(),
    )
    .await
    .unwrap();
    assert_eq!(next_peer_message(peer).await, PeerMessage::TransferResponse { token, allowed: true, reason: None });
}

/// Send file bytes from `offset` after checking the client asked for that offset.
async fn upload(mut file_conn: TcpStream, data: &[u8], expected_offset: u64) {
    let mut offset = [0u8; 8];
    timeout(WAIT, file_conn.read_exact(&mut offset)).await.unwrap().unwrap();
    assert_eq!(u64::from_le_bytes(offset), expected_offset);
    let start = usize::try_from(expected_offset).unwrap();
    for chunk in data[start..].chunks(7_919) {
        file_conn.write_all(chunk).await.unwrap();
    }
    // The downloader closes the connection when it has everything.
    let mut rest = Vec::new();
    let _ = timeout(WAIT, file_conn.read_to_end(&mut rest)).await;
}

#[tokio::test]
async fn downloads_over_direct_connections() {
    let (client, mut server, client_port) = online_client().await;
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = temp_dir("direct");
    let destination = dir.join("Radiohead/OK Computer/02 - Paranoid Android.flac");
    let data = sample(300_000);

    let download = client.download(DownloadRequest {
        username: "uploader".into(),
        filename: FILE.into(),
        destination: destination.clone(),
    });
    answer_address(&mut server, peer_listener.local_addr().unwrap().port()).await;

    let mut peer = accept_queue_request(&peer_listener).await;
    peer.send(PeerMessage::PlaceInQueueResponse { filename: FILE.into(), place: 2 }.encode()).await.unwrap();
    let mut state = download.state();
    timeout(WAIT, state.wait_for(|s| *s == DownloadState::Queued { place: Some(2) })).await.unwrap().unwrap();

    request_transfer(&mut peer, 555, data.len() as u64).await;

    // Init frame and token in one write: the client must not lose the token to its frame buffer.
    let mut file_conn = TcpStream::connect(("127.0.0.1", client_port)).await.unwrap();
    let mut hello = PeerInit::PeerInit { username: "uploader".into(), kind: "F".into(), token: 0 }.encode();
    hello.extend_from_slice(&555u32.to_le_bytes());
    file_conn.write_all(&hello).await.unwrap();
    let uploader = tokio::spawn({
        let data = data.clone();
        async move { upload(file_conn, &data, 0).await }
    });

    let outcome = timeout(WAIT, download.finished()).await.unwrap();
    assert_eq!(outcome, DownloadState::Completed { bytes: data.len() as u64 });
    assert_eq!(std::fs::read(&destination).unwrap(), data);
    assert!(!destination.with_file_name("02 - Paranoid Android.flac.part").exists());
    uploader.await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn resumes_a_partial_download() {
    let (client, mut server, client_port) = online_client().await;
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = temp_dir("resume");
    let destination = dir.join("02.flac");
    let data = sample(200_000);
    std::fs::write(dir.join("02.flac.part"), &data[..120_000]).unwrap();

    let download = client.download(DownloadRequest {
        username: "uploader".into(),
        filename: FILE.into(),
        destination: destination.clone(),
    });
    answer_address(&mut server, peer_listener.local_addr().unwrap().port()).await;
    let mut peer = accept_queue_request(&peer_listener).await;
    request_transfer(&mut peer, 77, data.len() as u64).await;

    let mut file_conn = TcpStream::connect(("127.0.0.1", client_port)).await.unwrap();
    file_conn
        .write_all(&PeerInit::PeerInit { username: "uploader".into(), kind: "F".into(), token: 0 }.encode())
        .await
        .unwrap();
    file_conn.write_all(&77u32.to_le_bytes()).await.unwrap();
    let uploader = tokio::spawn({
        let data = data.clone();
        async move { upload(file_conn, &data, 120_000).await }
    });

    assert_eq!(timeout(WAIT, download.finished()).await.unwrap(), DownloadState::Completed { bytes: 200_000 });
    assert_eq!(std::fs::read(&destination).unwrap(), data);
    uploader.await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn reports_a_declined_download() {
    let (client, mut server, _) = online_client().await;
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = temp_dir("denied");

    let download = client.download(DownloadRequest {
        username: "uploader".into(),
        filename: FILE.into(),
        destination: dir.join("x.flac"),
    });
    answer_address(&mut server, peer_listener.local_addr().unwrap().port()).await;
    let mut peer = accept_queue_request(&peer_listener).await;
    peer.send(PeerMessage::UploadDenied { filename: FILE.into(), reason: "File not shared.".into() }.encode())
        .await
        .unwrap();

    assert_eq!(
        timeout(WAIT, download.finished()).await.unwrap(),
        DownloadState::Failed { reason: "uploader declined: File not shared".into() }
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn waits_out_a_peer_that_is_over_its_limits() {
    let (client, mut server, _) = online_client().await;
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = temp_dir("over-limit");

    let download = client.download(DownloadRequest {
        username: "uploader".into(),
        filename: FILE.into(),
        destination: dir.join("x.flac"),
    });
    answer_address(&mut server, peer_listener.local_addr().unwrap().port()).await;
    let mut peer = accept_queue_request(&peer_listener).await;
    peer.send(PeerMessage::UploadDenied { filename: FILE.into(), reason: "Too many megabytes.".into() }.encode())
        .await
        .unwrap();

    // Their queue is full, not our file missing: the download waits instead of failing.
    let mut state = download.state();
    timeout(WAIT, state.wait_for(|s| *s == DownloadState::Queued { place: None })).await.unwrap().unwrap();
    assert!(timeout(Duration::from_millis(250), download.finished()).await.is_err());
    assert_eq!(*download.state().borrow(), DownloadState::Queued { place: None });
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn firewalled_uploader_reaches_us_through_the_server() {
    let (client, mut server, _) = online_client().await;
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let file_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = temp_dir("indirect");
    let destination = dir.join("02.flac");
    let data = sample(64_000);

    let download = client.download(DownloadRequest {
        username: "uploader".into(),
        filename: FILE.into(),
        destination: destination.clone(),
    });
    answer_address(&mut server, peer_listener.local_addr().unwrap().port()).await;
    let mut peer = accept_queue_request(&peer_listener).await;
    request_transfer(&mut peer, 4040, data.len() as u64).await;

    // The uploader can't reach us, so the server tells us to connect to it.
    let mut connect = Writer::new();
    connect
        .string("uploader")
        .string("F")
        .ip(Ipv4Addr::LOCALHOST)
        .u32(u32::from(file_listener.local_addr().unwrap().port()))
        .u32(9001)
        .bool(false);
    server.send(connect.finish(code::CONNECT_TO_PEER)).await.unwrap();

    let uploader = tokio::spawn({
        let data = data.clone();
        async move {
            let (socket, _) = timeout(WAIT, file_listener.accept()).await.unwrap().unwrap();
            let mut conn = framed(socket);
            assert_eq!(
                PeerInit::decode(&next_frame(&mut conn).await).unwrap(),
                PeerInit::PierceFirewall { token: 9001 }
            );
            let mut stream = conn.into_parts().io;
            stream.write_all(&4040u32.to_le_bytes()).await.unwrap();
            upload(stream, &data, 0).await;
        }
    });

    assert_eq!(timeout(WAIT, download.finished()).await.unwrap(), DownloadState::Completed { bytes: 64_000 });
    assert_eq!(std::fs::read(&destination).unwrap(), data);
    uploader.await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
