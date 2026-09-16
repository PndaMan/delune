//! Types shared by the HTTP API and its clients (TUI, web via generated TypeScript).
//!
//! Anything here is part of the public API contract: renaming a field is a
//! breaking change for every client.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{EntityKind, Provider, Quality};

/// `GET /api/v1/health`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Health {
    pub name: String,
    pub version: String,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Degraded,
}

/// `GET /api/v1/soulseek`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SoulseekStatus {
    pub state: SoulseekState,
    /// The account delune logs in with, when configured.
    pub username: Option<String>,
    /// Human-readable detail for anything other than `online`.
    pub message: Option<String>,
    /// The address the Soulseek server sees delune at.
    #[serde(default)]
    pub public_ip: Option<String>,
    /// The port other users connect to, when delune is listening.
    #[serde(default)]
    pub listen_port: Option<u16>,
    /// Someone on the internet has connected to that port, so it's open.
    #[serde(default)]
    pub reachable: bool,
    #[serde(default = "PortMapping::off")]
    pub port_mapping: PortMapping,
}

/// Forwarding the Soulseek port on the router with UPnP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PortMapping {
    pub state: PortMappingState,
    /// The router's public address, once mapped.
    pub external_ip: Option<String>,
    /// Why it failed.
    pub message: Option<String>,
}

impl PortMapping {
    #[must_use]
    pub const fn off() -> Self {
        Self { state: PortMappingState::Off, external_ip: None, message: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum PortMappingState {
    Off,
    Mapped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

/// How downloads from one Soulseek user have gone before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PeerHistory {
    pub files_done: u32,
    pub files_failed: u32,
    pub bytes: u64,
    /// Bytes per second, over everything downloaded from them.
    pub average_speed: u64,
    /// Unix seconds.
    pub last_seen: u64,
}

/// A folder shared by one peer that looks like a release: the unit users choose
/// between in search results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// How downloading from this user has gone before, if delune has.
    #[serde(default)]
    pub peer: Option<PeerHistory>,
}

/// Broad quality band, compared before exact resolution: any complete lossless album
/// beats a hi-res fragment, but hi-res beats CD when both are complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// 5. People downloads have gone well from before, over strangers, over people
    ///    whose downloads mostly failed.
    /// 6. Whoever can start sending soonest and fastest.
    #[must_use]
    pub fn compare_in(a: &Self, b: &Self, typical_tracks: u32) -> Ordering {
        let lossless = |c: &Self| QualityTier::of(c.quality) >= QualityTier::Lossless;
        lossless(b)
            .cmp(&lossless(a))
            .then(b.looks_complete(typical_tracks).cmp(&a.looks_complete(typical_tracks)))
            .then(b.quality_rank.cmp(&a.quality_rank))
            .then(a.mixed_quality.cmp(&b.mixed_quality))
            .then(b.track_record().cmp(&a.track_record()))
            .then(b.free_slot.cmp(&a.free_slot))
            .then(a.queue_length.cmp(&b.queue_length))
            .then(b.avg_speed.cmp(&a.avg_speed))
            .then(a.id.cmp(&b.id))
    }

    /// 2 for someone whose downloads have gone well, 0 for someone whose mostly
    /// failed, 1 otherwise (including strangers).
    fn track_record(&self) -> u8 {
        match &self.peer {
            Some(p) if p.files_failed > p.files_done => 0,
            Some(p) if p.files_done >= 3 && p.files_failed.saturating_mul(5) <= p.files_done => 2,
            _ => 1,
        }
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestedFile {
    pub path: String,
    pub size: u64,
}

/// `POST /api/v1/downloads`: fetch these files from one folder of one peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DownloadJobRequest {
    pub username: String,
    pub folder: String,
    pub title: String,
    pub parent: Option<String>,
    pub files: Vec<RequestedFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ImportResult {
    pub imported: u32,
    pub folder: String,
    pub scan_started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Once imported: the folder it went to (relative to the library) and when.
    #[serde(default)]
    pub imported_to: Option<String>,
    #[serde(default)]
    pub imported_at: Option<u64>,
    /// Jobs with a higher priority start first when only a few may download at once.
    #[serde(default)]
    pub priority: u64,
    /// While held back by the limit on downloads at once: its place in line, from 1.
    #[serde(default)]
    pub waiting_for_slot: Option<u32>,
    /// Why the job failed, when the files can't say (a fetch command that got nothing).
    #[serde(default)]
    pub error: Option<String>,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Ask for albums; someone who manages delune approves them. Matters for people
    /// who can't download themselves.
    #[serde(default = "yes")]
    pub request: bool,
}

impl Permissions {
    pub const ALL: Self = Self { search: true, download: true, skip_approval: true, manage: true, request: true };
    /// What someone signing in for the first time gets.
    pub const MEMBER: Self = Self { search: true, download: true, skip_approval: false, manage: false, request: true };
}

/// Where a request for an album has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RequestStatus {
    /// Waiting for someone who manages delune.
    Pending,
    Declined,
    /// Approved; on the wishlist until a good enough copy turns up.
    Searching,
    Downloading,
    /// Downloaded and waiting in review.
    Review,
    /// In the library.
    Available,
    /// The download failed; it can be retried from Downloads.
    Failed,
}

/// Someone asking for an album.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MusicRequest {
    pub id: String,
    pub requested_by: String,
    /// Unix seconds.
    pub requested_at: u64,
    pub title: String,
    pub artist: Option<String>,
    /// What to search Soulseek for.
    pub query: String,
    /// The link it was found from, if any.
    pub link: Option<String>,
    /// A particular copy the requester chose; otherwise the wishlist finds one.
    pub download: Option<DownloadJobRequest>,
    /// That copy's quality, for display.
    pub quality_label: Option<String>,
    pub note: Option<String>,
    pub status: RequestStatus,
    pub decided_by: Option<String>,
    pub decided_at: Option<u64>,
    /// Why it was declined.
    pub reason: Option<String>,
    pub download_id: Option<String>,
    pub wishlist_id: Option<String>,
}

/// `POST /api/v1/requests`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NewRequest {
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    pub query: String,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub download: Option<DownloadJobRequest>,
    #[serde(default)]
    pub quality_label: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// `POST /api/v1/requests/{id}/decision`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestDecision {
    pub approve: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum NotificationKind {
    RequestNew,
    RequestApproved,
    RequestDeclined,
    ReviewReady,
    DownloadFailed,
    Imported,
}

/// Something that happened that someone should know about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Notification {
    pub id: String,
    pub kind: NotificationKind,
    /// Unix seconds.
    pub at: u64,
    pub title: String,
    pub detail: Option<String>,
    /// Where in the web UI to go, such as `/review`.
    pub link: Option<String>,
    pub read: bool,
}

/// `GET /api/v1/notifications`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Notifications {
    pub unread: u32,
    pub items: Vec<Notification>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// Accounts come from Navidrome; everyone signs in.
    Navidrome,
    /// No Navidrome configured: whoever can reach delune is the admin.
    Open,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Night or blue hour, following the device.
    #[default]
    System,
    Night,
    BlueHour,
    /// Night on true black, for OLED screens.
    Midnight,
    /// A deep green night.
    Forest,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Accent {
    #[default]
    Moon,
    Aurora,
    Dusk,
    Ember,
    Tide,
    Fern,
}

/// How delune looks for one person, saved to their account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Appearance {
    pub theme: Theme,
    pub accent: Accent,
}

/// `GET /api/v1/session`: who is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Me {
    pub username: String,
    pub admin: bool,
    /// Effective permissions (all of them for admins).
    pub permissions: Permissions,
    /// Whether this person can import their own downloads right now.
    pub can_import: bool,
    pub mode: AuthMode,
    #[serde(default)]
    pub appearance: Appearance,
    /// Changes whenever their profile picture does; none without one.
    #[serde(default)]
    pub avatar: Option<u64>,
    /// Only returned to clients that asked for one, such as the TUI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// `POST /api/v1/session`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Return a bearer token in the response instead of relying on the cookie.
    #[serde(default)]
    pub token: bool,
}

/// One person in `GET /api/v1/users`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Person {
    pub username: String,
    pub admin: bool,
    pub permissions: Permissions,
    /// Unix seconds.
    pub last_login: u64,
    #[serde(default)]
    pub avatar: Option<u64>,
    /// Devices they're signed in on.
    #[serde(default)]
    pub sessions: u32,
}

/// One place someone is signed in, in `GET /api/v1/session/devices`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionInfo {
    /// Opaque; not the token.
    pub id: String,
    /// What signed in, such as "Firefox on Linux".
    pub device: String,
    /// Unix seconds.
    pub created_at: u64,
    pub last_seen: u64,
    /// The session making this request.
    pub current: bool,
}

/// `GET /api/v1/users`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct People {
    /// Imports wait for an admin unless the person may skip approval.
    pub require_approval: bool,
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    Online,
    Away,
    Offline,
}

/// `GET /api/v1/soulseek/users/{username}`: another Soulseek user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SoulseekProfile {
    pub description: String,
    pub has_picture: bool,
    pub queue_size: u32,
    pub slots_free: bool,
    pub total_uploads: u32,
}

/// One folder in `GET /api/v1/soulseek/users/{username}/shares`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ShareTree {
    pub username: String,
    pub folders: Vec<ShareFolder>,
    /// Folders only their buddies can download from.
    pub private_folders: u32,
    /// Set when this is delune's saved copy of a favourite's shares (Unix seconds it
    /// was fetched); a fresh list is on its way.
    pub saved_at: Option<u64>,
}

/// `GET /api/v1/bandcamp/release`: an album on Bandcamp, to buy there or fetch again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BandcampOffer {
    /// The album's page on Bandcamp, where it's bought.
    pub url: String,
    pub title: String,
    pub artist: String,
    /// The digital price; name-your-price albums give their minimum.
    pub price: Option<f64>,
    pub currency: Option<String>,
    pub name_your_price: bool,
    /// The signed-in person's linked account has bought it.
    pub owned: bool,
    /// Their purchase, when owned, for downloading it again.
    pub purchase: Option<String>,
}

/// `GET /api/v1/bandcamp/account`: someone's linked Bandcamp account. The login
/// itself is never sent back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BandcampAccount {
    pub linked: bool,
    pub username: Option<String>,
    pub name: Option<String>,
    pub purchases: u32,
    /// When purchases were last fetched (Unix seconds).
    pub synced_at: Option<u64>,
    pub syncing: bool,
    /// Why the last sync failed, such as an expired login.
    pub problem: Option<String>,
}

/// `PUT /api/v1/bandcamp/account`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LinkBandcamp {
    /// The `identity` cookie, a whole `Cookie:` header, or a cookies.txt file.
    pub cookie: String,
}

/// `GET /api/v1/bandcamp/purchases`: something bought on Bandcamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BandcampPurchase {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub purchased_at: Option<u64>,
    pub art: Option<String>,
    pub url: Option<String>,
    /// Bandcamp offers the files again.
    pub downloadable: bool,
    /// The download job fetching it, once one was started.
    pub job: Option<String>,
}

/// `POST /api/v1/bandcamp/purchases/{id}/download`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BandcampDownload {
    /// `flac` (the default), `mp3-320`, `mp3-v0`, `alac`, `aac-hi`, `vorbis`, `wav` or `aiff-lossless`.
    pub format: Option<String>,
}

/// `GET /api/v1/soundcloud/artist`: an artist's SoundCloud, with what they put out lately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SoundcloudArtist {
    pub id: u64,
    pub name: String,
    pub permalink: String,
    pub url: String,
    pub avatar: Option<String>,
    pub followers: u64,
    pub tracks: u64,
    pub verified: bool,
    /// New tracks from them go onto the wishlist.
    pub following: bool,
    pub recent: Vec<SoundcloudTrack>,
}

/// A track from an artist's SoundCloud feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SoundcloudTrack {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub published_at: Option<u64>,
    pub duration_secs: Option<u32>,
    pub artwork: Option<String>,
}

/// `GET /api/v1/soundcloud/track`: one track, and whether its artist gives it away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SoundcloudTrackDetail {
    pub title: String,
    pub artist: String,
    pub url: String,
    pub artwork: Option<String>,
    pub duration_secs: Option<u32>,
    pub album: Option<String>,
    /// Where the artist offers it for free: their own link, or the track page when
    /// SoundCloud's download button is on. None when it isn't given away.
    pub free_download: Option<String>,
}

/// `GET /api/v1/soundcloud/follows`: a SoundCloud artist whose new tracks are wished for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SoundcloudFollow {
    pub id: u64,
    pub name: String,
    pub permalink: String,
    pub avatar: Option<String>,
    pub added_by: String,
    pub since: u64,
    pub last_checked: Option<u64>,
    /// Tracks already seen, so only newer ones are wished for.
    #[serde(default)]
    pub seen: Vec<u64>,
}

/// `GET /api/v1/soulseek/favourites`: a Soulseek user someone starred, so their
/// shares are kept to hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FavouriteUser {
    pub username: String,
    /// When they were starred (Unix seconds).
    pub since: u64,
    /// When their share list was last saved, if it has been.
    pub saved_at: Option<u64>,
    /// Folders and files in that saved list.
    pub folders: u32,
    pub files: u32,
}

/// One line of chat, private or in a room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationSummary {
    pub username: String,
    pub last: Option<ChatMessage>,
    pub unread: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoomSummary {
    pub name: String,
    pub members: u32,
    pub joined: bool,
    pub unread: u32,
}

/// `GET /api/v1/soulseek/chat`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChatOverview {
    pub conversations: Vec<ConversationSummary>,
    /// Rooms we're in first, then the busiest public rooms.
    pub rooms: Vec<RoomSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoomPerson {
    pub username: String,
    pub presence: Presence,
    pub files: u32,
    pub avg_speed: u32,
    pub country: Option<String>,
}

/// `GET /api/v1/soulseek/chat/rooms/{room}`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoomView {
    pub name: String,
    pub joined: bool,
    pub members: Vec<RoomPerson>,
    pub messages: Vec<ChatMessage>,
}

/// Streamed from `GET /api/v1/soulseek/chat/events` so pages can refresh what changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ChatUpdate {
    Conversation { username: String, message: ChatMessage },
    Room { room: String, message: Option<ChatMessage> },
    Rooms,
}

/// How delune shares the library on Soulseek. Off until someone turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Albums downloading at once; the rest wait their turn. None means no limit.
    #[serde(default)]
    pub downloads_at_once: Option<u32>,
    /// Ask the router to forward the Soulseek port (UPnP).
    #[serde(default)]
    pub upnp: bool,
    /// Other clients delune may pass searches on to in the distributed network;
    /// 0 keeps it a leaf.
    #[serde(default)]
    pub distributed_children: u32,
    /// People who can't download from us.
    pub banned: Vec<String>,
    /// Different speed limits for part of each day.
    #[serde(default)]
    pub schedule: Option<SpeedSchedule>,
}

/// Speed limits that replace the usual ones between two times of day, for
/// example keeping transfers slow while people are home in the evening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SpeedSchedule {
    /// Minutes after midnight the window opens, in `time_zone`.
    pub start_minute: u16,
    /// Minutes after midnight it closes; before `start_minute` means it runs past midnight.
    pub end_minute: u16,
    /// Upload cap in KiB/s during the window; none means no cap.
    pub upload_limit_kib: Option<u32>,
    /// Download cap in KiB/s during the window; none means no cap.
    pub download_limit_kib: Option<u32>,
    /// IANA time zone the times are in, such as `Europe/London`.
    pub time_zone: String,
}

impl SpeedSchedule {
    /// Whether `minute` (after local midnight) falls inside the window.
    #[must_use]
    pub fn covers(&self, minute: u16) -> bool {
        if self.start_minute <= self.end_minute {
            (self.start_minute..self.end_minute).contains(&minute)
        } else {
            minute >= self.start_minute || minute < self.end_minute
        }
    }
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
            downloads_at_once: None,
            upnp: false,
            distributed_children: 0,
            banned: Vec::new(),
            schedule: None,
        }
    }
}

/// `GET /api/v1/soulseek/stats`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Clients delune passes distributed searches on to right now.
    #[serde(default)]
    pub distributed_children: u32,
}

/// `GET /api/v1/sharing`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Whether the speed schedule's limits are in force right now.
    #[serde(default)]
    pub scheduled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WishlistItem {
    pub id: String,
    pub query: String,
    /// For a single song: its title, matched against file names. Its download is
    /// just that file.
    #[serde(default)]
    pub track: Option<String>,
    /// The playlist it came from, if any.
    #[serde(default)]
    pub playlist: Option<String>,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WishlistRequest {
    pub query: String,
    #[serde(default)]
    pub track: Option<String>,
    #[serde(default)]
    pub playlist: Option<String>,
    #[serde(default = "yes")]
    pub auto_download: bool,
    #[serde(default)]
    pub min_quality: MinQuality,
}

const fn yes() -> bool {
    true
}

/// `GET/PUT /api/v1/external`: a program delune may run to fetch a link, for
/// sources it has no lawful downloader of its own for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ExternalSource {
    pub enabled: bool,
    /// The program to run, such as `yt-dlp`.
    pub program: String,
    /// Its arguments. `{url}` and `{output}` are replaced; nothing else is.
    pub arguments: Vec<String>,
}

/// `GET /api/v1/automation`: what delune does on its own.
///
/// Following an artist is its own switch — new releases from anyone followed always
/// reach the wishlist — so what's left here is off by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AutomationSettings {
    /// Look for better copies of lossy albums in the library.
    pub quality_upgrades: bool,
    /// What an upgrade must be.
    pub upgrade_to: MinQuality,
    /// Download what automation finds (for review), or just list it.
    pub auto_download: bool,
}

impl Default for AutomationSettings {
    fn default() -> Self {
        Self { quality_upgrades: false, upgrade_to: MinQuality::Lossless, auto_download: true }
    }
}

/// An artist someone follows. `GET /api/v1/follows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Follow {
    pub artist: String,
    pub deezer_id: u64,
    pub picture: Option<String>,
    pub added_by: String,
    /// Unix seconds. Releases from before this aren't fetched.
    pub since: u64,
    pub last_checked: Option<u64>,
    /// Releases already put on the wishlist.
    #[serde(default)]
    pub seen: Vec<u64>,
}

/// `PATCH /api/v1/wishlist/{id}`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WishlistUpdate {
    pub auto_download: Option<bool>,
    pub min_quality: Option<MinQuality>,
    pub paused: Option<bool>,
}

/// A link someone pasted, resolved to the release or track it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// The album's barcode, when the service gives it.
    #[serde(default)]
    pub upc: Option<String>,
    /// The track's ISRC, when the service gives it.
    #[serde(default)]
    pub isrc: Option<String>,
    /// The same release on MusicBrainz, when it could be matched.
    #[serde(default)]
    pub musicbrainz: Option<MusicBrainzMatch>,
}

/// A release found on MusicBrainz for a link from another service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MusicBrainzMatch {
    /// The exact release, when matched by barcode or ISRC.
    pub release_id: Option<String>,
    pub release_group_id: String,
    pub title: String,
    pub artist: Option<String>,
    /// When the release first came out, whichever edition the link is.
    pub original_year: Option<u16>,
    pub matched_by: MatchedBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MatchedBy {
    Barcode,
    Isrc,
    /// Title and artist, when both matched exactly.
    Name,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ResolvedTrack {
    pub title: String,
    pub artist: Option<String>,
    /// The album it's on, for playlists whose service says.
    #[serde(default)]
    pub album: Option<String>,
    pub duration_secs: Option<u32>,
}

/// `GET /api/v1/music/artist`: an artist, their releases and what the library has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ArtistInfo {
    pub name: String,
    pub picture: Option<String>,
    /// How many people follow them on Deezer, as a rough sense of scale.
    pub listeners: Option<u64>,
    /// Their releases, newest first.
    pub albums: Vec<ArtistAlbum>,
    /// What Navidrome already has by them.
    pub in_library: Vec<LibraryAlbum>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ArtistAlbum {
    pub title: String,
    pub year: Option<u16>,
    pub cover: Option<String>,
    /// `album`, `ep` or `single`.
    pub kind: String,
    pub in_library: bool,
}

/// An album Navidrome has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LibraryAlbum {
    pub id: String,
    pub title: String,
    pub year: Option<u16>,
    pub track_count: u32,
}

/// `GET /api/v1/music/search`: artists and albums matching what someone is typing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MusicSearch {
    pub artists: Vec<ArtistHit>,
    pub albums: Vec<AlbumHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ArtistHit {
    pub name: String,
    pub picture: Option<String>,
    /// Followers on Deezer, as a rough sense of scale.
    pub listeners: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AlbumHit {
    pub title: String,
    pub artist: String,
    pub year: Option<u16>,
    pub cover: Option<String>,
    pub track_count: Option<u32>,
}

/// `GET /api/v1/music/album`: one album, whether you have it or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AlbumInfo {
    pub title: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub cover: Option<String>,
    pub tracks: Vec<AlbumTrack>,
    pub in_library: LibraryMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AlbumTrack {
    pub position: u32,
    pub title: String,
    pub artist: Option<String>,
    pub duration_secs: Option<u32>,
}

/// `GET /api/v1/library/album`: whether an album is already in Navidrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LibraryState {
    /// No Navidrome configured, or it couldn't be reached.
    Unknown,
    NotInLibrary,
    InLibrary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LibraryTrack {
    pub title: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

/// Events streamed by `GET /api/v1/search` (Server-Sent Events, JSON data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SearchEvent {
    /// A pasted link, understood. Sent before `started`, which then carries the
    /// text delune searches Soulseek for.
    Resolved {
        link: Box<ResolvedLink>,
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

    #[test]
    fn speed_schedules_can_run_past_midnight() {
        let mut schedule = SpeedSchedule {
            start_minute: 18 * 60,
            end_minute: 23 * 60,
            upload_limit_kib: Some(100),
            download_limit_kib: None,
            time_zone: "UTC".into(),
        };
        assert!(schedule.covers(18 * 60));
        assert!(!schedule.covers(23 * 60));
        assert!(!schedule.covers(60));
        schedule.end_minute = 7 * 60;
        assert!(schedule.covers(23 * 60 + 59));
        assert!(schedule.covers(0));
        assert!(!schedule.covers(7 * 60));
        assert!(!schedule.covers(12 * 60));
    }

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
            peer: None,
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
        let history = |done, failed| {
            Some(PeerHistory { files_done: done, files_failed: failed, bytes: 0, average_speed: 0, last_seen: 0 })
        };
        let cd_trusted = Candidate { peer: history(20, 1), ..cd_free.clone() };
        let cd_flaky = Candidate { id: "flaky".into(), peer: history(2, 9), ..cd_free.clone() };
        let mut trust = vec![cd_flaky.clone(), cd_free.clone(), cd_trusted.clone()];
        Candidate::rank(&mut trust);
        assert_eq!(trust, [cd_trusted, cd_free.clone(), cd_flaky], "track record breaks ties");
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
            imported_to: None,
            imported_at: None,
            priority: 0,
            waiting_for_slot: None,
            error: None,
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
