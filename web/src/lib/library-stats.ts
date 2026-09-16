import { useQuery } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"
import type { LibraryStats, RecentAlbum } from "@/lib/api.generated"

export type { LibraryStats, RecentAlbum, StatShare } from "@/lib/api.generated"

async function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(`/api/v1${path}`, { signal })
  if (!res.ok) throw await toApiError(res)
  return res.json() as Promise<T>
}

/** Counting the library takes a moment on the server; the answer is kept for an hour. */
export function useLibraryStats() {
  return useQuery({
    queryKey: ["library", "stats"],
    queryFn: ({ signal }) => get<LibraryStats>("/stats", signal),
    staleTime: 10 * 60_000,
    retry: false,
  })
}

export function useRecentAlbums() {
  return useQuery({
    queryKey: ["library", "recent"],
    queryFn: ({ signal }) => get<RecentAlbum[]>("/library/recent", signal),
    staleTime: 60_000,
    retry: false,
  })
}

/** 187_000 seconds → "52 hours"; long libraries in days. */
export function formatListeningTime(seconds: number): string {
  const hours = seconds / 3600
  if (hours >= 72) return `${Math.round(hours / 24).toLocaleString()} days`
  if (hours >= 1) return `${Math.round(hours).toLocaleString()} hours`
  return `${Math.round(seconds / 60)} minutes`
}
