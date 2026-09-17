import type { Candidate, CandidateFile } from "@/lib/api"
import type { ArtistHit } from "@/lib/music"
import { parseTrackName } from "@/lib/track-name"

/** Lowercase letters and digits, accents folded: how names are compared. */
const key = (text: string) =>
  text
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "")

const words = (text: string) =>
  text
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean)

/** The artist the search names, when it names one: "fred again" and "fred again usb" both name Fred again.. */
export function namedArtist(query: string, artists: ArtistHit[] | undefined): ArtistHit | null {
  const q = key(query)
  if (q.length < 2) return null
  return (
    (artists ?? []).find((a) => {
      const name = key(a.name)
      return name.length >= 2 && (q === name || (name.length >= 3 && q.startsWith(name)))
    }) ?? null
  )
}

/** Failing a match among artists, one whose album the search found: "radiohead creep". */
export function artistOfAlbums(query: string, albums: { artist: string }[] | undefined): ArtistHit | null {
  const q = key(query)
  const found = (albums ?? []).find((a) => {
    const name = key(a.artist)
    return name.length >= 3 && q.startsWith(name)
  })
  return found ? { name: found.artist, picture: null, listeners: null } : null
}

/** One person's copy of one song. */
export type TrackHit = {
  id: string
  title: string
  file: CandidateFile
  /** The folder it's in, narrowed to this song (and its artwork), ready to open or download. */
  candidate: Candidate
  album: Candidate
}

/**
 * Songs the search names: every word of the song's title is in the search, and the
 * rest of the search (the artist, say) is in the folder the song sits in.
 */
export function trackHits(query: string, candidates: Candidate[]): TrackHit[] {
  const asked = words(query)
  if (!asked.length) return []
  const hits: TrackHit[] = []
  for (const c of candidates) {
    const where = new Set(words(`${c.parent ?? ""} ${c.folder}`))
    const images = c.files.filter((f) => !f.audio && /\.(jpe?g|png|webp)$/i.test(f.name))
    for (const file of c.files) {
      if (!file.audio) continue
      const title = parseTrackName(file.name).title
      const titleWords = words(title.replace(/\s*[([][^)\]]*[)\]]/g, ""))
      if (!titleWords.length || !titleWords.every((w) => asked.includes(w))) continue
      const rest = asked.filter((w) => !titleWords.includes(w))
      if (!rest.every((w) => where.has(w))) continue
      hits.push({
        id: `${c.id}${file.path}`,
        title,
        file,
        album: c,
        candidate: {
          ...c,
          files: [file, ...images],
          audio_files: 1,
          total_bytes: file.size + images.reduce((n, f) => n + f.size, 0),
          duration_secs: file.duration_secs,
          quality: file.quality,
          quality_label: file.quality_label,
          mixed_quality: false,
        },
      })
    }
  }
  // Ready to start first, then the best copies.
  return hits.sort(
    (a, b) =>
      Number(b.album.free_slot) - Number(a.album.free_slot) ||
      (b.file.quality ? 1 : 0) - (a.file.quality ? 1 : 0) ||
      b.album.quality_rank - a.album.quality_rank ||
      b.album.avg_speed - a.album.avg_speed,
  )
}
