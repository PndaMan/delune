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
    Login { username: String, password: String },
    SetWaitPort { port: u32 },
    SetStatus(Status),
    SharedFoldersFiles { folders: u32, files: u32 },
    HaveNoParent(bool),
    GetPeerAddress { username: String },
    ConnectToPeer { token: u32, username: String, kind: ConnectionType },
    FileSearch { token: u32, query: String },
    CantConnectToPeer { token: u32, username: String },
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
            code::EXCLUDED_SEARCH_PHRASES => {
                let count = r.count(4)?;
                Self::ExcludedSearchPhrases((0..count).map(|_| r.string()).collect::<Result<_, _>>()?)
            }
            other => Self::Unhandled { code: other, len: body.len() },
        })
    }
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
    fn unknown_codes_do_not_error() {
        assert_eq!(ServerEvent::decode(9999, &[1, 2, 3]).unwrap(), ServerEvent::Unhandled { code: 9999, len: 3 });
    }
}
