// Hand-written mirror of delune-core::api and the server routes. Replaced by a
// client generated from the OpenAPI document once utoipa is wired in.

export type Health = { name: string; version: string; status: "ok" | "degraded" }

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

export type EntityKind = "track" | "album" | "artist" | "playlist"

export type Classification =
  | { kind: "empty" }
  | { kind: "text"; query: string }
  | { kind: "link"; provider: Provider; entity: EntityKind; id: string }
  | { kind: "short-link"; provider: Provider }

export type SourceInfo = {
  provider: Provider
  name: string
  role: "peer-to-peer" | "streaming" | "metadata-only"
  enabled: boolean
  order: number | null
}

export type SoulseekState = "not-configured" | "connecting" | "online" | "reconnecting" | "stopped"
export type SoulseekStatus = { state: SoulseekState; username: string | null; message: string | null }

export type Codec = "flac" | "alac" | "wav" | "aiff" | "mp3" | "aac" | "opus" | "vorbis"

export type Quality = {
  codec: Codec
  bit_depth: number | null
  sample_rate: number | null
  bitrate_kbps: number | null
  vbr: boolean
}

export type CandidateFile = {
  path: string
  name: string
  size: number
  audio: boolean
  quality: Quality | null
  quality_label: string | null
  duration_secs: number | null
}

export type Candidate = {
  id: string
  username: string
  folder: string
  title: string
  parent: string | null
  files: CandidateFile[]
  audio_files: number
  total_bytes: number
  duration_secs: number | null
  quality: Quality | null
  quality_label: string | null
  quality_rank: number
  mixed_quality: boolean
  has_cover: boolean
  free_slot: boolean
  avg_speed: number
  queue_length: number
}

export type FileStatus = "waiting" | "connecting" | "queued" | "starting" | "transferring" | "done" | "failed" | "cancelled"
export type JobStatus = "queued" | "downloading" | "ready" | "failed" | "cancelled" | "imported"
export type ReviewState = "waiting" | "checking" | "ready" | "failed"

export type ReviewTrack = {
  file: string
  destination: string
  title: string
  artist: string
  track: number
  disc: number
  quality: Quality | null
  quality_label: string | null
  duration_secs: number | null
  cutoff_hz: number | null
  suspect_transcode: boolean
  problem: string | null
}

export type ReviewReport = {
  album_artist: string
  album: string
  year: number | null
  tracks: ReviewTrack[]
  cover: string | null
  warnings: string[]
  conflicts: string[]
  library_dir: string | null
  blocked_reason: string | null
}

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

export type DownloadJob = {
  id: string
  username: string
  folder: string
  title: string
  parent: string | null
  created_at: number
  status: JobStatus
  files: JobFile[]
  bytes: number
  total_bytes: number
  review: ReviewState
  /** Who started it; null for downloads from before accounts. */
  requested_by: string | null
}

export type DownloadJobRequest = {
  username: string
  folder: string
  title: string
  parent: string | null
  files: { path: string; size: number }[]
}

export type ApiErrorBody = { code: string; message: string }

export type ResolvedTrack = { title: string; artist: string | null; duration_secs: number | null }

/** A pasted link, resolved to what it points at. */
export type ResolvedLink = {
  provider: Provider
  kind: EntityKind
  title: string
  artist: string | null
  album: string | null
  year: number | null
  tracks: ResolvedTrack[]
  query: string
}

export type SearchEvent =
  | { type: "resolved"; link: ResolvedLink }
  | { type: "started"; query: string; timeout_secs: number }
  | { type: "candidates"; items: Candidate[] }
  | { type: "finished"; peers: number; candidates: number }
  | { type: "failed"; error: ApiErrorBody }

export type TokenInfo = { name: string; description: string }

export type NamingOptions = {
  track_padding: number
  multi_disc: "disc-prefix" | "continuous" | "per-disc"
  illegal_replacement: string
  whitespace: "preserve" | "collapse" | "underscore"
  max_component_bytes: number
}

export type NamingPreview =
  | { examples: { label: string; path: string }[] }
  | { error: { position: number; message: string } }

export const PROVIDER_NAMES: Record<Provider, string> = {
  soulseek: "Soulseek",
  qobuz: "Qobuz",
  tidal: "Tidal",
  deezer: "Deezer",
  "youtube-music": "YouTube Music",
  "sound-cloud": "SoundCloud",
  bandcamp: "Bandcamp",
  spotify: "Spotify",
  "apple-music": "Apple Music",
  "music-brainz": "MusicBrainz",
}

export type Permissions = { search: boolean; download: boolean; skip_approval: boolean; manage: boolean }

export type Me = {
  username: string
  admin: boolean
  permissions: Permissions
  can_import: boolean
  mode: "navidrome" | "open"
}

export type Person = { username: string; admin: boolean; permissions: Permissions; last_login: number }
export type People = { require_approval: boolean; people: Person[] }

export type SoulseekUser = {
  username: string
  exists: boolean
  presence: "online" | "away" | "offline"
  avg_speed: number
  files: number
  folders: number
  country: string | null
  profile: { description: string; has_picture: boolean; queue_size: number; slots_free: boolean; total_uploads: number } | null
}

export type ShareFolder = {
  path: string
  files: number
  audio_files: number
  bytes: number
  quality_label: string | null
  quality_rank: number
}

export type ShareTree = { username: string; folders: ShareFolder[]; private_folders: number }

/** Fired whenever the server says the session is gone, so the app can show sign-in. */
export const sessionEvents = new EventTarget()

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  constructor(status: number, code: string, message: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

/** Turn a failed response into an ApiError, using the server's message when there is one. */
export async function toApiError(res: Response): Promise<ApiError> {
  if (res.status === 401) sessionEvents.dispatchEvent(new Event("signed-out"))
  try {
    const body = (await res.json()) as Partial<ApiErrorBody>
    if (body.message) return new ApiError(res.status, body.code ?? "error", body.message)
  } catch {
    // Not JSON; fall through.
  }
  return new ApiError(res.status, "http", `The server answered ${res.status} ${res.statusText}.`)
}

async function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`/api/v1${path}`, { signal, headers: { accept: "application/json" } })
  if (!res.ok) throw await toApiError(res)
  return res.json() as Promise<T>
}

async function send<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 ? undefined : await res.json()) as T
}

export const api = {
  /** The signed-in person, or null when nobody is. */
  session: async (signal?: AbortSignal): Promise<Me | null> => {
    const res = await fetch("/api/v1/session", { signal, headers: { accept: "application/json" } })
    if (!res.ok) throw await toApiError(res)
    return res.json() as Promise<Me | null>
  },
  signIn: (username: string, password: string) => send<Me>("POST", "/session", { username, password }),
  signOut: () => send<void>("DELETE", "/session"),
  people: (signal?: AbortSignal) => get<People>("/users", signal),
  soulseekUser: (username: string, signal?: AbortSignal) =>
    get<SoulseekUser>(`/soulseek/users/${encodeURIComponent(username)}`, signal),
  shareTree: (username: string, signal?: AbortSignal) =>
    get<ShareTree>(`/soulseek/users/${encodeURIComponent(username)}/shares`, signal),
  sharedFolder: (username: string, path: string, signal?: AbortSignal) =>
    get<Candidate>(`/soulseek/users/${encodeURIComponent(username)}/folder?path=${encodeURIComponent(path)}`, signal),
  setPermissions: (username: string, permissions: Permissions) =>
    send<People>("PUT", `/users/${encodeURIComponent(username)}/permissions`, { permissions }),
  setRequireApproval: (require_approval: boolean) => send<People>("PUT", "/users/approval", { require_approval }),
  health: (signal?: AbortSignal) => get<Health>("/health", signal),
  sources: (signal?: AbortSignal) => get<SourceInfo[]>("/sources", signal),
  soulseek: (signal?: AbortSignal) => get<SoulseekStatus>("/soulseek", signal),
  classify: (q: string, signal?: AbortSignal) => get<Classification>(`/classify?q=${encodeURIComponent(q)}`, signal),
  downloads: (signal?: AbortSignal) => get<DownloadJob[]>("/downloads", signal),
  startDownload: async (request: DownloadJobRequest): Promise<DownloadJob> => {
    const res = await fetch("/api/v1/downloads", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(request),
    })
    if (!res.ok) throw await toApiError(res)
    return res.json() as Promise<DownloadJob>
  },
  removeDownload: async (id: string): Promise<void> => {
    const res = await fetch(`/api/v1/downloads/${encodeURIComponent(id)}`, { method: "DELETE" })
    if (!res.ok && res.status !== 404) throw await toApiError(res)
  },
  review: async (id: string, signal?: AbortSignal): Promise<ReviewReport | null> => {
    const res = await fetch(`/api/v1/downloads/${encodeURIComponent(id)}/review`, { signal })
    if (res.status === 202) return null
    if (!res.ok) throw await toApiError(res)
    return res.json() as Promise<ReviewReport>
  },
  importRelease: async (id: string): Promise<ImportResult> => {
    const res = await fetch(`/api/v1/downloads/${encodeURIComponent(id)}/import`, { method: "POST" })
    if (!res.ok) throw await toApiError(res)
    return res.json() as Promise<ImportResult>
  },
  namingTokens: (signal?: AbortSignal) => get<TokenInfo[]>("/naming/tokens", signal),
  namingPreview: async (template: string, options: NamingOptions, signal?: AbortSignal): Promise<NamingPreview> => {
    const res = await fetch("/api/v1/naming/preview", {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ template, options }),
    })
    if (res.ok || res.status === 422) return res.json() as Promise<NamingPreview>
    throw await toApiError(res)
  },
}
