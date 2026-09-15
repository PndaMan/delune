//! Peer messages — above all, search results.
//!
//! When we search, the server fans the query out and matching peers open a
//! connection *to us* and send a zlib-compressed [`SearchResponse`]. Everything the
//! ranking engine knows about a Soulseek candidate comes from this one message: the
//! file list, the audio attributes each peer chose to report, and the peer's upload
//! slot, speed and queue.

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
    pub const SEARCH_RESPONSE: u32 = 9;
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

fn read_files(r: &mut Reader<'_>) -> Result<Vec<SharedFile>, DecodeError> {
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

fn write_files(w: &mut Writer, files: &[SharedFile]) {
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
    fn rejects_garbage() {
        assert!(matches!(SearchResponse::decode(b"not zlib"), Err(DecodeError::Zlib(_))));
    }
}
