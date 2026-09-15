//! Length-prefixed framing for TCP streams.
//!
//! Every Soulseek connection — server and peer alike — is a stream of frames:
//! a `uint32` little-endian length followed by that many bytes. [`FrameCodec`] turns
//! the byte stream into those payloads and nothing more; what the payload *means*
//! (a `uint32` code for server and peer messages, a `uint8` code for peer init
//! messages) is decided by the caller.
//!
//! The codec enforces a maximum frame size. Without it a single hostile peer could
//! claim a 4 GiB message and make us buffer it.

use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::wire::{DecodeError, Reader};

/// Largest frame accepted from the server.
pub const MAX_SERVER_FRAME: usize = 16 << 20;
/// Largest frame accepted from a peer. Search responses from huge shares can be a
/// few megabytes compressed.
pub const MAX_PEER_FRAME: usize = 16 << 20;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("frame of {0} bytes exceeds the limit")]
    TooLarge(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct FrameCodec {
    max_len: usize,
}

impl FrameCodec {
    #[must_use]
    pub const fn new(max_len: usize) -> Self {
        Self { max_len }
    }
}

impl Decoder for FrameCodec {
    type Item = Bytes;
    type Error = FrameError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Bytes>, FrameError> {
        let Some(header) = src.get(..4) else { return Ok(None) };
        let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if len > self.max_len {
            return Err(FrameError::TooLarge(len));
        }
        if src.len() < 4 + len {
            src.reserve(4 + len - src.len());
            return Ok(None);
        }
        src.advance(4);
        Ok(Some(src.split_to(len).freeze()))
    }
}

/// Outgoing messages are already framed by [`crate::wire::Writer::finish`].
impl Encoder<BytesMut> for FrameCodec {
    type Error = FrameError;

    fn encode(&mut self, item: BytesMut, dst: &mut BytesMut) -> Result<(), FrameError> {
        dst.extend_from_slice(&item);
        Ok(())
    }
}

/// Split a server or peer message payload into its `uint32` code and body.
pub fn split_code(payload: &[u8]) -> Result<(u32, &[u8]), DecodeError> {
    let mut r = Reader::new(payload);
    let code = r.u32()?;
    Ok((code, &payload[4..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Writer;

    #[test]
    fn decodes_split_and_joined_frames() {
        let mut codec = FrameCodec::new(1024);
        let mut a = Writer::new();
        a.u32(7);
        let mut b = Writer::new();
        b.string("hello");
        let mut stream = BytesMut::new();
        stream.extend_from_slice(&a.finish(1));
        stream.extend_from_slice(&b.finish(2));

        // Feed one byte short of the first frame: nothing yet.
        let mut partial = stream.split_to(11);
        assert!(codec.decode(&mut partial).unwrap().is_none());
        partial.unsplit(stream);

        let first = codec.decode(&mut partial).unwrap().unwrap();
        assert_eq!(split_code(&first).unwrap(), (1, &[7, 0, 0, 0][..]));
        let second = codec.decode(&mut partial).unwrap().unwrap();
        assert_eq!(split_code(&second).unwrap().0, 2);
        assert!(codec.decode(&mut partial).unwrap().is_none());
    }

    #[test]
    fn rejects_oversized_frames_before_buffering() {
        let mut codec = FrameCodec::new(100);
        let mut src = BytesMut::from(&[0xFF, 0xFF, 0xFF, 0x0F][..]);
        assert!(matches!(codec.decode(&mut src), Err(FrameError::TooLarge(_))));
    }
}
