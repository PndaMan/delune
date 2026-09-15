// Hand-written for now; replaced by a client generated from the server's OpenAPI
// document once utoipa is wired in (see docs/ARCHITECTURE.md).

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
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

async function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`/api/v1${path}`, { signal, headers: { accept: "application/json" } })
  if (!res.ok) throw new ApiError(res.status, `${res.status} ${res.statusText}`)
  return res.json() as Promise<T>
}

export const api = {
  health: (signal?: AbortSignal) => get<Health>("/health", signal),
  sources: (signal?: AbortSignal) => get<SourceInfo[]>("/sources", signal),
  classify: (q: string, signal?: AbortSignal) =>
    get<Classification>(`/classify?q=${encodeURIComponent(q)}`, signal),
}
