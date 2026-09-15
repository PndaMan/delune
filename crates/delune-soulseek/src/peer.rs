//! Peer messages: search results and the handshake that starts a download.
//!
//! When we search, the server fans the query out and matching peers open a
//! connection *to us* and send a zlib-compressed [`SearchResponse`]. Everything the
//! ranking engine knows about a Soulseek candidate comes from this one message: the
//! file list, the audio attributes each peer chose to report, and the peer's upload
//! slot, speed and queue.
//!
//! Downloading is a conversation over the same kind of connection (see
//! [`PeerMessage`]): we ask the peer to queue a file, the peer tells us when it's
//! ready, and the bytes then flow over a separate file connection
//! ([`crate::transfer`]).

use std::io::{Read, Write as _};

use bytes::BytesMut;
use delune_core::{Codec, Quality};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

use crate::wire::{DecodeError, Reader, Writer};

pub mod code {
    /// Peer init: sent first on a connection we opened to answer `ConnectToPeer`.
    pub const PIERCE_FIREWALL: u8 = 0;
    /// Peer init: sent first on a connection we opened directly.
    pub const PEER_INIT: u8 = 1;
    pub const SHARED_FILE_LIST_REQUEST: u32 = 4;
    pub const SHARED_FILE_LIST_RESPONSE: u32 = 5;
    pub const SEARCH_RESPONSE: u32 = 9;
    pub const USER_INFO_REQUEST: u32 = 15;
    pub const USER_INFO_RESPONSE: u32 = 16;
    pub const FOLDER_CONTENTS_REQUEST: u32 = 36;
    pub const FOLDER_CONTENTS_RESPONSE: u32 = 37;
    pub const TRANSFER_REQUEST: u32 = 40;
    pub const TRANSFER_RESPONSE: u32 = 41;
    pub const QUEUE_UPLOAD: u32 = 43;
    pub const PLACE_IN_QUEUE_RESPONSE: u32 = 44;
    pub const UPLOAD_FAILED: u32 = 46;
    pub const UPLOAD_DENIED: u32 = 50;
    pub const PLACE_IN_QUEUE_REQUEST: u32 = 51;
}

/// Direction field of [`PeerMessage::TransferRequest`], from the sender's side.
pub mod direction {
    /// The sender wants to download from us (legacy clients).
    pub const DOWNLOAD: u32 = 0;
    /// The sender is ready to upload to us.
    pub const UPLOAD: u32 = 1;
}

/// Messages on a peer ("P") connection, other than search responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerMessage {
    /// Ask for everything the peer shares ("browse").
    SharedFileListRequest,
    /// Ask for the peer's profile: description, picture, slots.
    UserInfoRequest,
    /// Ask for one folder and its subfolders.
    FolderContentsRequest {
        token: u32,
        folder: String,
    },
    /// Ask the peer to put `filename` in its upload queue.
    QueueUpload {
        filename: String,
    },
    /// The peer is ready to send `filename` (direction [`direction::UPLOAD`]) or,
    /// from old clients, wants to download it from us.
    TransferRequest {
        direction: u32,
        token: u32,
        filename: String,
        size: Option<u64>,
    },
    /// Our answer to a transfer request. For uploads to us, only `allowed` and an
    /// optional rejection reason are sent.
    TransferResponse {
        token: u32,
        allowed: bool,
        reason: Option<String>,
    },
    PlaceInQueueRequest {
        filename: String,
    },
    PlaceInQueueResponse {
        filename: String,
        place: u32,
    },
    /// The file connection for an upload closed before completion.
    UploadFailed {
        filename: String,
    },
    /// The peer won't send this file, e.g. "File not shared." or "Queued".
    UploadDenied {
        filename: String,
        reason: String,
    },
}

impl PeerMessage {
    /// Decode a framed peer message body. Returns `None` for codes this client
    /// doesn't handle (and for search responses, which have their own decoder).
    pub fn decode(message_code: u32, body: &[u8]) -> Result<Option<Self>, DecodeError> {
        let mut r = Reader::new(body);
        Ok(Some(match message_code {
            code::SHARED_FILE_LIST_REQUEST => Self::SharedFileListRequest,
            code::USER_INFO_REQUEST => Self::UserInfoRequest,
            code::FOLDER_CONTENTS_REQUEST => Self::FolderContentsRequest { token: r.u32()?, folder: r.string()? },
            code::QUEUE_UPLOAD => Self::QueueUpload { filename: r.string()? },
            code::TRANSFER_REQUEST => {
                let direction = r.u32()?;
                let token = r.u32()?;
                let filename = r.string()?;
                let size = if direction == direction::UPLOAD && r.remaining() >= 8 { Some(r.u64()?) } else { None };
                Self::TransferRequest { direction, token, filename, size }
            }
            code::TRANSFER_RESPONSE => {
                let token = r.u32()?;
                let allowed = r.bool()?;
                let reason = if !allowed && r.remaining() >= 4 { Some(r.string()?) } else { None };
                Self::TransferResponse { token, allowed, reason }
            }
            code::PLACE_IN_QUEUE_REQUEST => Self::PlaceInQueueRequest { filename: r.string()? },
            code::PLACE_IN_QUEUE_RESPONSE => Self::PlaceInQueueResponse { filename: r.string()?, place: r.u32()? },
            code::UPLOAD_FAILED => Self::UploadFailed { filename: r.string()? },
            code::UPLOAD_DENIED => Self::UploadDenied { filename: r.string()?, reason: r.string()? },
            _ => return Ok(None),
        }))
    }

    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        let message_code = match self {
            Self::SharedFileListRequest => code::SHARED_FILE_LIST_REQUEST,
            Self::UserInfoRequest => code::USER_INFO_REQUEST,
            Self::FolderContentsRequest { token, folder } => {
                w.u32(*token).string(folder);
                code::FOLDER_CONTENTS_REQUEST
            }
            Self::QueueUpload { filename } => {
                w.string(filename);
                code::QUEUE_UPLOAD
            }
            Self::TransferRequest { direction, token, filename, size } => {
                w.u32(*direction).u32(*token).string(filename);
                if let Some(size) = size {
                    w.u64(*size);
                }
                code::TRANSFER_REQUEST
            }
            Self::TransferResponse { token, allowed, reason } => {
                w.u32(*token).bool(*allowed);
                if let (false, Some(reason)) = (allowed, reason) {
                    w.string(reason);
                }
                code::TRANSFER_RESPONSE
            }
            Self::PlaceInQueueRequest { filename } => {
                w.string(filename);
                code::PLACE_IN_QUEUE_REQUEST
            }
            Self::PlaceInQueueResponse { filename, place } => {
                w.string(filename).u32(*place);
                code::PLACE_IN_QUEUE_RESPONSE
            }
            Self::UploadFailed { filename } => {
                w.string(filename);
                code::UPLOAD_FAILED
            }
            Self::UploadDenied { filename, reason } => {
                w.string(filename).string(reason);
                code::UPLOAD_DENIED
            }
        };
        w.finish(message_code)
    }
}

/// Decompressed search responses bigger than this are dropped (zip-bomb guard).
pub const MAX_DECOMPRESSED_BYTES: u64 = 32 << 20;

/// Numeric attribute codes attached to shared files.
mod attr {
    pub const BITRATE: u32 = 0;
    pub const DURATION: u32 = 1;
    pub const VBR: u32 = 2;
    pub const SAMPLE_RATE: u32 = 4;
    pub const BIT_DEPTH: u32 = 5;
}

/// One file in a search response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFile {
    /// Full virtual path as the peer shares it, `\`-separated.
    pub path: String,
    pub size: u64,
    pub extension: String,
    pub bitrate_kbps: Option<u32>,
    pub duration_secs: Option<u32>,
    pub vbr: bool,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

impl SharedFile {
    /// File name without folders.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.path.rsplit(['\\', '/']).next().unwrap_or(&self.path)
    }

    /// Folder the file lives in — the unit albums are grouped by.
    #[must_use]
    pub fn folder(&self) -> &str {
        self.path.rfind(['\\', '/']).map_or("", |i| &self.path[..i])
    }

    /// Best-effort quality from the reported attributes. Many peers send
    /// incomplete attributes, so we fall back to the extension and estimate the
    /// bitrate from size and duration. `None` for non-audio files.
    #[must_use]
    pub fn quality(&self) -> Option<Quality> {
        let ext = if self.extension.is_empty() {
            self.file_name().rsplit_once('.').map_or("", |(_, e)| e)
        } else {
            &self.extension
        };
        let mut codec = Codec::from_extension(ext)?;
        let estimated = self.duration_secs.and_then(|d| Quality::estimate_bitrate_kbps(self.size, d));
        let mut bitrate = self.bitrate_kbps.or(estimated);

        // `.m4a` holds either AAC or ALAC. AAC tops out around 320 kbps; anything
        // well above that is lossless.
        if codec == Codec::Aac && bitrate.is_some_and(|b| b >= 500) {
            codec = Codec::Alac;
        }
        // MP3 can't exceed 320 kbps. A higher figure is a bad attribute from the
        // peer, so prefer the size-based estimate if it's plausible.
        if codec == Codec::Mp3 && bitrate.is_some_and(|b| b > 320) {
            bitrate = estimated.filter(|&b| b <= 330).map(|b| b.min(320));
        }

        Some(Quality {
            codec,
            bit_depth: self.bit_depth.and_then(|b| u8::try_from(b).ok()).filter(|_| codec.is_lossless()),
            sample_rate: self.sample_rate.filter(|_| codec.is_lossless()),
            bitrate_kbps: if codec.is_lossless() { None } else { bitrate },
            vbr: self.vbr,
        })
    }
}

/// A peer's answer to one of our searches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResponse {
    pub username: String,
    pub token: u32,
    pub files: Vec<SharedFile>,
    /// A free upload slot means a download would start immediately.
    pub free_slot: bool,
    /// Average upload speed in bytes/s, as measured by the server.
    pub avg_speed: u32,
    pub queue_length: u32,
    /// Files shared only with the peer's buddies. We can see them but usually
    /// can't download them, so ranking ignores these by default.
    pub private_files: Vec<SharedFile>,
}

impl SearchResponse {
    /// Decode a zlib-compressed message body.
    pub fn decode(compressed: &[u8]) -> Result<Self, DecodeError> {
        let mut raw = Vec::new();
        ZlibDecoder::new(compressed)
            .take(MAX_DECOMPRESSED_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|e| DecodeError::Zlib(e.to_string()))?;
        if raw.len() as u64 > MAX_DECOMPRESSED_BYTES {
            return Err(DecodeError::TooLong(raw.len()));
        }
        let mut r = Reader::new(&raw);

        let username = r.string()?;
        let token = r.u32()?;
        let files = read_files(&mut r)?;
        let free_slot = r.bool()?;
        let avg_speed = r.u32()?;
        let queue_length = r.u32()?;
        // Older clients stop here.
        let private_files = if r.remaining() >= 8 {
            let _unknown = r.u32()?;
            read_files(&mut r)?
        } else {
            Vec::new()
        };

        Ok(Self { username, token, files, free_slot, avg_speed, queue_length, private_files })
    }

    /// Encode as a framed, compressed peer message. Used when *we* answer searches
    /// for our shared library, and by tests.
    ///
    /// # Panics
    /// Never in practice: compressing into memory cannot fail.
    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        w.string(&self.username).u32(self.token);
        write_files(&mut w, &self.files);
        w.bool(self.free_slot).u32(self.avg_speed).u32(self.queue_length).u32(0);
        write_files(&mut w, &self.private_files);

        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&w.into_body()).expect("in-memory write");
        let compressed = enc.finish().expect("in-memory write");

        let mut framed = Writer::new();
        framed.raw(&compressed);
        framed.finish(code::SEARCH_RESPONSE)
    }
}

/// The first message on every peer connection. Its code is a single byte, unlike
/// every later message on the same connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerInit {
    /// "I'm connecting because the server told me to": answers a `ConnectToPeer`
    /// the other side sent, identified by its token.
    PierceFirewall { token: u32 },
    /// "I'm connecting to you directly." `kind` is `P`, `F` or `D`.
    PeerInit { username: String, kind: String, token: u32 },
}

impl PeerInit {
    /// Decode a frame payload (code byte included).
    pub fn decode(payload: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(payload);
        match r.u8()? {
            code::PIERCE_FIREWALL => Ok(Self::PierceFirewall { token: r.u32()? }),
            code::PEER_INIT => Ok(Self::PeerInit { username: r.string()?, kind: r.string()?, token: r.u32()? }),
            other => Err(DecodeError::UnknownInit(other)),
        }
    }

    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        match self {
            Self::PierceFirewall { token } => {
                w.u32(*token);
                w.finish_init(code::PIERCE_FIREWALL)
            }
            Self::PeerInit { username, kind, token } => {
                w.string(username).string(kind).u32(*token);
                w.finish_init(code::PEER_INIT)
            }
        }
    }
}

pub(crate) fn read_files(r: &mut Reader<'_>) -> Result<Vec<SharedFile>, DecodeError> {
    // Smallest possible entry: code(1) + empty path(4) + size(8) + ext(4) + attrs(4).
    let count = r.count(21)?;
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let _code = r.u8()?;
        let path = r.string()?;
        let size = r.u64()?;
        let extension = r.string()?;
        let mut file = SharedFile {
            path,
            size,
            extension,
            bitrate_kbps: None,
            duration_secs: None,
            vbr: false,
            sample_rate: None,
            bit_depth: None,
        };
        for _ in 0..r.count(8)? {
            let (key, value) = (r.u32()?, r.u32()?);
            match key {
                attr::BITRATE => file.bitrate_kbps = Some(value),
                attr::DURATION => file.duration_secs = Some(value),
                attr::VBR => file.vbr = value == 1,
                attr::SAMPLE_RATE => file.sample_rate = Some(value),
                attr::BIT_DEPTH => file.bit_depth = Some(value),
                _ => {}
            }
        }
        files.push(file);
    }
    Ok(files)
}

pub(crate) fn write_files(w: &mut Writer, files: &[SharedFile]) {
    w.u32(u32::try_from(files.len()).unwrap_or(u32::MAX));
    for f in files {
        let attrs: Vec<(u32, u32)> = [
            f.bitrate_kbps.map(|v| (attr::BITRATE, v)),
            f.duration_secs.map(|v| (attr::DURATION, v)),
            f.vbr.then_some((attr::VBR, 1)),
            f.sample_rate.map(|v| (attr::SAMPLE_RATE, v)),
            f.bit_depth.map(|v| (attr::BIT_DEPTH, v)),
        ]
        .into_iter()
        .flatten()
        .collect();
        w.u8(1).string(&f.path).u64(f.size).string(&f.extension);
        w.u32(u32::try_from(attrs.len()).unwrap_or(0));
        for (k, v) in attrs {
            w.u32(k).u32(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flac(path: &str) -> SharedFile {
        SharedFile {
            path: path.into(),
            size: 60_000_000,
            extension: "flac".into(),
            bitrate_kbps: None,
            duration_secs: Some(386),
            vbr: false,
            sample_rate: Some(96_000),
            bit_depth: Some(24),
        }
    }

    #[test]
    fn search_response_round_trip() {
        let response = SearchResponse {
            username: "moonlight".into(),
            token: 42,
            files: vec![flac(r"@@music\Radiohead\1997 - OK Computer\02 - Paranoid Android.flac")],
            free_slot: true,
            avg_speed: 1_250_000,
            queue_length: 0,
            private_files: vec![],
        };
        let frame = response.encode();
        let body = &frame[8..];
        assert_eq!(SearchResponse::decode(body).unwrap(), response);
    }

    #[test]
    fn folder_and_name() {
        let f = flac(r"@@music\Radiohead\OK Computer\02 - Paranoid Android.flac");
        assert_eq!(f.folder(), r"@@music\Radiohead\OK Computer");
        assert_eq!(f.file_name(), "02 - Paranoid Android.flac");
    }

    #[test]
    fn quality_from_attributes() {
        assert_eq!(flac("a.flac").quality().unwrap().to_string(), "FLAC 24/96");

        // No attributes at all: infer codec from the name, bitrate from size/duration.
        let bare = SharedFile {
            path: r"x\song.mp3".into(),
            size: 7_200_000,
            extension: String::new(),
            bitrate_kbps: None,
            duration_secs: Some(180),
            vbr: false,
            sample_rate: None,
            bit_depth: None,
        };
        assert_eq!(bare.quality().unwrap().to_string(), "MP3 320");

        assert!(SharedFile { extension: "jpg".into(), ..flac("cover.jpg") }.quality().is_none());
    }

    #[test]
    fn peer_init_round_trip() {
        for init in [
            PeerInit::PierceFirewall { token: 0xABCD },
            PeerInit::PeerInit { username: "moonlight".into(), kind: "P".into(), token: 0 },
        ] {
            let frame = init.encode();
            assert_eq!(u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize, frame.len() - 4);
            assert_eq!(PeerInit::decode(&frame[4..]).unwrap(), init);
        }
        assert!(matches!(PeerInit::decode(&[9]), Err(DecodeError::UnknownInit(9))));
    }

    #[test]
    fn corrects_implausible_peer_attributes() {
        let base = SharedFile {
            path: r"x\song.m4a".into(),
            size: 40_000_000,
            extension: "m4a".into(),
            bitrate_kbps: Some(1046),
            duration_secs: Some(300),
            vbr: false,
            sample_rate: Some(44_100),
            bit_depth: Some(16),
        };
        assert_eq!(base.quality().unwrap().to_string(), "ALAC 16/44.1");
        assert_eq!(SharedFile { bitrate_kbps: Some(256), ..base.clone() }.quality().unwrap().to_string(), "AAC 256");

        // 12 MB over 300 s is ~320 kbps: trust the size, not the claimed 1013.
        let mp3 = SharedFile {
            path: r"x\song.mp3".into(),
            extension: "mp3".into(),
            size: 12_000_000,
            bitrate_kbps: Some(1013),
            ..base.clone()
        };
        assert_eq!(mp3.quality().unwrap().to_string(), "MP3 320");
        let no_duration = SharedFile { duration_secs: None, ..mp3 };
        assert_eq!(no_duration.quality().unwrap().to_string(), "MP3");
    }

    #[test]
    fn peer_messages_round_trip() {
        let messages = [
            PeerMessage::SharedFileListRequest,
            PeerMessage::UserInfoRequest,
            PeerMessage::FolderContentsRequest { token: 3, folder: r"@@moon\Album".into() },
            PeerMessage::QueueUpload { filename: r"@@moon\Album\01.flac".into() },
            PeerMessage::TransferRequest {
                direction: direction::UPLOAD,
                token: 7,
                filename: "a.flac".into(),
                size: Some(30_000_000),
            },
            PeerMessage::TransferRequest {
                direction: direction::DOWNLOAD,
                token: 8,
                filename: "b.flac".into(),
                size: None,
            },
            PeerMessage::TransferResponse { token: 7, allowed: true, reason: None },
            PeerMessage::TransferResponse { token: 9, allowed: false, reason: Some("Cancelled".into()) },
            PeerMessage::PlaceInQueueRequest { filename: "a.flac".into() },
            PeerMessage::PlaceInQueueResponse { filename: "a.flac".into(), place: 12 },
            PeerMessage::UploadFailed { filename: "a.flac".into() },
            PeerMessage::UploadDenied { filename: "a.flac".into(), reason: "File not shared.".into() },
        ];
        for message in messages {
            let frame = message.encode();
            let (message_code, body) = crate::frame::split_code(&frame[4..]).unwrap();
            assert_eq!(PeerMessage::decode(message_code, body).unwrap(), Some(message));
        }
        assert_eq!(PeerMessage::decode(code::SEARCH_RESPONSE, &[]).unwrap(), None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(SearchResponse::decode(b"not zlib"), Err(DecodeError::Zlib(_))));
    }
}
