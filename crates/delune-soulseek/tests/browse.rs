//! Browsing, profiles and chat against a fake server and fake peers, over real TCP sockets.

use std::net::Ipv4Addr;
use std::time::Duration;

use bytes::Bytes;
use delune_soulseek::client::{Client, Config, SessionState};
use delune_soulseek::frame::{FrameCodec, split_code};
use delune_soulseek::peer::{PeerInit, PeerMessage, SharedFile, code as peer_code};
use delune_soulseek::server::code;
use delune_soulseek::shares::{SharedDirectory, SharedFileList, UserInfo};
use delune_soulseek::wire::{Reader, Writer};
use delune_soulseek::{ChatEvent, PeerError, UserStatus};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_util::codec::Framed;

type Conn = Framed<TcpStream, FrameCodec>;
const WAIT: Duration = Duration::from_secs(10);

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

async fn accept_peer(listener: &TcpListener) -> Conn {
    let (socket, _) = timeout(WAIT, listener.accept()).await.unwrap().unwrap();
    let mut peer = framed(socket);
    let init = PeerInit::decode(&next_frame(&mut peer).await).unwrap();
    assert!(matches!(init, PeerInit::PeerInit { ref kind, .. } if kind == "P"));
    peer
}

fn shares() -> SharedFileList {
    let file = |path: &str| SharedFile {
        path: path.into(),
        size: 25_000_000,
        extension: "flac".into(),
        bitrate_kbps: None,
        duration_secs: Some(200),
        vbr: false,
        sample_rate: Some(44_100),
        bit_depth: Some(16),
    };
    SharedFileList {
        directories: vec![SharedDirectory {
            path: r"@@lib\Boards of Canada\Twoism".into(),
            files: vec![
                file(r"@@lib\Boards of Canada\Twoism\01 Sixtyniner.flac"),
                file(r"@@lib\Boards of Canada\Twoism\02 Oirectine.flac"),
            ],
        }],
        private_directories: vec![],
    }
}

#[tokio::test]
async fn browses_a_user_and_reads_their_profile() {
    let (client, mut server, _) = online_client().await;
    let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

    let browse = tokio::spawn({
        let client = client.clone();
        async move { client.browse("sharer").await }
    });
    answer_address(&mut server, peer_listener.local_addr().unwrap().port()).await;
    let mut peer = accept_peer(&peer_listener).await;
    expect_code(&mut peer, peer_code::SHARED_FILE_LIST_REQUEST).await;
    peer.send(shares().encode()).await.unwrap();
    let list = timeout(WAIT, browse).await.unwrap().unwrap().unwrap();
    assert_eq!(*list, shares());

    // The profile request reuses the open connection.
    let info = tokio::spawn({
        let client = client.clone();
        async move { client.user_info("sharer").await }
    });
    expect_code(&mut peer, peer_code::USER_INFO_REQUEST).await;
    let profile = UserInfo { description: "hi".into(), queue_size: 3, slots_free: true, ..UserInfo::default() };
    peer.send(profile.encode()).await.unwrap();
    assert_eq!(timeout(WAIT, info).await.unwrap().unwrap().unwrap(), profile);
}

#[tokio::test]
async fn reports_presence_from_the_server() {
    let (client, mut server, _) = online_client().await;
    let presence = tokio::spawn({
        let client = client.clone();
        async move { client.user_presence("alice").await }
    });
    let body = expect_code(&mut server, code::WATCH_USER).await;
    assert_eq!(Reader::new(&body).string().unwrap(), "alice");
    let mut w = Writer::new();
    w.string("alice").bool(true).u32(1).u32(50_000).u32(3).u32(0).u32(400).u32(20).string("DE");
    server.send(w.finish(code::WATCH_USER)).await.unwrap();

    let presence = timeout(WAIT, presence).await.unwrap().unwrap().unwrap();
    assert_eq!((presence.status, presence.files, presence.country.as_deref()), (UserStatus::Away, 400, Some("DE")));
    expect_code(&mut server, code::UNWATCH_USER).await;
}

#[tokio::test]
async fn answers_people_browsing_us() {
    let (client, _server, port) = online_client().await;
    client.set_shares(shares());

    let mut browser = framed(TcpStream::connect(("127.0.0.1", port)).await.unwrap());
    browser.send(PeerInit::PeerInit { username: "curious".into(), kind: "P".into(), token: 0 }.encode()).await.unwrap();
    browser.send(PeerMessage::SharedFileListRequest.encode()).await.unwrap();
    let body = expect_code(&mut browser, peer_code::SHARED_FILE_LIST_RESPONSE).await;
    assert_eq!(SharedFileList::decode(&body).unwrap(), shares());

    browser.send(PeerMessage::UserInfoRequest.encode()).await.unwrap();
    let body = expect_code(&mut browser, peer_code::USER_INFO_RESPONSE).await;
    assert!(UserInfo::decode(&body).is_ok());
}

#[tokio::test]
async fn browsing_offline_fails_fast() {
    let client = Client::start(Config { server: "127.0.0.1:1".into(), listen_port: None, ..Config::new("x", "y") });
    assert_eq!(client.browse("anyone").await.unwrap_err(), PeerError::Offline);
}

#[tokio::test]
async fn private_messages_arrive_and_are_acknowledged() {
    let (client, mut server, _) = online_client().await;
    let mut chat = client.chat_events();

    let mut w = Writer::new();
    w.u32(77).u32(1_789_000_000).string("alice").string("got any Talk Talk?").bool(true);
    server.send(w.finish(code::MESSAGE_USER)).await.unwrap();

    let event = timeout(WAIT, chat.recv()).await.unwrap().unwrap();
    assert_eq!(
        event,
        ChatEvent::PrivateMessage {
            timestamp: 1_789_000_000,
            username: "alice".into(),
            message: "got any Talk Talk?".into()
        }
    );
    assert_eq!(Reader::new(&expect_code(&mut server, code::MESSAGE_ACKED).await).u32().unwrap(), 77);

    client.send_message("alice", "Spirit of Eden, yes").unwrap();
    let body = expect_code(&mut server, code::MESSAGE_USER).await;
    let mut r = Reader::new(&body);
    assert_eq!((r.string().unwrap(), r.string().unwrap()), ("alice".into(), "Spirit of Eden, yes".into()));
}

#[tokio::test]
async fn rooms_can_be_joined_and_spoken_in() {
    let (client, mut server, _) = online_client().await;
    let mut chat = client.chat_events();

    client.join_room("ambient").unwrap();
    assert_eq!(Reader::new(&expect_code(&mut server, code::JOIN_ROOM).await).string().unwrap(), "ambient");
    let mut w = Writer::new();
    w.string("ambient")
        .u32(1)
        .string("bob")
        .u32(1)
        .u32(2)
        .u32(1)
        .u32(0)
        .u32(0)
        .u32(0)
        .u32(10)
        .u32(1)
        .u32(1)
        .u32(0)
        .u32(1)
        .string("GB");
    server.send(w.finish(code::JOIN_ROOM)).await.unwrap();
    let ChatEvent::JoinedRoom { room, members } = timeout(WAIT, chat.recv()).await.unwrap().unwrap() else {
        panic!("expected to join")
    };
    assert_eq!((room.as_str(), members[0].username.as_str()), ("ambient", "bob"));

    client.say("ambient", "hello").unwrap();
    let body = expect_code(&mut server, code::SAY_CHATROOM).await;
    let mut r = Reader::new(&body);
    assert_eq!((r.string().unwrap(), r.string().unwrap()), ("ambient".into(), "hello".into()));

    assert!(client.say("ambient", "   ").is_err(), "blank messages aren't sent");
}
