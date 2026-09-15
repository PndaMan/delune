//! Messages exchanged with the Soulseek server (`server.slsknet.org:2242`).
//!
//! The server is a rendezvous point: it authenticates us, relays searches to other
//! users, and tells us how to reach peers. Actual search results and file transfers
//! flow peer-to-peer (see [`crate::peer`]).
//!
//! Only the messages delune needs are modelled. Anything else decodes to
//! [`ServerEvent::Unhandled`] so an unfamiliar message never kills the connection.

use std::net::Ipv4Addr;

use bytes::BytesMut;

use crate::wire::{DecodeError, Reader, Writer, md5_hex};

/// Protocol version we announce. 160 is what current Soulseek clients send.
pub const CLIENT_VERSION: u32 = 160;
/// Minor version, sent alongside [`CLIENT_VERSION`].
pub const CLIENT_MINOR_VERSION: u32 = 1;

pub mod code {
    pub const LOGIN: u32 = 1;
    pub const SET_WAIT_PORT: u32 = 2;
    pub const GET_PEER_ADDRESS: u32 = 3;
    pub const WATCH_USER: u32 = 5;
    pub const UNWATCH_USER: u32 = 6;
    pub const GET_USER_STATUS: u32 = 7;
    pub const SAY_CHATROOM: u32 = 13;
    pub const JOIN_ROOM: u32 = 14;
    pub const LEAVE_ROOM: u32 = 15;
    pub const USER_JOINED_ROOM: u32 = 16;
    pub const USER_LEFT_ROOM: u32 = 17;
    pub const MESSAGE_USER: u32 = 22;
    pub const MESSAGE_ACKED: u32 = 23;
    pub const ROOM_LIST: u32 = 64;
    pub const WISHLIST_SEARCH: u32 = 103;
    pub const WISHLIST_INTERVAL: u32 = 104;
    pub const SEND_UPLOAD_SPEED: u32 = 121;
    pub const GET_USER_STATS: u32 = 36;
    pub const CONNECT_TO_PEER: u32 = 18;
    pub const FILE_SEARCH: u32 = 26;
    pub const SET_STATUS: u32 = 28;
    pub const PING: u32 = 32;
    pub const SHARED_FOLDERS_FILES: u32 = 35;
    pub const HAVE_NO_PARENT: u32 = 71;
    pub const RELOGGED: u32 = 41;
    pub const EXCLUDED_SEARCH_PHRASES: u32 = 160;
    pub const CANT_CONNECT_TO_PEER: u32 = 1001;
}

/// Our online status as shown to other users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Offline = 0,
    Away = 1,
    Online = 2,
}

impl Status {
    const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Away,
            2 => Self::Online,
            _ => Self::Offline,
        }
    }
}

/// What the server knows about another user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPresence {
    pub username: String,
    /// False when no account has that name.
    pub exists: bool,
    pub status: Status,
    /// Average upload speed in bytes per second, as measured by the server.
    pub avg_speed: u32,
    pub upload_count: u32,
    pub files: u32,
    pub folders: u32,
    /// Uppercase country code, when the user is online.
    pub country: Option<String>,
}

/// Someone in a chat room, as the server describes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomMember {
    pub username: String,
    pub status: Status,
    pub avg_speed: u32,
    pub files: u32,
    pub folders: u32,
    pub slots_full: bool,
    pub country: Option<String>,
}

/// A public room and how many people are in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomSummary {
    pub name: String,
    pub users: u32,
}

/// The kind of peer connection being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    /// Messages: search results, browsing, transfer negotiation.
    Peer,
    /// A file transfer.
    File,
    /// Distributed search network.
    Distributed,
}

impl ConnectionType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Peer => "P",
            Self::File => "F",
            Self::Distributed => "D",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "P" => Self::Peer,
            "F" => Self::File,
            "D" => Self::Distributed,
            _ => return None,
        })
    }
}

/// Messages we send to the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRequest {
    Login {
        username: String,
        password: String,
    },
    SetWaitPort {
        port: u32,
    },
    SetStatus(Status),
    SharedFoldersFiles {
        folders: u32,
        files: u32,
    },
    HaveNoParent(bool),
    GetPeerAddress {
        username: String,
    },
    /// Get a user's status and stats now, and status changes from then on.
    WatchUser {
        username: String,
    },
    UnwatchUser {
        username: String,
    },
    /// A private message to another user.
    MessageUser {
        username: String,
        message: String,
    },
    /// Confirm we received a private message, so the server stops resending it.
    MessageAcked {
        id: u32,
    },
    JoinRoom {
        room: String,
    },
    LeaveRoom {
        room: String,
    },
    SayChatroom {
        room: String,
        message: String,
    },
    RoomList,
    /// A saved search, sent at the server's wishlist interval instead of counting
    /// against the normal search limits.
    WishlistSearch {
        token: u32,
        query: String,
    },
    /// Our average speed for a finished upload, for the server's stats.
    SendUploadSpeed {
        speed: u32,
    },
    ConnectToPeer {
        token: u32,
        username: String,
        kind: ConnectionType,
    },
    FileSearch {
        token: u32,
        query: String,
    },
    CantConnectToPeer {
        token: u32,
        username: String,
    },
    Ping,
}

impl ServerRequest {
    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        let code = match self {
            Self::Login { username, password } => {
                let hash = md5_hex(format!("{username}{password}").as_bytes());
                w.string(username).string(password).u32(CLIENT_VERSION).string(&hash).u32(CLIENT_MINOR_VERSION);
                code::LOGIN
            }
            Self::SetWaitPort { port } => {
                w.u32(*port);
                code::SET_WAIT_PORT
            }
            Self::SetStatus(status) => {
                w.u32(*status as u32);
                code::SET_STATUS
            }
            Self::SharedFoldersFiles { folders, files } => {
                w.u32(*folders).u32(*files);
                code::SHARED_FOLDERS_FILES
            }
            Self::HaveNoParent(v) => {
                w.bool(*v);
                code::HAVE_NO_PARENT
            }
            Self::GetPeerAddress { username } => {
                w.string(username);
                code::GET_PEER_ADDRESS
            }
            Self::WatchUser { username } => {
                w.string(username);
                code::WATCH_USER
            }
            Self::UnwatchUser { username } => {
                w.string(username);
                code::UNWATCH_USER
            }
            Self::MessageUser { username, message } => {
                w.string(username).string(message);
                code::MESSAGE_USER
            }
            Self::MessageAcked { id } => {
                w.u32(*id);
                code::MESSAGE_ACKED
            }
            Self::JoinRoom { room } => {
                // Rooms we create are public.
                w.string(room).u32(0);
                code::JOIN_ROOM
            }
            Self::LeaveRoom { room } => {
                w.string(room);
                code::LEAVE_ROOM
            }
            Self::SayChatroom { room, message } => {
                w.string(room).string(message);
                code::SAY_CHATROOM
            }
            Self::RoomList => code::ROOM_LIST,
            Self::WishlistSearch { token, query } => {
                w.u32(*token).string(query);
                code::WISHLIST_SEARCH
            }
            Self::SendUploadSpeed { speed } => {
                w.u32(*speed);
                code::SEND_UPLOAD_SPEED
            }
            Self::ConnectToPeer { token, username, kind } => {
                w.u32(*token).string(username).string(kind.as_str());
                code::CONNECT_TO_PEER
            }
            Self::FileSearch { token, query } => {
                w.u32(*token).string(query);
                code::FILE_SEARCH
            }
            Self::CantConnectToPeer { token, username } => {
                w.u32(*token).string(username);
                code::CANT_CONNECT_TO_PEER
            }
            Self::Ping => code::PING,
        };
        w.finish(code)
    }
}

/// Why the server refused a login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginRejection {
    InvalidUsername,
    InvalidPassword,
    InvalidVersion,
    Other(String),
}

/// Messages the server sends us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    LoginOk {
        greeting: String,
        public_ip: Ipv4Addr,
        supporter: bool,
    },
    LoginRejected(LoginRejection),
    PeerAddress {
        username: String,
        ip: Ipv4Addr,
        port: u32,
    },
    /// A peer wants us to connect to them (they couldn't reach us directly).
    ConnectToPeer {
        username: String,
        kind: Option<ConnectionType>,
        ip: Ipv4Addr,
        port: u32,
        token: u32,
    },
    /// Someone is searching; if we share matching files we answer them directly.
    FileSearch {
        username: String,
        token: u32,
        query: String,
    },
    /// The same account logged in somewhere else, and the server disconnected us.
    Relogged,
    /// Answer to `WatchUser`.
    WatchedUser(UserPresence),
    /// A watched user went away, came back or went offline.
    UserStatus {
        username: String,
        status: Status,
        privileged: bool,
    },
    UserStats {
        username: String,
        avg_speed: u32,
        upload_count: u32,
        files: u32,
        folders: u32,
    },
    PrivateMessage {
        id: u32,
        /// Unix seconds.
        timestamp: u32,
        username: String,
        message: String,
        /// False when the server is resending something we missed while offline.
        new: bool,
    },
    /// We joined a room; everyone in it.
    JoinedRoom {
        room: String,
        members: Vec<RoomMember>,
    },
    LeftRoom {
        room: String,
    },
    RoomMessage {
        room: String,
        username: String,
        message: String,
    },
    UserJoinedRoom {
        room: String,
        member: RoomMember,
    },
    UserLeftRoom {
        room: String,
        username: String,
    },
    RoomList(Vec<RoomSummary>),
    /// How often we may send a wishlist search.
    WishlistInterval(u32),
    /// Phrases the network excludes from search results. Peers drop matching files,
    /// so searching for one of these returns nothing.
    ExcludedSearchPhrases(Vec<String>),
    Unhandled {
        code: u32,
        len: usize,
    },
}

impl ServerEvent {
    /// Decode a message body (after the length and code have been stripped).
    pub fn decode(code: u32, body: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(body);
        Ok(match code {
            code::LOGIN => {
                if r.bool()? {
                    let greeting = r.string()?;
                    let public_ip = r.ip()?;
                    let _password_hash = r.string()?;
                    // Older servers omit the supporter flag.
                    let supporter = if r.remaining() > 0 { r.bool()? } else { false };
                    Self::LoginOk { greeting, public_ip, supporter }
                } else {
                    Self::LoginRejected(match r.string()?.as_str() {
                        "INVALIDUSERNAME" => LoginRejection::InvalidUsername,
                        "INVALIDPASS" => LoginRejection::InvalidPassword,
                        "INVALIDVERSION" => LoginRejection::InvalidVersion,
                        other => LoginRejection::Other(other.to_owned()),
                    })
                }
            }
            code::GET_PEER_ADDRESS => Self::PeerAddress { username: r.string()?, ip: r.ip()?, port: r.u32()? },
            code::CONNECT_TO_PEER => {
                let username = r.string()?;
                let kind = ConnectionType::parse(&r.string()?);
                Self::ConnectToPeer { username, kind, ip: r.ip()?, port: r.u32()?, token: r.u32()? }
            }
            code::FILE_SEARCH => Self::FileSearch { username: r.string()?, token: r.u32()?, query: r.string()? },
            code::RELOGGED => Self::Relogged,
            code::WATCH_USER => decode_watched_user(&mut r)?,
            code::GET_USER_STATUS => {
                let username = r.string()?;
                let status = Status::from_u32(r.u32()?);
                let privileged = r.remaining() > 0 && r.bool()?;
                Self::UserStatus { username, status, privileged }
            }
            code::MESSAGE_USER => Self::PrivateMessage {
                id: r.u32()?,
                timestamp: r.u32()?,
                username: r.string()?,
                message: r.string()?,
                new: r.remaining() == 0 || r.bool()?,
            },
            code::SAY_CHATROOM => Self::RoomMessage { room: r.string()?, username: r.string()?, message: r.string()? },
            code::LEAVE_ROOM => Self::LeftRoom { room: r.string()? },
            code::USER_LEFT_ROOM => Self::UserLeftRoom { room: r.string()?, username: r.string()? },
            code::USER_JOINED_ROOM => decode_member_joined(&mut r)?,
            code::JOIN_ROOM => decode_joined_room(&mut r)?,
            code::WISHLIST_INTERVAL => Self::WishlistInterval(r.u32()?),
            code::ROOM_LIST => {
                let names: Vec<String> = (0..r.count(4)?).map(|_| r.string()).collect::<Result<_, _>>()?;
                let counts: Vec<u32> = (0..r.count(4)?).map(|_| r.u32()).collect::<Result<_, _>>()?;
                // Private room sections follow; delune only lists public rooms.
                Self::RoomList(names.into_iter().zip(counts).map(|(name, users)| RoomSummary { name, users }).collect())
            }
            code::GET_USER_STATS => {
                let username = r.string()?;
                let avg_speed = r.u32()?;
                let upload_count = r.u32()?;
                let _unknown = r.u32()?;
                Self::UserStats { username, avg_speed, upload_count, files: r.u32()?, folders: r.u32()? }
            }
            code::EXCLUDED_SEARCH_PHRASES => {
                let count = r.count(4)?;
                Self::ExcludedSearchPhrases((0..count).map(|_| r.string()).collect::<Result<_, _>>()?)
            }
            other => Self::Unhandled { code: other, len: body.len() },
        })
    }
}

fn decode_joined_room(r: &mut Reader<'_>) -> Result<ServerEvent, DecodeError> {
    let room = r.string()?;
    let names: Vec<String> = (0..r.count(4)?).map(|_| r.string()).collect::<Result<_, _>>()?;
    let statuses: Vec<u32> = (0..r.count(4)?).map(|_| r.u32()).collect::<Result<_, _>>()?;
    let stats: Vec<[u32; 5]> = (0..r.count(20)?)
        .map(|_| Ok([r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?]))
        .collect::<Result<_, DecodeError>>()?;
    let slots: Vec<u32> = (0..r.count(4)?).map(|_| r.u32()).collect::<Result<_, _>>()?;
    let countries: Vec<String> =
        if r.remaining() >= 4 { (0..r.count(4)?).map(|_| r.string()).collect::<Result<_, _>>()? } else { Vec::new() };
    let members = names
        .into_iter()
        .enumerate()
        .map(|(i, username)| RoomMember {
            username,
            status: statuses.get(i).map_or(Status::Online, |s| Status::from_u32(*s)),
            avg_speed: stats.get(i).map_or(0, |s| s[0]),
            files: stats.get(i).map_or(0, |s| s[3]),
            folders: stats.get(i).map_or(0, |s| s[4]),
            slots_full: slots.get(i).is_some_and(|s| *s != 0),
            country: countries.get(i).filter(|c| !c.is_empty()).cloned(),
        })
        .collect();
    Ok(ServerEvent::JoinedRoom { room, members })
}

fn decode_member_joined(r: &mut Reader<'_>) -> Result<ServerEvent, DecodeError> {
    let room = r.string()?;
    let username = r.string()?;
    let status = Status::from_u32(r.u32()?);
    let avg_speed = r.u32()?;
    let _uploads = r.u32()?;
    let _unknown = r.u32()?;
    let files = r.u32()?;
    let folders = r.u32()?;
    let slots_full = r.u32()? != 0;
    let country = if r.remaining() >= 4 { Some(r.string()?).filter(|c| !c.is_empty()) } else { None };
    Ok(ServerEvent::UserJoinedRoom {
        room,
        member: RoomMember { username, status, avg_speed, files, folders, slots_full, country },
    })
}

fn decode_watched_user(r: &mut Reader<'_>) -> Result<ServerEvent, DecodeError> {
    let username = r.string()?;
    let exists = r.bool()?;
    let mut presence = UserPresence {
        username,
        exists,
        status: Status::Offline,
        avg_speed: 0,
        upload_count: 0,
        files: 0,
        folders: 0,
        country: None,
    };
    if exists {
        presence.status = Status::from_u32(r.u32()?);
        presence.avg_speed = r.u32()?;
        presence.upload_count = r.u32()?;
        let _unknown = r.u32()?;
        presence.files = r.u32()?;
        presence.folders = r.u32()?;
        if presence.status != Status::Offline && r.remaining() >= 4 {
            presence.country = Some(r.string()?).filter(|c| !c.is_empty());
        }
    }
    Ok(ServerEvent::WatchedUser(presence))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(frame: &[u8]) -> (u32, &[u8]) {
        let mut r = Reader::new(frame);
        let len = r.u32().unwrap() as usize;
        assert_eq!(len, frame.len() - 4, "length prefix must cover code + body");
        (r.u32().unwrap(), &frame[8..])
    }

    #[test]
    fn login_request_layout() {
        let frame = ServerRequest::Login { username: "delune".into(), password: "hunter2".into() }.encode();
        let (code, body) = body_of(&frame);
        assert_eq!(code, code::LOGIN);
        let mut r = Reader::new(body);
        assert_eq!(r.string().unwrap(), "delune");
        assert_eq!(r.string().unwrap(), "hunter2");
        assert_eq!(r.u32().unwrap(), CLIENT_VERSION);
        assert_eq!(r.string().unwrap(), md5_hex(b"delunehunter2"));
        assert_eq!(r.u32().unwrap(), CLIENT_MINOR_VERSION);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn decodes_login_success_and_failure() {
        let mut ok = Writer::new();
        ok.bool(true).string("Welcome").ip(Ipv4Addr::new(1, 2, 3, 4)).string("abc").bool(true);
        assert_eq!(
            ServerEvent::decode(code::LOGIN, &ok.into_body()).unwrap(),
            ServerEvent::LoginOk { greeting: "Welcome".into(), public_ip: Ipv4Addr::new(1, 2, 3, 4), supporter: true }
        );

        let mut bad = Writer::new();
        bad.bool(false).string("INVALIDPASS");
        assert_eq!(
            ServerEvent::decode(code::LOGIN, &bad.into_body()).unwrap(),
            ServerEvent::LoginRejected(LoginRejection::InvalidPassword)
        );
    }

    #[test]
    fn decodes_connect_to_peer() {
        let mut w = Writer::new();
        w.string("alice").string("P").ip(Ipv4Addr::new(192, 168, 1, 9)).u32(2234).u32(77).bool(false);
        assert_eq!(
            ServerEvent::decode(code::CONNECT_TO_PEER, &w.into_body()).unwrap(),
            ServerEvent::ConnectToPeer {
                username: "alice".into(),
                kind: Some(ConnectionType::Peer),
                ip: Ipv4Addr::new(192, 168, 1, 9),
                port: 2234,
                token: 77
            }
        );
    }

    #[test]
    fn decodes_excluded_phrases() {
        let mut w = Writer::new();
        w.u32(2).string("some artist").string("another phrase");
        assert_eq!(
            ServerEvent::decode(code::EXCLUDED_SEARCH_PHRASES, &w.into_body()).unwrap(),
            ServerEvent::ExcludedSearchPhrases(vec!["some artist".into(), "another phrase".into()])
        );
    }

    #[test]
    fn decodes_watched_users() {
        let mut w = Writer::new();
        w.string("alice").bool(true).u32(2).u32(125_000).u32(40).u32(0).u32(12_000).u32(900).string("NZ");
        assert_eq!(
            ServerEvent::decode(code::WATCH_USER, &w.into_body()).unwrap(),
            ServerEvent::WatchedUser(UserPresence {
                username: "alice".into(),
                exists: true,
                status: Status::Online,
                avg_speed: 125_000,
                upload_count: 40,
                files: 12_000,
                folders: 900,
                country: Some("NZ".into()),
            })
        );

        let mut missing = Writer::new();
        missing.string("nobody").bool(false);
        let ServerEvent::WatchedUser(presence) = ServerEvent::decode(code::WATCH_USER, &missing.into_body()).unwrap()
        else {
            panic!("expected a watched user")
        };
        assert!(!presence.exists);
    }

    #[test]
    fn decodes_chat() {
        let mut w = Writer::new();
        w.u32(12).u32(1_789_000_000).string("alice").string("hello there").bool(true);
        assert_eq!(
            ServerEvent::decode(code::MESSAGE_USER, &w.into_body()).unwrap(),
            ServerEvent::PrivateMessage {
                id: 12,
                timestamp: 1_789_000_000,
                username: "alice".into(),
                message: "hello there".into(),
                new: true
            }
        );

        let mut w = Writer::new();
        w.string("ambient").u32(2).string("alice").string("bob");
        w.u32(2).u32(2).u32(1);
        w.u32(2);
        for files in [100, 200] {
            w.u32(1000).u32(0).u32(0).u32(files).u32(10);
        }
        w.u32(2).u32(0).u32(1);
        w.u32(2).string("NZ").string("");
        let ServerEvent::JoinedRoom { room, members } = ServerEvent::decode(code::JOIN_ROOM, &w.into_body()).unwrap()
        else {
            panic!("expected a joined room")
        };
        assert_eq!(room, "ambient");
        assert_eq!(members.len(), 2);
        assert_eq!(
            (members[0].status, members[0].files, members[0].country.as_deref()),
            (Status::Online, 100, Some("NZ"))
        );
        assert_eq!(
            (members[1].status, members[1].slots_full, members[1].country.as_deref()),
            (Status::Away, true, None)
        );

        let mut w = Writer::new();
        w.u32(2).string("ambient").string("jazz").u32(2).u32(40).u32(12).u32(0).u32(0).u32(0).u32(0).u32(0);
        assert_eq!(
            ServerEvent::decode(code::ROOM_LIST, &w.into_body()).unwrap(),
            ServerEvent::RoomList(vec![
                RoomSummary { name: "ambient".into(), users: 40 },
                RoomSummary { name: "jazz".into(), users: 12 }
            ])
        );
    }

    #[test]
    fn unknown_codes_do_not_error() {
        assert_eq!(ServerEvent::decode(9999, &[1, 2, 3]).unwrap(), ServerEvent::Unhandled { code: 9999, len: 3 });
    }
}
