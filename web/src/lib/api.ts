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
export type JobStatus = "queued" | "downloading" | "ready" | "failed" | "cancelled"

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
}

export type DownloadJobRequest = {
  username: string
  folder: string
  title: string
  parent: string | null
  files: { path: string; size: number }[]
}

export type ApiErrorBody = { code: string; message: string }

export type SearchEvent =
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

export const api = {
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
