//! Uploading and answering searches, against a fake server and a fake downloader,
//! over real TCP sockets on localhost.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use delune_soulseek::client::{Client, Config, SessionState};
use delune_soulseek::frame::{FrameCodec, split_code};
use delune_soulseek::peer::{PeerInit, PeerMessage, SearchResponse, SharedFile, code as peer_code, direction};
use delune_soulseek::server::code;
use delune_soulseek::wire::{Reader, Writer};
use delune_soulseek::{IndexedFile, ShareIndex, UploadState};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_util::codec::Framed;

type Conn = Framed<TcpStream, FrameCodec>;
const WAIT: Duration = Duration::from_secs(10);
const SHARED: &str = r"Music\Talk Talk\Spirit of Eden\01 The Rainbow.flac";

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
    loop {
        let frame = next_frame(conn).await;
        let (message_code, body) = split_code(&frame).unwrap();
        if let Some(message) = PeerMessage::decode(message_code, body).unwrap() {
            return message;
        }
    }
}

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
    (client, conn, port)
}

async fn answer_address(server: &mut Conn, port: u16) {
    let body = expect_code(server, code::GET_PEER_ADDRESS).await;
    let username = Reader::new(&body).string().unwrap();
    let mut w = Writer::new();
    w.string(&username).ip(Ipv4Addr::LOCALHOST).u32(u32::from(port)).u32(0).u16(0);
    server.send(w.finish(code::GET_PEER_ADDRESS)).await.unwrap();
}

fn shared_file(dir: &std::path::Path, data: &[u8]) -> ShareIndex {
    let disk_path = dir.join("01 The Rainbow.flac");
    std::fs::write(&disk_path, data).unwrap();
    ShareIndex::new(vec![IndexedFile {
        file: SharedFile {
            path: SHARED.into(),
            size: data.len() as u64,
            extension: "flac".into(),
            bitrate_kbps: None,
            duration_secs: Some(560),
            vbr: false,
            sample_rate: Some(44_100),
            bit_depth: Some(16),
        },
        disk_path,
    }])
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("delune-share-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn uploads_a_shared_file_and_refuses_others() {
    let (client, mut server, client_port) = online_client().await;
    let dir = temp_dir("upload");
    let data: Vec<u8> = (0..250_000).map(|i| u8::try_from(i % 241).unwrap()).collect();
    client.set_share_index(shared_file(&dir, &data));
    let mut changes = client.uploads_changed();

    // The downloader connects to us and asks for two files, one we don't share.
    let downloader = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut peer = framed(TcpStream::connect(("127.0.0.1", client_port)).await.unwrap());
    peer.send(PeerInit::PeerInit { username: "listener".into(), kind: "P".into(), token: 0 }.encode()).await.unwrap();
    peer.send(PeerMessage::QueueUpload { filename: r"Music\secret.flac".into() }.encode()).await.unwrap();
    assert_eq!(
        next_peer_message(&mut peer).await,
        PeerMessage::UploadDenied { filename: r"Music\secret.flac".into(), reason: "File not shared.".into() }
    );
    peer.send(PeerMessage::QueueUpload { filename: SHARED.into() }.encode()).await.unwrap();

    // A slot is free, so we offer the file straight away on the same connection.
    let PeerMessage::TransferRequest { direction: dir_code, token, filename, size } =
        next_peer_message(&mut peer).await
    else {
        panic!("expected a transfer request")
    };
    assert_eq!((dir_code, filename.as_str(), size), (direction::UPLOAD, SHARED, Some(data.len() as u64)));
    peer.send(PeerMessage::TransferResponse { token, allowed: true, reason: None }.encode()).await.unwrap();

    // We connect to the downloader directly for the file data.
    answer_address(&mut server, downloader.local_addr().unwrap().port()).await;
    let (socket, _) = timeout(WAIT, downloader.accept()).await.unwrap().unwrap();
    let mut file_conn = framed(socket);
    let init = PeerInit::decode(&next_frame(&mut file_conn).await).unwrap();
    assert!(matches!(init, PeerInit::PeerInit { ref kind, .. } if kind == "F"));
    // The frame reader may already hold the token bytes that followed the init message.
    let parts = file_conn.into_parts();
    let (mut raw, mut buffered) = (parts.io, parts.read_buf.to_vec());
    while buffered.len() < 4 {
        let mut chunk = [0u8; 4];
        let n = timeout(WAIT, raw.read(&mut chunk)).await.unwrap().unwrap();
        buffered.extend_from_slice(&chunk[..n]);
    }
    assert_eq!(u32::from_le_bytes(buffered[..4].try_into().unwrap()), token);

    // Resume from 100 kB, like a partial download would.
    raw.write_all(&100_000u64.to_le_bytes()).await.unwrap();
    let mut received = vec![0u8; data.len() - 100_000];
    timeout(WAIT, raw.read_exact(&mut received)).await.unwrap().unwrap();
    assert_eq!(received, data[100_000..]);
    drop(raw);

    let done = timeout(WAIT, async {
        loop {
            if let Some(upload) = client.uploads().into_iter().find(|u| u.filename == SHARED)
                && upload.state.is_finished()
            {
                return upload.state;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(done, UploadState::Completed { bytes: data.len() as u64 });
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn answers_searches_that_match_our_shares() {
    let (client, mut server, _) = online_client().await;
    let dir = temp_dir("search");
    client.set_share_index(shared_file(&dir, b"flac"));

    // Someone searches; the server relays it to us.
    let searcher = TcpListener::bind("127.0.0.1:0").await.unwrap();
    for query in ["nothing we have", "talk talk rainbow"] {
        let mut w = Writer::new();
        w.string("seeker").u32(4242).string(query);
        server.send(w.finish(code::FILE_SEARCH)).await.unwrap();
    }

    answer_address(&mut server, searcher.local_addr().unwrap().port()).await;
    let (socket, _) = timeout(WAIT, searcher.accept()).await.unwrap().unwrap();
    let mut conn = framed(socket);
    let _init = next_frame(&mut conn).await;
    let body = expect_code(&mut conn, peer_code::SEARCH_RESPONSE).await;
    let response = SearchResponse::decode(&body).unwrap();
    assert_eq!((response.username.as_str(), response.token, response.files.len()), ("delune-test", 4242, 1));
    assert_eq!(response.files[0].path, SHARED);
    std::fs::remove_dir_all(dir).unwrap();
}
