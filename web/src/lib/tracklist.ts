import { createContext, useContext } from "react"

import type { Candidate, CandidateFile, ResolvedLink } from "@/lib/api"
import { parseTrackName, sameTitle, titleKey } from "@/lib/track-name"

/** The link the current search came from, if it did. */
export const ResolvedContext = createContext<ResolvedLink | null>(null)
export const useResolved = () => useContext(ResolvedContext)

export type LinkMatch =
  /** How much of the linked album's tracklist this folder holds. */
  | { kind: "album"; matched: number; total: number }
  /** The linked track's file in this folder, if it has one. */
  | { kind: "track"; file: CandidateFile | undefined }

const cache = new WeakMap<Candidate, { link: ResolvedLink; match: LinkMatch | null }>()

/**
 * Compare a shared folder with what a link points at. Null when there's nothing to
 * compare against: no link, or a service that doesn't list tracks.
 */
export function matchLink(candidate: Candidate, link: ResolvedLink | null): LinkMatch | null {
  if (!link) return null
  const hit = cache.get(candidate)
  if (hit?.link === link) return hit.match

  const files = candidate.files.filter((f) => f.audio).map((file) => ({ file, key: titleKey(parseTrackName(file.name).title) }))
  let match: LinkMatch | null = null
  if (link.kind === "track") {
    const wanted = titleKey(link.title)
    match = { kind: "track", file: files.find((f) => sameTitle(f.key, wanted))?.file }
  } else if (link.kind === "album" && link.tracks.length > 0) {
    const matched = link.tracks.filter((t) => {
      const wanted = titleKey(t.title)
      return files.some((f) => sameTitle(f.key, wanted))
    }).length
    match = { kind: "album", matched, total: link.tracks.length }
  }
  cache.set(candidate, { link, match })
  return match
}

/** 0 is the closest match. Sorting by this first puts the linked release at the top. */
export function relevance(match: LinkMatch | null): number {
  if (!match) return 0
  if (match.kind === "track") return match.file ? 0 : 1
  const ratio = match.matched / Math.max(1, match.total)
  return ratio >= 0.9 ? 0 : ratio >= 0.5 ? 1 : 2
}
