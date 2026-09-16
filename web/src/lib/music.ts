import { useQuery } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"
import type { LibraryMatch } from "@/lib/library"

export type ArtistAlbum = {
  title: string
  year: number | null
  cover: string | null
  /** `album`, `ep` or `single`. */
  kind: string
  in_library: boolean
}

export type LibraryAlbum = { id: string; title: string; year: number | null; track_count: number }

export type ArtistInfo = {
  name: string
  picture: string | null
  listeners: number | null
  albums: ArtistAlbum[]
  in_library: LibraryAlbum[]
}

export type AlbumTrack = { position: number; title: string; artist: string | null; duration_secs: number | null }

export type AlbumInfo = {
  title: string
  artist: string | null
  year: number | null
  cover: string | null
  tracks: AlbumTrack[]
  in_library: LibraryMatch
}

export type Words = { synced: string | null; plain: string | null }

async function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`/api/v1${path}`, { signal })
  if (!res.ok) throw await toApiError(res)
  return res.json() as Promise<T>
}

const HOUR = 60 * 60 * 1000

export function useArtist(name: string) {
  return useQuery({
    queryKey: ["music", "artist", name],
    queryFn: ({ signal }) => get<ArtistInfo>(`/music/artist?name=${encodeURIComponent(name)}`, signal),
    staleTime: HOUR,
    retry: false,
  })
}

export function useAlbum(artist: string | null | undefined, album: string | null) {
  const query = artist ? `?artist=${encodeURIComponent(artist)}&album=` : "?album="
  return useQuery({
    queryKey: ["music", "album", artist ?? "", album ?? ""],
    enabled: !!album,
    queryFn: ({ signal }) => get<AlbumInfo>(`/music/album${query}${encodeURIComponent(album ?? "")}`, signal),
    staleTime: HOUR,
    retry: false,
  })
}

export function useLyrics(artist: string | null | undefined, title: string | null, durationSecs?: number | null) {
  return useQuery({
    queryKey: ["music", "lyrics", artist ?? "", title ?? "", durationSecs ?? 0],
    enabled: !!artist && !!title,
    queryFn: ({ signal }) => {
      const params = new URLSearchParams({ artist: artist ?? "", title: title ?? "" })
      if (durationSecs) params.set("duration_secs", String(durationSecs))
      return get<Words>(`/music/lyrics?${params.toString()}`, signal)
    },
    staleTime: HOUR,
    retry: false,
  })
}

/** Strip LRC timings: `[00:12.34] words` becomes `words`. */
export function plainFrom(words: Words): string[] {
  const text = words.plain ?? words.synced ?? ""
  return text
    .split("\n")
    .map((line) => line.replace(/^\[\d+:\d+(\.\d+)?\]\s*/, ""))
    .map((line) => line.trimEnd())
}
