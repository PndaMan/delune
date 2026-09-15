//! Soulseek wire primitives.
//!
//! The protocol is little-endian and length-prefixed throughout:
//!
//! | Type     | Encoding                                     |
//! |----------|----------------------------------------------|
//! | `uint8`  | 1 byte                                       |
//! | `uint32` | 4 bytes LE                                   |
//! | `uint64` | 8 bytes LE                                   |
//! | `bool`   | 1 byte, `0` or `1`                           |
//! | `string` | `uint32` byte length, then bytes             |
//! | `ip`     | `uint32` LE whose *value* is the address     |
//!
//! Strings are nominally UTF-8, but old clients send Latin-1/Windows-1252, so
//! [`Reader::string`] falls back to Latin-1 instead of failing the whole message.
//!
//! Reference: the Nicotine+ protocol documentation (`doc/SLSKPROTOCOL.md`).

use bytes::{Buf, BufMut, BytesMut};
use std::net::Ipv4Addr;

/// Upper bound on any single length field we'll trust, to stop a malicious peer
/// from making us allocate gigabytes.
pub const MAX_STRING_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("message ended early: needed {needed} more bytes")]
    Truncated { needed: usize },
    #[error("length field {0} exceeds the sanity limit")]
    TooLong(usize),
    #[error("invalid boolean byte {0}")]
    InvalidBool(u8),
    #[error("decompression failed: {0}")]
    Zlib(String),
    #[error("unknown peer init code {0}")]
    UnknownInit(u8),
}

/// Reads protocol primitives from a byte slice.
#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buf.len()
    }

    fn need(&self, n: usize) -> Result<(), DecodeError> {
        if self.buf.len() < n { Err(DecodeError::Truncated { needed: n - self.buf.len() }) } else { Ok(()) }
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        self.need(1)?;
        Ok(self.buf.get_u8())
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        self.need(2)?;
        Ok(self.buf.get_u16_le())
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        self.need(4)?;
        Ok(self.buf.get_u32_le())
    }

    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        self.need(8)?;
        Ok(self.buf.get_u64_le())
    }

    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(DecodeError::InvalidBool(other)),
        }
    }

    pub fn ip(&mut self) -> Result<Ipv4Addr, DecodeError> {
        self.u32().map(Ipv4Addr::from)
    }

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        self.need(len)?;
        let (head, tail) = self.buf.split_at(len);
        self.buf = tail;
        Ok(head)
    }

    pub fn string(&mut self) -> Result<String, DecodeError> {
        let len = self.u32()? as usize;
        if len > MAX_STRING_BYTES {
            return Err(DecodeError::TooLong(len));
        }
        let raw = self.bytes(len)?;
        Ok(String::from_utf8(raw.to_vec()).unwrap_or_else(|_| raw.iter().map(|&b| char::from(b)).collect()))
    }

    /// Read a `uint32` count that precedes a list, bounded so we never
    /// pre-allocate more entries than there could possibly be bytes for.
    pub fn count(&mut self, min_entry_bytes: usize) -> Result<usize, DecodeError> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_entry_bytes.max(1)) > self.remaining() {
            return Err(DecodeError::Truncated { needed: n * min_entry_bytes - self.remaining() });
        }
        Ok(n)
    }
}

/// Builds a message body. Framing (length + code) is added by [`Writer::finish`].
#[derive(Debug, Default)]
pub struct Writer {
    buf: BytesMut,
}

impl Writer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.put_u8(v);
        self
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.put_u16_le(v);
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.put_u32_le(v);
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.put_u64_le(v);
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }

    pub fn ip(&mut self, ip: Ipv4Addr) -> &mut Self {
        self.u32(u32::from(ip))
    }

    /// # Panics
    /// If the string is longer than `u32::MAX` bytes.
    pub fn string(&mut self, s: &str) -> &mut Self {
        self.u32(u32::try_from(s.len()).expect("string longer than 4 GiB"));
        self.buf.put_slice(s.as_bytes());
        self
    }

    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.put_slice(bytes);
        self
    }

    #[must_use]
    pub fn into_body(self) -> BytesMut {
        self.buf
    }

    /// Frame as a server/peer message: `uint32 length | uint32 code | body`.
    ///
    /// # Panics
    /// If the body is longer than `u32::MAX - 4` bytes.
    #[must_use]
    pub fn finish(self, code: u32) -> BytesMut {
        let len = u32::try_from(self.buf.len() + 4).expect("message longer than 4 GiB");
        let mut out = BytesMut::with_capacity(self.buf.len() + 8);
        out.put_u32_le(len);
        out.put_u32_le(code);
        out.put_slice(&self.buf);
        out
    }

    /// Frame as a peer *init* message, whose code is a single byte.
    ///
    /// # Panics
    /// If the body is longer than `u32::MAX - 1` bytes.
    #[must_use]
    pub fn finish_init(self, code: u8) -> BytesMut {
        let len = u32::try_from(self.buf.len() + 1).expect("message longer than 4 GiB");
        let mut out = BytesMut::with_capacity(self.buf.len() + 5);
        out.put_u32_le(len);
        out.put_u8(code);
        out.put_slice(&self.buf);
        out
    }
}

/// Lowercase hex MD5, as used by the login handshake.
#[must_use]
pub fn md5_hex(input: &[u8]) -> String {
    use md5::{Digest, Md5};
    use std::fmt::Write as _;
    Md5::digest(input).iter().fold(String::with_capacity(32), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_round_trip() {
        let mut w = Writer::new();
        w.u8(7).u32(0xDEAD_BEEF).u64(u64::MAX - 1).bool(true).string("Sigur Rós").ip(Ipv4Addr::new(10, 0, 0, 27));
        let body = w.into_body();
        let mut r = Reader::new(&body);
        assert_eq!(r.u8().unwrap(), 7);
        assert_eq!(r.u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.u64().unwrap(), u64::MAX - 1);
        assert!(r.bool().unwrap());
        assert_eq!(r.string().unwrap(), "Sigur Rós");
        assert_eq!(r.ip().unwrap(), Ipv4Addr::new(10, 0, 0, 27));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn framing_layout() {
        let mut w = Writer::new();
        w.u32(1234);
        assert_eq!(&w.finish(26)[..], &[8, 0, 0, 0, 26, 0, 0, 0, 0xD2, 0x04, 0, 0]);
    }

    #[test]
    fn latin1_fallback() {
        let body = [3, 0, 0, 0, b'c', 0xE9, b'u'];
        assert_eq!(Reader::new(&body).string().unwrap(), "céu");
    }

    #[test]
    fn hostile_lengths_are_rejected() {
        let body = [0xFF, 0xFF, 0xFF, 0x7F];
        assert!(matches!(Reader::new(&body).string(), Err(DecodeError::TooLong(_))));
        assert!(matches!(Reader::new(&body).count(1), Err(DecodeError::Truncated { .. })));
        assert!(matches!(Reader::new(&[1, 0]).u32(), Err(DecodeError::Truncated { needed: 2 })));
    }

    #[test]
    fn md5_matches_known_vector() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }
}
