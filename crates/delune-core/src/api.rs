//! Types shared by the HTTP API and its clients (TUI, web via generated TypeScript).
//!
//! Anything here is part of the public API contract: renaming a field is a
//! breaking change for every client.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{EntityKind, Provider, Quality};

/// `GET /api/v1/health`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub name: String,
    pub version: String,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Degraded,
}

/// `GET /api/v1/soulseek`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulseekStatus {
    pub state: SoulseekState,
    /// The account delune logs in with, when configured.
    pub username: Option<String>,
    /// Human-readable detail for anything other than `online`.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SoulseekState {
    /// No Soulseek account configured.
    NotConfigured,
    Connecting,
    Online,
    Reconnecting,
    /// Won't reconnect without a restart (bad password, logged in elsewhere).
    Stopped,
}

/// Error body returned by every failing API route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// Stable machine-readable code, e.g. `soulseek-not-configured`.
    pub code: String,
    /// What happened and what to do about it, written for the end user.
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

/// One file inside a [`Candidate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateFile {
    /// Full path as the peer shares it; what a download request names.
    pub path: String,
    pub name: String,
    pub size: u64,
    pub audio: bool,
    pub quality: Option<Quality>,
    pub quality_label: Option<String>,
    pub duration_secs: Option<u32>,
}

/// A folder shared by one peer that looks like a release: the unit users choose
/// between in search results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable within a search: `username` + folder path.
    pub id: String,
    pub username: String,
    /// Full folder path as the peer shares it.
    pub folder: String,
    /// Display title, usually the album folder name.
    pub title: String,
    /// The folder above, usually the artist.
    pub parent: Option<String>,
    pub files: Vec<CandidateFile>,
    pub audio_files: u32,
    pub total_bytes: u64,
    /// Sum of audio durations, when every audio file reports one.
    pub duration_secs: Option<u32>,
    /// The *lowest* quality among the audio files: what you're guaranteed to get.
    pub quality: Option<Quality>,
    pub quality_label: Option<String>,
    /// [`Quality::rank`] of `quality`; 0 when unknown.
    pub quality_rank: u32,
    /// Audio files differ in codec, depth or rate.
    pub mixed_quality: bool,
    pub has_cover: bool,
    pub free_slot: bool,
    /// Bytes per second.
    pub avg_speed: u32,
    pub queue_length: u32,
}

/// Broad quality band, compared before exact resolution: any complete lossless album
/// beats a hi-res fragment, but hi-res beats CD when both are complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityTier {
    Unknown,
    Lossy,
    Lossless,
    HiRes,
}

impl QualityTier {
    #[must_use]
    pub fn of(quality: Option<Quality>) -> Self {
        match quality {
            None => Self::Unknown,
            Some(q) if !q.codec.is_lossless() => Self::Lossy,
            Some(q) if q.bit_depth.unwrap_or(16) > 16 || q.sample_rate.unwrap_or(44_100) > 48_000 => Self::HiRes,
            Some(_) => Self::Lossless,
        }
    }
}

impl Candidate {
    /// Whether this folder has about as many tracks as a full release in these
    /// results. `typical_tracks` is the median audio file count of the search; a
    /// folder with a single track from a ten-track album is a fragment.
    #[must_use]
    pub fn looks_complete(&self, typical_tracks: u32) -> bool {
        let needed = (typical_tracks * 6 / 10).max(1);
        self.audio_files >= needed.min(typical_tracks)
    }

    /// Default ordering within one search:
    ///
    /// 1. Lossless over lossy.
    /// 2. Complete-looking folders over fragments.
    /// 3. Hi-res over CD quality, then exact resolution.
    /// 4. Consistent quality over mixed.
    /// 5. Whoever can start sending soonest and fastest.
    #[must_use]
    pub fn compare_in(a: &Self, b: &Self, typical_tracks: u32) -> Ordering {
        let lossless = |c: &Self| QualityTier::of(c.quality) >= QualityTier::Lossless;
        lossless(b)
            .cmp(&lossless(a))
            .then(b.looks_complete(typical_tracks).cmp(&a.looks_complete(typical_tracks)))
            .then(b.quality_rank.cmp(&a.quality_rank))
            .then(a.mixed_quality.cmp(&b.mixed_quality))
            .then(b.free_slot.cmp(&a.free_slot))
            .then(a.queue_length.cmp(&b.queue_length))
            .then(b.avg_speed.cmp(&a.avg_speed))
            .then(a.id.cmp(&b.id))
    }

    /// Ordering without search context: every folder counts as complete.
    #[must_use]
    pub fn compare(a: &Self, b: &Self) -> Ordering {
        Self::compare_in(a, b, 1)
    }

    /// Median number of audio files across `results`, for [`Candidate::compare_in`].
    #[must_use]
    pub fn typical_tracks(results: &[Self]) -> u32 {
        let mut counts: Vec<u32> = results.iter().map(|c| c.audio_files).collect();
        if counts.is_empty() {
            return 1;
        }
        counts.sort_unstable();
        counts[counts.len() / 2].max(1)
    }

    /// Sort `results` best-first using the search's own typical track count.
    pub fn rank(results: &mut [Self]) {
        let typical = Self::typical_tracks(results);
        results.sort_by(|a, b| Self::compare_in(a, b, typical));
    }
}

/// A file to download, as listed in a [`Candidate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedFile {
    pub path: String,
    pub size: u64,
}

/// `POST /api/v1/downloads`: fetch these files from one folder of one peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadJobRequest {
    pub username: String,
    pub folder: String,
    pub title: String,
    pub parent: Option<String>,
    pub files: Vec<RequestedFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobStatus {
    /// Waiting for an earlier file or for the peer.
    Queued,
    Downloading,
    /// Every file arrived; waiting in the review inbox.
    Ready,
    /// Some files couldn't be downloaded. The rest are kept.
    Failed,
    Cancelled,
    /// Approved and moved into the library.
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewState {
    /// Waiting for the download to finish.
    Waiting,
    /// Decoding and analysing the files.
    Checking,
    Ready,
    Failed,
}

/// One track as the review screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTrack {
    pub file: String,
    /// Where it will go, relative to the library.
    pub destination: String,
    pub title: String,
    pub artist: String,
    pub track: u32,
    pub disc: u32,
    pub quality: Option<Quality>,
    pub quality_label: Option<String>,
    pub duration_secs: Option<u32>,
    /// Highest frequency with real content, for lossless files.
    pub cutoff_hz: Option<u32>,
    pub suspect_transcode: bool,
    /// What's wrong with this file, if anything, in plain language.
    pub problem: Option<String>,
}

/// `GET /api/v1/downloads/{id}/review`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReport {
    pub album_artist: String,
    pub album: String,
    pub year: Option<u16>,
    pub tracks: Vec<ReviewTrack>,
    /// Destination of the cover image, if one will be imported.
    pub cover: Option<String>,
    pub warnings: Vec<String>,
    /// Destinations that already exist in the library.
    pub conflicts: Vec<String>,
    /// The library folder delune imports into, when configured.
    pub library_dir: Option<String>,
    /// Why importing isn't possible right now, if it isn't.
    pub blocked_reason: Option<String>,
}

/// `POST /api/v1/downloads/{id}/import`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: u32,
    pub folder: String,
    pub scan_started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    Waiting,
    Connecting,
    /// In the peer's upload queue.
    Queued,
    Starting,
    Transferring,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub status: FileStatus,
    pub bytes: u64,
    pub place_in_queue: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: String,
    pub username: String,
    pub folder: String,
    pub title: String,
    pub parent: Option<String>,
    /// Unix time in seconds.
    pub created_at: u64,
    pub status: JobStatus,
    pub files: Vec<JobFile>,
    pub bytes: u64,
    pub total_bytes: u64,
    pub review: ReviewState,
    /// Who started it. Jobs saved before accounts existed have none.
    #[serde(default)]
    pub requested_by: Option<String>,
}

impl DownloadJob {
    /// Recompute `status` and byte totals from the files.
    pub fn refresh(&mut self) {
        self.bytes = self.files.iter().map(|f| f.bytes).sum();
        self.total_bytes = self.files.iter().map(|f| f.size).sum();
        if matches!(self.status, JobStatus::Cancelled | JobStatus::Imported) {
            return;
        }
        let all = |pred: fn(FileStatus) -> bool| self.files.iter().all(|f| pred(f.status));
        let any = |pred: fn(FileStatus) -> bool| self.files.iter().any(|f| pred(f.status));
        self.status = if all(|s| s == FileStatus::Done) {
            JobStatus::Ready
        } else if all(|s| matches!(s, FileStatus::Done | FileStatus::Failed)) {
            JobStatus::Failed
        } else if any(|s| matches!(s, FileStatus::Transferring | FileStatus::Starting | FileStatus::Done)) {
            JobStatus::Downloading
        } else {
            JobStatus::Queued
        };
    }
}

/// What someone may do in delune. Admins (Navidrome admins) can do everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools, reason = "independent switches, shown as toggles")]
pub struct Permissions {
    /// Search Soulseek and open releases.
    pub search: bool,
    /// Start downloads. Everything downloaded still waits for review.
    pub download: bool,
    /// Import their own downloads even when imports need an admin's approval.
    pub skip_approval: bool,
    /// See and act on everyone's downloads, and manage people and settings.
    pub manage: bool,
}

impl Permissions {
    pub const ALL: Self = Self { search: true, download: true, skip_approval: true, manage: true };
    /// What someone signing in for the first time gets.
    pub const MEMBER: Self = Self { search: true, download: true, skip_approval: false, manage: false };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// Accounts come from Navidrome; everyone signs in.
    Navidrome,
    /// No Navidrome configured: whoever can reach delune is the admin.
    Open,
}

/// `GET /api/v1/session`: who is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub username: String,
    pub admin: bool,
    /// Effective permissions (all of them for admins).
    pub permissions: Permissions,
    /// Whether this person can import their own downloads right now.
    pub can_import: bool,
    pub mode: AuthMode,
    /// Only returned to clients that asked for one, such as the TUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// `POST /api/v1/session`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Return a bearer token in the response instead of relying on the cookie.
    #[serde(default)]
    pub token: bool,
}

/// One person in `GET /api/v1/users`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub username: String,
    pub admin: bool,
    pub permissions: Permissions,
    /// Unix seconds.
    pub last_login: u64,
}

/// `GET /api/v1/users`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct People {
    /// Imports wait for an admin unless the person may skip approval.
    pub require_approval: bool,
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    Online,
    Away,
    Offline,
}

/// `GET /api/v1/soulseek/users/{username}`: another Soulseek user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulseekUser {
    pub username: String,
    /// False when no account has that name.
    pub exists: bool,
    pub presence: Presence,
    /// Upload speed in bytes per second, as the server measured it.
    pub avg_speed: u32,
    pub files: u32,
    pub folders: u32,
    pub country: Option<String>,
    /// From the user themselves; absent when they couldn't be reached.
    pub profile: Option<SoulseekProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulseekProfile {
    pub description: String,
    pub has_picture: bool,
    pub queue_size: u32,
    pub slots_free: bool,
    pub total_uploads: u32,
}

/// One folder in `GET /api/v1/soulseek/users/{username}/shares`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareFolder {
    /// Virtual path, `\`-separated.
    pub path: String,
    pub files: u32,
    pub audio_files: u32,
    pub bytes: u64,
    /// Quality of the folder's weakest audio file.
    pub quality_label: Option<String>,
    pub quality_rank: u32,
}

/// `GET /api/v1/soulseek/users/{username}/shares`: every folder a user shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareTree {
    pub username: String,
    pub folders: Vec<ShareFolder>,
    /// Folders only their buddies can download from.
    pub private_folders: u32,
}

/// One line of chat, private or in a room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: u64,
    /// Unix seconds.
    pub at: u64,
    pub from: String,
    pub text: String,
    /// Sent by this delune's Soulseek account.
    pub outgoing: bool,
}

/// A private conversation, as listed in `GET /api/v1/soulseek/chat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub username: String,
    pub last: Option<ChatMessage>,
    pub unread: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomSummary {
    pub name: String,
    pub members: u32,
    pub joined: bool,
    pub unread: u32,
}

/// `GET /api/v1/soulseek/chat`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatOverview {
    pub conversations: Vec<ConversationSummary>,
    /// Rooms we're in first, then the busiest public rooms.
    pub rooms: Vec<RoomSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomPerson {
    pub username: String,
    pub presence: Presence,
    pub files: u32,
    pub avg_speed: u32,
    pub country: Option<String>,
}

/// `GET /api/v1/soulseek/chat/rooms/{room}`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomView {
    pub name: String,
    pub joined: bool,
    pub members: Vec<RoomPerson>,
    pub messages: Vec<ChatMessage>,
}

/// Streamed from `GET /api/v1/soulseek/chat/events` so pages can refresh what changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ChatUpdate {
    Conversation { username: String, message: ChatMessage },
    Room { room: String, message: Option<ChatMessage> },
    Rooms,
}

/// How delune shares the library on Soulseek. Off until someone turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingSettings {
    pub enabled: bool,
    /// The top folder other people see, instead of where the library really is.
    pub share_name: String,
    /// Uploads at once.
    pub slots: u32,
    /// Files one person may have waiting.
    pub queue_per_user: u32,
    /// Upload speed cap in KiB/s; none means no cap.
    pub speed_limit_kib: Option<u32>,
    /// Download speed cap in KiB/s, shared by every download; none means no cap.
    #[serde(default)]
    pub download_limit_kib: Option<u32>,
    /// Refuse uploads to people who share nothing themselves.
    #[serde(default)]
    pub refuse_leechers: bool,
    /// People who can't download from us.
    pub banned: Vec<String>,
}

impl Default for SharingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            share_name: "Music".into(),
            slots: 3,
            queue_per_user: 200,
            speed_limit_kib: None,
            download_limit_kib: None,
            refuse_leechers: false,
            banned: Vec::new(),
        }
    }
}

/// `GET /api/v1/soulseek/stats`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoulseekStats {
    pub shared_files: u32,
    pub shared_folders: u32,
    pub uploads_running: u32,
    pub uploads_waiting: u32,
    pub downloads_running: u32,
    /// Bytes, ever (since delune started keeping count).
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub uploads_completed: u32,
}

/// `GET /api/v1/sharing`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharingStatus {
    pub settings: SharingSettings,
    /// The folder being shared, when a library is configured.
    pub library_dir: Option<String>,
    pub scanning: bool,
    pub files: u32,
    pub folders: u32,
    /// Unix seconds.
    pub last_scan: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UploadStatus {
    Queued,
    Connecting,
    Transferring,
    Completed,
    Failed,
    Cancelled,
}

/// One upload in `GET /api/v1/soulseek/uploads`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub id: u64,
    pub username: String,
    /// The shared path they asked for.
    pub filename: String,
    pub size: u64,
    pub bytes: u64,
    pub status: UploadStatus,
    pub reason: Option<String>,
    /// Unix seconds.
    pub queued_at: u64,
    /// Bytes per second.
    pub speed: u64,
}

/// The least a wishlist match must be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MinQuality {
    Any,
    #[default]
    Lossless,
    HiRes,
}

impl MinQuality {
    #[must_use]
    pub fn accepts(self, tier: QualityTier) -> bool {
        match self {
            Self::Any => true,
            Self::Lossless => matches!(tier, QualityTier::Lossless | QualityTier::HiRes),
            Self::HiRes => tier == QualityTier::HiRes,
        }
    }
}

/// Something to keep looking for. `GET /api/v1/wishlist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WishlistItem {
    pub id: String,
    pub query: String,
    pub added_by: String,
    /// Unix seconds.
    pub added_at: u64,
    /// Start a download (for review) as soon as a good enough copy turns up.
    pub auto_download: bool,
    pub min_quality: MinQuality,
    pub paused: bool,
    pub last_searched: Option<u64>,
    /// Good enough copies found by the last search.
    pub last_matches: u32,
    /// The best of them.
    pub best: Option<Candidate>,
    /// The download started for this item, once one has been.
    pub download_id: Option<String>,
}

/// `POST /api/v1/wishlist`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WishlistRequest {
    pub query: String,
    #[serde(default = "yes")]
    pub auto_download: bool,
    #[serde(default)]
    pub min_quality: MinQuality,
}

const fn yes() -> bool {
    true
}

/// `PATCH /api/v1/wishlist/{id}`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WishlistUpdate {
    pub auto_download: Option<bool>,
    pub min_quality: Option<MinQuality>,
    pub paused: Option<bool>,
}

/// A link someone pasted, resolved to the release or track it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedLink {
    pub provider: Provider,
    pub kind: EntityKind,
    /// The album, track, artist or playlist name.
    pub title: String,
    pub artist: Option<String>,
    /// For a track: the album it's from, when the service says.
    pub album: Option<String>,
    pub year: Option<u16>,
    /// The tracklist, for albums and playlists whose service lists one.
    pub tracks: Vec<ResolvedTrack>,
    /// What delune searches Soulseek for.
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTrack {
    pub title: String,
    pub artist: Option<String>,
    pub duration_secs: Option<u32>,
}

/// `GET /api/v1/library/album`: whether an album is already in Navidrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryMatch {
    pub state: LibraryState,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub year: Option<u16>,
    /// Tracks the library has, in disc and track order.
    pub tracks: Vec<LibraryTrack>,
    /// Quality of the library copy (the lowest across its tracks).
    pub quality_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LibraryState {
    /// No Navidrome configured, or it couldn't be reached.
    Unknown,
    NotInLibrary,
    InLibrary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryTrack {
    pub title: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

/// Events streamed by `GET /api/v1/search` (Server-Sent Events, JSON data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SearchEvent {
    /// A pasted link, understood. Sent before `started`, which then carries the
    /// text delune searches Soulseek for.
    Resolved {
        link: ResolvedLink,
    },
    Started {
        query: String,
        timeout_secs: u32,
    },
    /// New candidates from one peer. Clients merge and sort.
    Candidates {
        items: Vec<Candidate>,
    },
    Finished {
        peers: u32,
        candidates: u32,
    },
    Failed {
        error: ApiError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Codec;

    fn candidate(id: &str, quality: Option<Quality>) -> Candidate {
        Candidate {
            id: id.into(),
            username: "u".into(),
            folder: String::new(),
            title: String::new(),
            parent: None,
            files: vec![],
            audio_files: 1,
            total_bytes: 0,
            duration_secs: None,
            quality,
            quality_label: None,
            quality_rank: quality.map_or(0, |q| q.rank()),
            mixed_quality: false,
            has_cover: false,
            free_slot: true,
            avg_speed: 0,
            queue_length: 0,
        }
    }

    #[test]
    fn ordering_prefers_quality_then_availability() {
        let hires = candidate("a", Some(Quality::lossless(Codec::Flac, 24, 96_000)));
        let cd_busy = Candidate {
            free_slot: false,
            queue_length: 50,
            ..candidate("b", Some(Quality::lossless(Codec::Flac, 16, 44_100)))
        };
        let cd_free =
            Candidate { avg_speed: 5_000_000, ..candidate("c", Some(Quality::lossless(Codec::Flac, 16, 44_100))) };
        let cd_mixed = Candidate { mixed_quality: true, ..cd_free.clone() };
        let mp3 = candidate("d", Some(Quality::lossy(Codec::Mp3, 320)));
        let unknown = candidate("e", None);

        let mut list = [unknown, mp3, cd_busy, cd_mixed, hires, cd_free];
        list.sort_by(Candidate::compare);
        let ids: Vec<_> = list.iter().map(|c| (c.id.as_str(), c.mixed_quality)).collect();
        // A consistent-quality folder beats a mixed one even if it's busier (ADR 0004).
        assert_eq!(ids, [("a", false), ("c", false), ("b", false), ("c", true), ("d", false), ("e", false)]);
    }

    #[test]
    fn complete_albums_beat_hires_fragments() {
        let album = |id: &str, q: Quality, tracks: u32| Candidate { audio_files: tracks, ..candidate(id, Some(q)) };
        let mut results = [
            album("hires-single", Quality::lossless(Codec::Flac, 24, 192_000), 1),
            album("cd-album", Quality::lossless(Codec::Flac, 16, 44_100), 10),
            album("hires-album", Quality::lossless(Codec::Flac, 24, 96_000), 10),
            album("mp3-album", Quality::lossy(Codec::Mp3, 320), 10),
            album("cd-album-2", Quality::lossless(Codec::Flac, 16, 44_100), 11),
        ];
        assert_eq!(Candidate::typical_tracks(&results), 10);
        Candidate::rank(&mut results);
        let ids: Vec<_> = results.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["hires-album", "cd-album", "cd-album-2", "hires-single", "mp3-album"]);
    }

    #[test]
    fn tiers() {
        assert_eq!(QualityTier::of(Some(Quality::lossless(Codec::Flac, 16, 96_000))), QualityTier::HiRes);
        assert_eq!(QualityTier::of(Some(Quality::lossless(Codec::Alac, 16, 44_100))), QualityTier::Lossless);
        assert_eq!(QualityTier::of(Some(Quality::lossy(Codec::Opus, 256))), QualityTier::Lossy);
        assert_eq!(QualityTier::of(None), QualityTier::Unknown);
    }

    #[test]
    fn job_status_follows_files() {
        let file = |status, bytes| JobFile {
            path: String::new(),
            name: String::new(),
            size: 10,
            status,
            bytes,
            place_in_queue: None,
            error: None,
        };
        let mut job = DownloadJob {
            id: "j".into(),
            username: "u".into(),
            folder: String::new(),
            title: String::new(),
            parent: None,
            created_at: 0,
            status: JobStatus::Queued,
            files: vec![file(FileStatus::Queued, 0), file(FileStatus::Waiting, 0)],
            bytes: 0,
            total_bytes: 0,
            review: ReviewState::Waiting,
            requested_by: None,
        };
        job.refresh();
        assert_eq!((job.status, job.total_bytes), (JobStatus::Queued, 20));
        job.files[0] = file(FileStatus::Transferring, 4);
        job.refresh();
        assert_eq!((job.status, job.bytes), (JobStatus::Downloading, 4));
        job.files = vec![file(FileStatus::Done, 10), file(FileStatus::Failed, 0)];
        job.refresh();
        assert_eq!(job.status, JobStatus::Failed);
        job.files[1] = file(FileStatus::Done, 10);
        job.refresh();
        assert_eq!(job.status, JobStatus::Ready);
    }

    #[test]
    fn search_events_are_tagged() {
        let json = serde_json::to_string(&SearchEvent::Finished { peers: 3, candidates: 5 }).unwrap();
        assert_eq!(json, r#"{"type":"finished","peers":3,"candidates":5}"#);
    }
}
