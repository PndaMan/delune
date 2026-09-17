// Generated from delune-core's API types. Don't edit by hand; regenerate with
// cargo run -q -p delune-core --example typescript --features ts > web/src/lib/api.generated.ts

export type Accent = "moon" | "aurora" | "dusk" | "ember" | "tide" | "fern"

export type AlbumFollow = {
  /**
   * The album's Deezer id.
   */
  id: number
  artist: string
  title: string
  cover: string | null
  added_by: string
  /**
   * Unix seconds.
   */
  since: number
  last_checked: number | null
  /**
   * Tracks on the album when last checked.
   */
  tracks: number
  /**
   * Tracks already put on the wishlist (as comparison keys), so each is asked for once.
   */
  queued: Array<string>
}

export type AlertDevice = {
  id: string
  label: string
  /**
   * Unix seconds.
   */
  added_at: number
}

export type AlertSettings = {
  /**
   * An ntfy topic address, like `https://ntfy.sh/my-delune`.
   */
  ntfy: string | null
  /**
   * A Discord webhook address.
   */
  discord: string | null
  /**
   * Kinds of notification that stay in delune only.
   */
  muted: Array<NotificationKind>
  /**
   * Browsers and phones that get push notifications.
   */
  devices: Array<AlertDevice>
  /**
   * The key browsers subscribe to push with.
   */
  push_key: string | null
}

export type AlertSettingsUpdate = { ntfy: string | null; discord: string | null; muted: Array<NotificationKind> }

export type AlertTestResult = { channel: string; ok: boolean; message: string }

export type ApiError = {
  /**
   * Stable machine-readable code, e.g. `soulseek-not-configured`.
   */
  code: string
  /**
   * What happened and what to do about it, written for the end user.
   */
  message: string
}

export type Appearance = { theme: Theme; accent: Accent }

export type AuthMode = "navidrome" | "open"

export type AutomationSettings = {
  /**
   * Look for better copies of lossy albums in the library.
   */
  quality_upgrades: boolean
  /**
   * What an upgrade must be.
   */
  upgrade_to: MinQuality
  /**
   * Download what automation finds (for review), or just list it.
   */
  auto_download: boolean
}

export type BandcampAccount = {
  linked: boolean
  username: string | null
  name: string | null
  purchases: number
  /**
   * When purchases were last fetched (Unix seconds).
   */
  synced_at: number | null
  syncing: boolean
  /**
   * Why the last sync failed, such as an expired login.
   */
  problem: string | null
}

export type BandcampDownload = {
  /**
   * `flac` (the default), `mp3-320`, `mp3-v0`, `alac`, `aac-hi`, `vorbis`, `wav` or `aiff-lossless`.
   */
  format: string | null
}

export type BandcampOffer = {
  /**
   * The album's page on Bandcamp, where it's bought.
   */
  url: string
  title: string
  artist: string
  /**
   * The digital price; name-your-price albums give their minimum.
   */
  price: number | null
  currency: string | null
  name_your_price: boolean
  /**
   * The signed-in person's linked account has bought it.
   */
  owned: boolean
  /**
   * Their purchase, when owned, for downloading it again.
   */
  purchase: string | null
}

export type BandcampPurchase = {
  id: string
  title: string
  artist: string
  purchased_at: number | null
  art: string | null
  url: string | null
  /**
   * Bandcamp offers the files again.
   */
  downloadable: boolean
  /**
   * The download job fetching it, once one was started.
   */
  job: string | null
}

export type Candidate = {
  /**
   * Stable within a search: `username` + folder path.
   */
  id: string
  username: string
  /**
   * Full folder path as the peer shares it.
   */
  folder: string
  /**
   * Display title, usually the album folder name.
   */
  title: string
  /**
   * The folder above, usually the artist.
   */
  parent: string | null
  files: Array<CandidateFile>
  audio_files: number
  total_bytes: number
  /**
   * Sum of audio durations, when every audio file reports one.
   */
  duration_secs: number | null
  /**
   * The *lowest* quality among the audio files: what you're guaranteed to get.
   */
  quality: Quality | null
  quality_label: string | null
  /**
   * [`Quality::rank`] of `quality`; 0 when unknown.
   */
  quality_rank: number
  /**
   * Audio files differ in codec, depth or rate.
   */
  mixed_quality: boolean
  has_cover: boolean
  free_slot: boolean
  /**
   * Bytes per second.
   */
  avg_speed: number
  queue_length: number
  /**
   * How downloading from this user has gone before, if delune has.
   */
  peer: PeerHistory | null
}

export type CandidateFile = {
  /**
   * Full path as the peer shares it; what a download request names.
   */
  path: string
  name: string
  size: number
  audio: boolean
  quality: Quality | null
  quality_label: string | null
  duration_secs: number | null
}

export type ChatMessage = {
  id: number
  /**
   * Unix seconds.
   */
  at: number
  from: string
  text: string
  /**
   * Sent by this delune's Soulseek account.
   */
  outgoing: boolean
}

export type ChatOverview = {
  conversations: Array<ConversationSummary>
  /**
   * Rooms we're in first, then the busiest public rooms.
   */
  rooms: Array<RoomSummary>
}

export type ChatUpdate =
  | { type: "conversation"; username: string; message: ChatMessage }
  | { type: "room"; room: string; message: ChatMessage | null }
  | { type: "rooms" }

export type CheckState = "problem" | "warning" | "ok" | "info"

export type Codec = "flac" | "alac" | "wav" | "aiff" | "mp3" | "aac" | "opus" | "vorbis"

export type ConversationSummary = { username: string; last: ChatMessage | null; unread: number }

export type DiagnosticCheck = {
  id: string
  /**
   * The part of delune it's about: `soulseek`, `sharing`, `library`, `downloads`, `services`.
   */
  area: string
  title: string
  state: CheckState
  summary: string
  /**
   * What to do about it, when something's wrong.
   */
  fix: string | null
  /**
   * Where in the app to do it.
   */
  link: string | null
}

export type Diagnostics = {
  /**
   * Unix seconds.
   */
  checked_at: number
  version: string
  checks: Array<DiagnosticCheck>
}

export type DownloadJob = {
  id: string
  username: string
  folder: string
  title: string
  parent: string | null
  /**
   * Unix time in seconds.
   */
  created_at: number
  status: JobStatus
  files: Array<JobFile>
  bytes: number
  total_bytes: number
  review: ReviewState
  /**
   * Who started it. Jobs saved before accounts existed have none.
   */
  requested_by: string | null
  /**
   * Once imported: the folder it went to (relative to the library) and when.
   */
  imported_to: string | null
  imported_at: number | null
  /**
   * Jobs with a higher priority start first when only a few may download at once.
   */
  priority: number
  /**
   * While held back by the limit on downloads at once: its place in line, from 1.
   */
  waiting_for_slot: number | null
  /**
   * Why the job failed, when the files can't say (a fetch command that got nothing).
   */
  error: string | null
}

export type DownloadJobRequest = {
  username: string
  folder: string
  title: string
  parent: string | null
  files: Array<RequestedFile>
}

export type EntityKind = "track" | "album" | "artist" | "playlist"

export type FavouriteUser = {
  username: string
  /**
   * When they were starred (Unix seconds).
   */
  since: number
  /**
   * When their share list was last saved, if it has been.
   */
  saved_at: number | null
  /**
   * Folders and files in that saved list.
   */
  folders: number
  files: number
}

export type FileStatus =
  "waiting" | "connecting" | "queued" | "starting" | "transferring" | "done" | "failed" | "cancelled"

export type FinishUpload = {
  title: string | null
  artist: string | null
  /**
   * Keep the album complete from now on.
   */
  follow: boolean
}

export type Follow = {
  artist: string
  deezer_id: number
  picture: string | null
  added_by: string
  /**
   * Unix seconds. Releases from before this aren't fetched.
   */
  since: number
  last_checked: number | null
  /**
   * Releases already put on the wishlist.
   */
  seen: Array<number>
}

export type FollowAlbumRequest = { artist: string; album: string }

export type Health = { name: string; version: string; status: HealthStatus }

export type HealthFinding = {
  /**
   * Changes whenever the files involved do; a fix needs the current one.
   */
  id: string
  /**
   * Stays the same for the same folders; ignoring a finding uses this.
   */
  key: string
  kind: HealthKind
  /**
   * Library-relative folders; for a split album, the one kept comes first.
   */
  folders: Array<string>
  /**
   * For duplicate tracks: the copies of each track, library-relative.
   */
  duplicates: Array<Array<string>>
  /**
   * How many audio files a fix moves, retags or puts in the trash.
   */
  files: number
  /**
   * For a mixed album: the album every track will say it's on.
   */
  album: string | null
  album_artist: string | null
  /**
   * For a mixed album: how many tracks get their album tags corrected.
   */
  retag: number
  /**
   * For a mixed album: tracks of other albums, which move to those albums' folders.
   */
  strays: Array<string>
}

export type HealthFixRequest = { id: string }

export type HealthFixed = {
  moved: number
  trashed: number
  retagged: number
  /**
   * Restore this batch to undo the fix.
   */
  batch: string
}

export type HealthIgnoreRequest = { key: string }

export type HealthKind = "split-album" | "duplicate-tracks" | "mixed-album"

export type HealthStatus = "ok" | "degraded"

export type ImportResult = { imported: number; folder: string; scan_started: boolean }

export type JobFile = {
  path: string
  name: string
  size: number
  status: FileStatus
  bytes: number
  place_in_queue: number | null
  error: string | null
}

export type JobStatus = "queued" | "downloading" | "ready" | "failed" | "cancelled" | "imported"

export type LibraryHealth = {
  albums: number
  tracks: number
  findings: Array<HealthFinding>
  trash: Array<TrashBatch>
  /**
   * Batches in the trash are emptied after this many days.
   */
  trash_days: number
  /**
   * Findings left out because someone chose to ignore them.
   */
  ignored: number
}

export type LibraryMatch = {
  state: LibraryState
  album: string | null
  artist: string | null
  year: number | null
  /**
   * Tracks the library has, in disc and track order.
   */
  tracks: Array<LibraryTrack>
  /**
   * Quality of the library copy (the lowest across its tracks).
   */
  quality_label: string | null
}

export type LibraryState = "unknown" | "not-in-library" | "in-library"

export type LibraryStats = {
  /**
   * Unix seconds.
   */
  computed_at: number
  albums: number
  songs: number
  artists: number
  bytes: number
  seconds: number
  /**
   * Songs by quality, best first.
   */
  qualities: Array<StatShare>
  /**
   * Artists with the most albums.
   */
  top_artists: Array<StatShare>
  /**
   * Albums by decade of release, oldest first; `label` is like "1990s".
   */
  decades: Array<StatShare>
  genres: Array<StatShare>
  /**
   * Albums delune imported, by month ("2026-09"), oldest first.
   */
  imports: Array<StatShare>
  /**
   * Who imported the most, through delune.
   */
  importers: Array<StatShare>
  /**
   * The Soulseek users delune has downloaded most from; `count` is files.
   */
  top_peers: Array<StatShare>
}

export type LibraryTrack = { title: string; track: number | null; disc: number | null }

export type LinkBandcamp = {
  /**
   * The `identity` cookie, a whole `Cookie:` header, or a cookies.txt file.
   */
  cookie: string
}

export type LoginRequest = {
  username: string
  password: string
  /**
   * Return a bearer token in the response instead of relying on the cookie.
   */
  token: boolean
}

export type MatchedBy = "barcode" | "isrc" | "name"

export type Me = {
  username: string
  admin: boolean
  /**
   * Effective permissions (all of them for admins).
   */
  permissions: Permissions
  /**
   * Whether this person can import their own downloads right now.
   */
  can_import: boolean
  mode: AuthMode
  appearance: Appearance
  /**
   * Changes whenever their profile picture does; none without one.
   */
  avatar: number | null
  /**
   * Only returned to clients that asked for one, such as the TUI.
   */
  token?: string | null
}

export type MinQuality = "any" | "lossless" | "hi-res"

export type MusicBrainzMatch = {
  /**
   * The exact release, when matched by barcode or ISRC.
   */
  release_id: string | null
  release_group_id: string
  title: string
  artist: string | null
  /**
   * When the release first came out, whichever edition the link is.
   */
  original_year: number | null
  matched_by: MatchedBy
}

export type MusicRequest = {
  id: string
  requested_by: string
  /**
   * Unix seconds.
   */
  requested_at: number
  title: string
  artist: string | null
  /**
   * What to search Soulseek for.
   */
  query: string
  /**
   * The link it was found from, if any.
   */
  link: string | null
  /**
   * A particular copy the requester chose; otherwise the wishlist finds one.
   */
  download: DownloadJobRequest | null
  /**
   * That copy's quality, for display.
   */
  quality_label: string | null
  note: string | null
  status: RequestStatus
  decided_by: string | null
  decided_at: number | null
  /**
   * Why it was declined.
   */
  reason: string | null
  download_id: string | null
  wishlist_id: string | null
}

export type NewRequest = {
  title: string
  artist: string | null
  query: string
  link: string | null
  download: DownloadJobRequest | null
  quality_label: string | null
  note: string | null
}

export type Notification = {
  id: string
  kind: NotificationKind
  /**
   * Unix seconds.
   */
  at: number
  title: string
  detail: string | null
  /**
   * Where in the web UI to go, such as `/review`.
   */
  link: string | null
  read: boolean
}

export type NotificationKind =
  | "request-new"
  | "request-approved"
  | "request-declined"
  | "review-ready"
  | "download-failed"
  | "imported"
  | "new-release"

export type Notifications = { unread: number; items: Array<Notification> }

export type PeerHistory = {
  files_done: number
  files_failed: number
  bytes: number
  /**
   * Bytes per second, over everything downloaded from them.
   */
  average_speed: number
  /**
   * Unix seconds.
   */
  last_seen: number
}

export type People = {
  /**
   * Imports wait for an admin unless the person may skip approval.
   */
  require_approval: boolean
  people: Array<Person>
}

export type Permissions = {
  /**
   * Search Soulseek and open releases.
   */
  search: boolean
  /**
   * Start downloads. Everything downloaded still waits for review.
   */
  download: boolean
  /**
   * Import their own downloads even when imports need an admin's approval.
   */
  skip_approval: boolean
  /**
   * See and act on everyone's downloads, and manage people and settings.
   */
  manage: boolean
  /**
   * Ask for albums; someone who manages delune approves them. Matters for people
   * who can't download themselves.
   */
  request: boolean
}

export type Person = {
  username: string
  admin: boolean
  permissions: Permissions
  /**
   * Unix seconds.
   */
  last_login: number
  avatar: number | null
  /**
   * Devices they're signed in on.
   */
  sessions: number
}

export type PortMapping = {
  state: PortMappingState
  /**
   * The router's public address, once mapped.
   */
  external_ip: string | null
  /**
   * Why it failed.
   */
  message: string | null
}

export type PortMappingState = "off" | "mapped" | "failed"

export type Presence = "online" | "away" | "offline"

export type Provider =
  | "soulseek"
  | "qobuz"
  | "tidal"
  | "deezer"
  | "youtube-music"
  | "sound-cloud"
  | "bandcamp"
  | "spotify"
  | "apple-music"
  | "music-brainz"

export type PushDevice = {
  endpoint: string
  p256dh: string
  auth: string
  /**
   * What to call this device, like "Firefox on Linux".
   */
  label: string
}

export type Quality = {
  codec: Codec
  /**
   * Bits per sample (lossless only), e.g. 16 or 24.
   */
  bit_depth: number | null
  /**
   * Sample rate in Hz, e.g. 44100 or 96000.
   */
  sample_rate: number | null
  /**
   * Average bitrate in kbps.
   */
  bitrate_kbps: number | null
  /**
   * Variable bitrate (lossy only).
   */
  vbr: boolean
}

export type QualityTier = "unknown" | "lossy" | "lossless" | "hi-res"

export type RadarRelease = {
  /**
   * Deezer's album id.
   */
  id: number
  artist: string
  title: string
  /**
   * `album`, `ep` or `single`.
   */
  kind: string
  /**
   * `YYYY-MM-DD`.
   */
  release_date: string
  /**
   * Not out yet.
   */
  upcoming: boolean
  cover: string | null
  in_library: boolean
  /**
   * On the wishlist.
   */
  wished: boolean
  /**
   * A copy is downloading or waiting for review.
   */
  downloading: boolean
}

export type RecentAlbum = {
  id: string
  title: string
  artist: string | null
  year: number | null
  /**
   * Same-origin address of its cover, when it has one.
   */
  cover: string | null
  /**
   * Unix seconds.
   */
  added_at: number | null
}

export type RequestDecision = { approve: boolean; reason: string | null }

export type RequestStatus = "pending" | "declined" | "searching" | "downloading" | "review" | "available" | "failed"

export type RequestedFile = { path: string; size: number }

export type ResolvedLink = {
  provider: Provider
  kind: EntityKind
  /**
   * The album, track, artist or playlist name.
   */
  title: string
  artist: string | null
  /**
   * For a track: the album it's from, when the service says.
   */
  album: string | null
  year: number | null
  /**
   * The tracklist, for albums and playlists whose service lists one.
   */
  tracks: Array<ResolvedTrack>
  /**
   * What delune searches Soulseek for.
   */
  query: string
  /**
   * The album's barcode, when the service gives it.
   */
  upc: string | null
  /**
   * The track's ISRC, when the service gives it.
   */
  isrc: string | null
  /**
   * The same release on MusicBrainz, when it could be matched.
   */
  musicbrainz: MusicBrainzMatch | null
}

export type ResolvedTrack = {
  title: string
  artist: string | null
  /**
   * The album it's on, for playlists whose service says.
   */
  album: string | null
  duration_secs: number | null
}

export type ReviewReport = {
  album_artist: string
  album: string
  year: number | null
  tracks: Array<ReviewTrack>
  /**
   * Destination of the cover image, if one will be imported.
   */
  cover: string | null
  warnings: Array<string>
  /**
   * Destinations that already exist in the library.
   */
  conflicts: Array<string>
  /**
   * The library folder delune imports into, when configured.
   */
  library_dir: string | null
  /**
   * Why importing isn't possible right now, if it isn't.
   */
  blocked_reason: string | null
}

export type ReviewState = "waiting" | "checking" | "ready" | "failed"

export type ReviewTrack = {
  file: string
  /**
   * Where it will go, relative to the library.
   */
  destination: string
  title: string
  artist: string
  track: number
  disc: number
  quality: Quality | null
  quality_label: string | null
  duration_secs: number | null
  /**
   * Highest frequency with real content, for lossless files.
   */
  cutoff_hz: number | null
  suspect_transcode: boolean
  /**
   * What's wrong with this file, if anything, in plain language.
   */
  problem: string | null
  /**
   * The quality of the library's copy this track replaces (which goes to the
   * library's trash).
   */
  replaces: string | null
  /**
   * The library already has this track at least as good; it won't be imported.
   */
  skipped: boolean
}

export type RoomPerson = {
  username: string
  presence: Presence
  files: number
  avg_speed: number
  country: string | null
}

export type RoomSummary = { name: string; members: number; joined: boolean; unread: number }

export type RoomView = { name: string; joined: boolean; members: Array<RoomPerson>; messages: Array<ChatMessage> }

export type SearchEvent =
  | { type: "resolved"; link: ResolvedLink }
  | { type: "started"; query: string; timeout_secs: number }
  | { type: "candidates"; items: Array<Candidate> }
  | { type: "finished"; peers: number; candidates: number }
  | { type: "failed"; error: ApiError }

export type SessionInfo = {
  /**
   * Opaque; not the token.
   */
  id: string
  /**
   * What signed in, such as "Firefox on Linux".
   */
  device: string
  /**
   * Unix seconds.
   */
  created_at: number
  last_seen: number
  /**
   * The session making this request.
   */
  current: boolean
}

export type ShareFolder = {
  /**
   * Virtual path, `\`-separated.
   */
  path: string
  files: number
  audio_files: number
  bytes: number
  /**
   * Quality of the folder's weakest audio file.
   */
  quality_label: string | null
  quality_rank: number
}

export type ShareTree = {
  username: string
  folders: Array<ShareFolder>
  /**
   * Folders only their buddies can download from.
   */
  private_folders: number
  /**
   * Set when this is delune's saved copy of a favourite's shares (Unix seconds it
   * was fetched); a fresh list is on its way.
   */
  saved_at: number | null
}

export type SharingSettings = {
  enabled: boolean
  /**
   * The top folder other people see, instead of where the library really is.
   */
  share_name: string
  /**
   * Uploads at once.
   */
  slots: number
  /**
   * Files one person may have waiting.
   */
  queue_per_user: number
  /**
   * Upload speed cap in KiB/s; none means no cap.
   */
  speed_limit_kib: number | null
  /**
   * Download speed cap in KiB/s, shared by every download; none means no cap.
   */
  download_limit_kib: number | null
  /**
   * Refuse uploads to people who share nothing themselves.
   */
  refuse_leechers: boolean
  /**
   * Albums downloading at once; the rest wait their turn. None means no limit.
   */
  downloads_at_once: number | null
  /**
   * Ask the router to forward the Soulseek port (UPnP).
   */
  upnp: boolean
  /**
   * Other clients delune may pass searches on to in the distributed network;
   * 0 keeps it a leaf.
   */
  distributed_children: number
  /**
   * People who can't download from us.
   */
  banned: Array<string>
  /**
   * Different speed limits for part of each day.
   */
  schedule: SpeedSchedule | null
}

export type SharingStatus = {
  settings: SharingSettings
  /**
   * The folder being shared, when a library is configured.
   */
  library_dir: string | null
  scanning: boolean
  files: number
  folders: number
  /**
   * Unix seconds.
   */
  last_scan: number | null
  error: string | null
  /**
   * Whether the speed schedule's limits are in force right now.
   */
  scheduled: boolean
}

export type SoulseekProfile = {
  description: string
  has_picture: boolean
  queue_size: number
  slots_free: boolean
  total_uploads: number
}

export type SoulseekState = "not-configured" | "connecting" | "online" | "reconnecting" | "stopped"

export type SoulseekStats = {
  shared_files: number
  shared_folders: number
  uploads_running: number
  uploads_waiting: number
  downloads_running: number
  /**
   * Bytes, ever (since delune started keeping count).
   */
  downloaded_bytes: number
  uploaded_bytes: number
  uploads_completed: number
  /**
   * Clients delune passes distributed searches on to right now.
   */
  distributed_children: number
}

export type SoulseekStatus = {
  state: SoulseekState
  /**
   * The account delune logs in with, when configured.
   */
  username: string | null
  /**
   * Human-readable detail for anything other than `online`.
   */
  message: string | null
  /**
   * The address the Soulseek server sees delune at.
   */
  public_ip: string | null
  /**
   * The port other users connect to, when delune is listening.
   */
  listen_port: number | null
  /**
   * Someone on the internet has connected to that port, so it's open.
   */
  reachable: boolean
  port_mapping: PortMapping
}

export type SoulseekUser = {
  username: string
  /**
   * False when no account has that name.
   */
  exists: boolean
  presence: Presence
  /**
   * Upload speed in bytes per second, as the server measured it.
   */
  avg_speed: number
  files: number
  folders: number
  country: string | null
  /**
   * From the user themselves; absent when they couldn't be reached.
   */
  profile: SoulseekProfile | null
}

export type SoundcloudArtist = {
  id: number
  name: string
  permalink: string
  url: string
  avatar: string | null
  followers: number
  tracks: number
  verified: boolean
  /**
   * New tracks from them go onto the wishlist.
   */
  following: boolean
  recent: Array<SoundcloudTrack>
}

export type SoundcloudFollow = {
  id: number
  name: string
  permalink: string
  avatar: string | null
  added_by: string
  since: number
  last_checked: number | null
  /**
   * Tracks already seen, so only newer ones are wished for.
   */
  seen: Array<number>
}

export type SoundcloudTrack = {
  id: number
  title: string
  url: string
  published_at: number | null
  duration_secs: number | null
  artwork: string | null
}

export type SoundcloudTrackDetail = {
  title: string
  artist: string
  url: string
  artwork: string | null
  duration_secs: number | null
  album: string | null
  /**
   * Where the artist offers it for free: their own link, or the track page when
   * SoundCloud's download button is on. None when it isn't given away.
   */
  free_download: string | null
}

export type SpeedSchedule = {
  /**
   * Minutes after midnight the window opens, in `time_zone`.
   */
  start_minute: number
  /**
   * Minutes after midnight it closes; before `start_minute` means it runs past midnight.
   */
  end_minute: number
  /**
   * Upload cap in KiB/s during the window; none means no cap.
   */
  upload_limit_kib: number | null
  /**
   * Download cap in KiB/s during the window; none means no cap.
   */
  download_limit_kib: number | null
  /**
   * IANA time zone the times are in, such as `Europe/London`.
   */
  time_zone: string
}

export type StatShare = {
  label: string
  count: number
  /**
   * Bytes, where that means something.
   */
  bytes: number
  /**
   * For qualities: `hires`, `lossless` or `lossy`.
   */
  tier: string | null
}

export type Theme = "system" | "night" | "blue-hour" | "midnight" | "forest"

export type TransferHistory = {
  /**
   * Start of the period, Unix seconds; none for all time.
   */
  since: number | null
  /**
   * Over the period.
   */
  uploaded_bytes: number
  downloaded_bytes: number
  /**
   * Since delune started keeping count.
   */
  all_time_uploaded_bytes: number
  all_time_downloaded_bytes: number
  /**
   * Files sent in full over the period, and how many people got them.
   */
  files_sent: number
  people: number
  /**
   * Bytes per hour, oldest first, for charts (at most the last 90 days).
   */
  hours: Array<TransferHour>
  /**
   * Who took the most, most first.
   */
  top_people: Array<UploadPerson>
  /**
   * The folders taken most, most first.
   */
  top_albums: Array<UploadAlbum>
  /**
   * Finished uploads, newest first.
   */
  recent: Array<UploadRecord>
}

export type TransferHour = {
  /**
   * Unix seconds at the start of the hour.
   */
  hour: number
  uploaded_bytes: number
  downloaded_bytes: number
}

export type TrashBatch = {
  id: string
  /**
   * Unix seconds.
   */
  created_at: number
  /**
   * What it was, e.g. "Merged 2 folders into Fred again../USB".
   */
  label: string | null
  changes: Array<TrashChange>
}

export type TrashChange = {
  /**
   * `removed` (now in the trash), `moved` or `retagged`.
   */
  kind: string
  /**
   * Library-relative.
   */
  path: string
  /**
   * Where a moved file went.
   */
  to: string | null
}

export type TrashRestored = {
  /**
   * Files that couldn't go back because their place is taken; they stay in the trash.
   */
  left: Array<string>
}

export type Upload = {
  id: number
  username: string
  /**
   * The shared path they asked for.
   */
  filename: string
  size: number
  bytes: number
  status: UploadStatus
  reason: string | null
  /**
   * Unix seconds.
   */
  queued_at: number
  /**
   * Bytes per second.
   */
  speed: number
}

export type UploadAlbum = {
  /**
   * The shared folder, as peers see it.
   */
  folder: string
  /**
   * The folder's name, and the one above it (usually the artist).
   */
  title: string
  parent: string | null
  files: number
  bytes: number
  people: number
  /**
   * Unix seconds.
   */
  last_at: number
}

export type UploadPerson = {
  username: string
  files: number
  bytes: number
  /**
   * Unix seconds.
   */
  last_at: number
}

export type UploadReceived = { received: number }

export type UploadRecord = {
  username: string
  /**
   * The shared path they asked for.
   */
  filename: string
  size: number
  bytes: number
  status: UploadStatus
  reason: string | null
  /**
   * Bytes per second.
   */
  speed: number
  /**
   * Unix seconds.
   */
  finished_at: number
}

export type UploadSession = {
  id: string
  /**
   * The largest piece one request may carry, in bytes.
   */
  max_chunk: number
}

export type UploadStatus = "queued" | "connecting" | "transferring" | "completed" | "failed" | "cancelled"

export type WishlistItem = {
  id: string
  query: string
  /**
   * For a single song: its title, matched against file names. Its download is
   * just that file.
   */
  track: string | null
  /**
   * The playlist it came from, if any.
   */
  playlist: string | null
  added_by: string
  /**
   * Unix seconds.
   */
  added_at: number
  /**
   * Start a download (for review) as soon as a good enough copy turns up.
   */
  auto_download: boolean
  min_quality: MinQuality
  paused: boolean
  last_searched: number | null
  /**
   * Good enough copies found by the last search.
   */
  last_matches: number
  /**
   * The best of them.
   */
  best: Candidate | null
  /**
   * The download started for this item, once one has been.
   */
  download_id: string | null
}

export type WishlistRequest = {
  query: string
  track: string | null
  playlist: string | null
  auto_download: boolean
  min_quality: MinQuality
}

export type WishlistUpdate = { auto_download: boolean | null; min_quality: MinQuality | null; paused: boolean | null }
