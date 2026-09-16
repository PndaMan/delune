//! What a peer shares, and who they are: the answers to browsing a user.
//!
//! - [`SharedFileList`] answers `SharedFileListRequest`: every shared folder.
//! - [`FolderContents`] answers `FolderContentsRequest`: one folder and below.
//! - [`UserInfo`] answers `UserInfoRequest`: description, picture and upload slots.
//!
//! File lists are zlib-compressed on the wire. In them a file's name is relative to
//! its folder; [`SharedDirectory`] joins the two so every [`SharedFile::path`] is a
//! full virtual path, the same as in search results, and can be downloaded as is.

use std::io::{Read, Write as _};

use bytes::BytesMut;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

use crate::peer::{SharedFile, code, read_files, write_files};
use crate::wire::{DecodeError, Reader, Writer};

/// Decompressed share lists bigger than this are refused. Large libraries run to a
/// few hundred thousand files, well under this.
pub const MAX_SHARE_LIST_BYTES: u64 = 96 << 20;
/// Profile pictures bigger than this are dropped.
pub const MAX_PICTURE_BYTES: usize = 4 << 20;

/// One shared folder and the files directly inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedDirectory {
    /// Virtual path, `\\`-separated.
    pub path: String,
    /// Files with full paths.
    pub files: Vec<SharedFile>,
}

/// Everything a peer shares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedFileList {
    pub directories: Vec<SharedDirectory>,
    /// Folders shared only with the peer's buddies.
    pub private_directories: Vec<SharedDirectory>,
}

/// One folder of a peer's shares, with its subfolders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderContents {
    pub token: u32,
    pub folder: String,
    pub directories: Vec<SharedDirectory>,
}

/// A peer's profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserInfo {
    pub description: String,
    pub picture: Option<Vec<u8>>,
    pub total_uploads: u32,
    pub queue_size: u32,
    /// A download from this peer would start straight away.
    pub slots_free: bool,
    /// Who may upload to this peer (not sent by every client).
    pub upload_permitted: Option<u32>,
}

fn inflate(compressed: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut raw = Vec::new();
    ZlibDecoder::new(compressed)
        .take(MAX_SHARE_LIST_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| DecodeError::Zlib(e.to_string()))?;
    if raw.len() as u64 > MAX_SHARE_LIST_BYTES {
        return Err(DecodeError::TooLong(raw.len()));
    }
    Ok(raw)
}

fn deflate_framed(body: &[u8], message_code: u32) -> BytesMut {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing into a Vec can't fail.
    let _ = enc.write_all(body);
    let compressed = enc.finish().unwrap_or_default();
    let mut framed = Writer::new();
    framed.raw(&compressed);
    framed.finish(message_code)
}

fn read_directories(r: &mut Reader<'_>) -> Result<Vec<SharedDirectory>, DecodeError> {
    // Smallest entry: empty name (4) + file count (4).
    let count = r.count(8)?;
    let mut directories = Vec::with_capacity(count);
    for _ in 0..count {
        let path = r.string()?;
        let mut files = read_files(r)?;
        for file in &mut files {
            // Some clients already send full paths; only join bare names.
            if !file.path.contains('\\') {
                file.path = if path.is_empty() { file.path.clone() } else { format!("{path}\\{}", file.path) };
            }
        }
        directories.push(SharedDirectory { path, files });
    }
    Ok(directories)
}

fn write_directories(w: &mut Writer, directories: &[SharedDirectory]) {
    w.u32(u32::try_from(directories.len()).unwrap_or(u32::MAX));
    for directory in directories {
        w.string(&directory.path);
        // On the wire, names are relative to their folder.
        let relative: Vec<SharedFile> =
            directory.files.iter().map(|f| SharedFile { path: f.file_name().to_owned(), ..f.clone() }).collect();
        write_files(w, &relative);
    }
}

impl SharedFileList {
    /// Decode a compressed `SharedFileListResponse` body.
    pub fn decode(compressed: &[u8]) -> Result<Self, DecodeError> {
        let raw = inflate(compressed)?;
        let mut r = Reader::new(&raw);
        let directories = read_directories(&mut r)?;
        let private_directories = if r.remaining() >= 8 {
            let _unknown = r.u32()?;
            read_directories(&mut r)?
        } else {
            Vec::new()
        };
        Ok(Self { directories, private_directories })
    }

    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        write_directories(&mut w, &self.directories);
        w.u32(0);
        write_directories(&mut w, &self.private_directories);
        deflate_framed(&w.into_body(), code::SHARED_FILE_LIST_RESPONSE)
    }

    /// The list compressed as Soulseek sends it, without the message header: compact
    /// enough to keep, and read back with [`SharedFileList::decode`].
    #[must_use]
    pub fn compressed(&self) -> Vec<u8> {
        // `encode` frames the body with a length and a message code, four bytes each.
        self.encode().split_off(8).to_vec()
    }

    /// Number of files, public and private.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.directories.iter().chain(&self.private_directories).map(|d| d.files.len()).sum()
    }
}

impl FolderContents {
    /// Decode a compressed `FolderContentsResponse` body.
    pub fn decode(compressed: &[u8]) -> Result<Self, DecodeError> {
        let raw = inflate(compressed)?;
        let mut r = Reader::new(&raw);
        Ok(Self { token: r.u32()?, folder: r.string()?, directories: read_directories(&mut r)? })
    }

    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        w.u32(self.token).string(&self.folder);
        write_directories(&mut w, &self.directories);
        deflate_framed(&w.into_body(), code::FOLDER_CONTENTS_RESPONSE)
    }
}

impl UserInfo {
    /// Decode a `UserInfoResponse` body (not compressed).
    pub fn decode(body: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(body);
        let description = r.string()?;
        let picture = if r.bool()? {
            let len = r.u32()? as usize;
            let bytes = r.bytes(len)?;
            (len <= MAX_PICTURE_BYTES).then(|| bytes.to_vec())
        } else {
            None
        };
        let total_uploads = r.u32()?;
        let queue_size = r.u32()?;
        let slots_free = r.bool()?;
        let upload_permitted = if r.remaining() >= 4 { Some(r.u32()?) } else { None };
        Ok(Self { description, picture, total_uploads, queue_size, slots_free, upload_permitted })
    }

    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let mut w = Writer::new();
        w.string(&self.description);
        match &self.picture {
            Some(picture) => {
                w.bool(true).u32(u32::try_from(picture.len()).unwrap_or(0)).raw(picture);
            }
            None => {
                w.bool(false);
            }
        }
        w.u32(self.total_uploads).u32(self.queue_size).bool(self.slots_free);
        if let Some(permitted) = self.upload_permitted {
            w.u32(permitted);
        }
        w.finish(code::USER_INFO_RESPONSE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::split_code;

    fn file(path: &str) -> SharedFile {
        SharedFile {
            path: path.into(),
            size: 30_000_000,
            extension: "flac".into(),
            bitrate_kbps: None,
            duration_secs: Some(240),
            vbr: false,
            sample_rate: Some(44_100),
            bit_depth: Some(16),
        }
    }

    fn body(frame: &BytesMut, expected: u32) -> Vec<u8> {
        let (message_code, body) = split_code(&frame[4..]).unwrap();
        assert_eq!(message_code, expected);
        body.to_vec()
    }

    #[test]
    fn share_lists_round_trip_with_full_paths() {
        let list = SharedFileList {
            directories: vec![
                SharedDirectory {
                    path: r"@@music\Boards of Canada\Twoism".into(),
                    files: vec![file(r"@@music\Boards of Canada\Twoism\01 Sixtyniner.flac")],
                },
                SharedDirectory { path: r"@@music\Empty".into(), files: vec![] },
            ],
            private_directories: vec![SharedDirectory {
                path: r"@@music\Buddies".into(),
                files: vec![file(r"@@music\Buddies\secret.flac")],
            }],
        };
        let decoded = SharedFileList::decode(&body(&list.encode(), code::SHARED_FILE_LIST_RESPONSE)).unwrap();
        assert_eq!(decoded, list);
        assert_eq!(decoded.file_count(), 2);
        assert_eq!(decoded.directories[0].files[0].file_name(), "01 Sixtyniner.flac");
        assert_eq!(SharedFileList::decode(&list.compressed()).unwrap(), list, "kept copies read back");
    }

    #[test]
    fn folder_contents_round_trip() {
        let contents = FolderContents {
            token: 9,
            folder: r"@@music\Twoism".into(),
            directories: vec![SharedDirectory {
                path: r"@@music\Twoism".into(),
                files: vec![file(r"@@music\Twoism\02 Oirectine.flac")],
            }],
        };
        let decoded = FolderContents::decode(&body(&contents.encode(), code::FOLDER_CONTENTS_RESPONSE)).unwrap();
        assert_eq!(decoded, contents);
    }

    #[test]
    fn user_info_round_trip_with_and_without_picture() {
        for info in [
            UserInfo {
                description: "hello".into(),
                picture: Some(vec![1, 2, 3]),
                total_uploads: 5,
                queue_size: 2,
                slots_free: true,
                upload_permitted: Some(1),
            },
            UserInfo {
                description: String::new(),
                picture: None,
                total_uploads: 0,
                queue_size: 0,
                slots_free: false,
                upload_permitted: None,
            },
        ] {
            assert_eq!(UserInfo::decode(&body(&info.encode(), code::USER_INFO_RESPONSE)).unwrap(), info);
        }
    }

    #[test]
    fn refuses_garbage() {
        assert!(SharedFileList::decode(b"nope").is_err());
        assert!(UserInfo::decode(&[0, 0]).is_err());
    }
}
