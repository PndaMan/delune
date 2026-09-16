import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { ApiError, toApiError } from "@/lib/api"

export type SoundcloudTrack = {
  id: number
  title: string
  url: string
  published_at: number | null
  duration_secs: number | null
  artwork: string | null
}

/** An artist's SoundCloud, with what they put out lately. */
export type SoundcloudArtist = {
  id: number
  name: string
  permalink: string
  url: string
  avatar: string | null
  followers: number
  tracks: number
  verified: boolean
  following: boolean
  recent: SoundcloudTrack[]
}

export type SoundcloudTrackDetail = {
  title: string
  artist: string
  url: string
  artwork: string | null
  duration_secs: number | null
  album: string | null
  /** Where the artist gives it away, if they do. */
  free_download: string | null
}

export type SoundcloudFollow = {
  id: number
  name: string
  permalink: string
  avatar: string | null
  added_by: string
  since: number
  last_checked: number | null
  seen: number[]
}

async function call<T>(method: string, path: string): Promise<T> {
  const res = await fetch(`/api/v1/soundcloud${path}`, { method })
  if (!res.ok) throw await toApiError(res)
  return (await res.json()) as T
}

/** Missing from SoundCloud is an answer, not an error. */
async function orNull<T>(request: Promise<T>): Promise<T | null> {
  try {
    return await request
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) return null
    throw e
  }
}

export function useSoundcloudArtist(name: string | null | undefined) {
  return useQuery({
    queryKey: ["soundcloud", "artist", name],
    queryFn: () => orNull(call<SoundcloudArtist>("GET", `/artist?name=${encodeURIComponent(name ?? "")}`)),
    enabled: !!name,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useSoundcloudTrack(url: string | null) {
  const path = url?.replace(/^https?:\/\/(www\.|m\.)?soundcloud\.com\//, "") ?? ""
  return useQuery({
    queryKey: ["soundcloud", "track", path],
    queryFn: () => orNull(call<SoundcloudTrackDetail>("GET", `/track?path=${encodeURIComponent(path)}`)),
    enabled: !!path,
    staleTime: 30 * 60_000,
    retry: false,
  })
}

export function useSoundcloudFollows() {
  return useQuery({ queryKey: ["soundcloud", "follows"], queryFn: () => call<SoundcloudFollow[]>("GET", "/follows") })
}

export function useSetSoundcloudFollow() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ artist, on }: { artist: string; on: boolean }) =>
      call<SoundcloudFollow[]>(on ? "PUT" : "DELETE", `/follows/${encodeURIComponent(artist)}`),
    onSuccess: (follows) => {
      client.setQueryData(["soundcloud", "follows"], follows)
      void client.invalidateQueries({ queryKey: ["soundcloud", "artist"] })
    },
  })
}
