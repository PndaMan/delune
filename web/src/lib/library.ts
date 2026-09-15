import { useQuery } from "@tanstack/react-query"
import { useContext } from "react"

import type { CoverStatus } from "@/components/cover"
import type { Candidate, DownloadJob } from "@/lib/api"
import { SearchContext } from "@/lib/artwork"
import { parseTrackName, sameTitle, titleKey } from "@/lib/track-name"

export type LibraryTrack = { title: string; track: number | null; disc: number | null }
export type LibraryMatch = {
  state: "unknown" | "not-in-library" | "in-library"
  album: string | null
  artist: string | null
  year: number | null
  tracks: LibraryTrack[]
  quality_label: string | null
}

/** What your Navidrome library already holds of this release. Cached briefly, like the server. */
export function useLibraryAlbum(artist: string | null, album: string | null) {
  const context = useContext(SearchContext)
  return useQuery({
    queryKey: ["library", artist ?? "", album ?? "", context ?? ""],
    enabled: !!album,
    staleTime: 60_000,
    retry: false,
    queryFn: async ({ signal }): Promise<LibraryMatch> => {
      const params = new URLSearchParams({ album: album ?? "" })
      if (artist) params.set("artist", artist)
      if (context) params.set("context", context)
      const res = await fetch(`/api/v1/library/album?${params}`, { signal })
      if (!res.ok) throw new Error(`library lookup failed: ${res.status}`)
      return res.json() as Promise<LibraryMatch>
    },
  })
}

export type Ownership = {
  /** Every audio file in this folder has a counterpart in the library. */
  complete: boolean
  /** Audio file names (not paths) the library doesn't have yet. */
  missing: Set<string>
  owned: number
}

/**
 * Compare a shared folder with the library copy, track by track: a file counts as
 * owned when its title matches a library track's. Returns null when the album
 * isn't in the library at all.
 */
export function ownership(candidate: Candidate, library: LibraryMatch | undefined): Ownership | null {
  if (!library || library.state !== "in-library") return null
  const titles = library.tracks.map((t) => titleKey(t.title)).filter(Boolean)

  const missing = new Set<string>()
  let owned = 0
  for (const file of candidate.files) {
    if (!file.audio) continue
    const title = titleKey(parseTrackName(file.name).title)
    if (titles.some((t) => sameTitle(title, t))) owned++
    else missing.add(file.name)
  }
  return { complete: missing.size === 0, missing, owned }
}

/** Which overlay a cover gets: an active download wins over what the library holds. */
export function coverStatus(job: DownloadJob | undefined, owned: Ownership | null): CoverStatus | undefined {
  if (job && (job.status === "queued" || job.status === "downloading")) {
    return { kind: "downloading", progress: job.total_bytes ? job.bytes / job.total_bytes : 0 }
  }
  if (job?.status === "imported" || owned?.complete) return { kind: "in-library" }
  if (owned && owned.owned > 0) return { kind: "in-library", partial: true }
  return undefined
}
