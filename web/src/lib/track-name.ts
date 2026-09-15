const AUDIO = new Set(["flac", "alac", "wav", "aif", "aiff", "mp3", "m4a", "aac", "opus", "ogg", "oga", "wv", "ape", "dsf", "dff"])

export type TrackName = {
  /** Printed position: "3", "2-04", or a vinyl side like "A1". */
  position?: string
  title: string
  extension: string
}

/**
 * Pull a track position and a readable title out of a shared file name.
 *
 *   "Pink Floyd - The Dark Side of the Moon - 03 - Time.flac" → { position: "3", title: "Time" }
 *   "01. Speak to Me.flac"                                   → { position: "1", title: "Speak to Me" }
 *   "2-04 Lift.flac"                                         → { position: "2-4", title: "Lift" }
 *   "A1. Sixtyniner.flac"                                    → { position: "A1", title: "Sixtyniner" }
 *
 * Non-audio files keep their full name: "cover.jpg" stays "cover.jpg".
 */
export function parseTrackName(fileName: string): TrackName {
  const dot = fileName.lastIndexOf(".")
  const extension = dot > 0 ? fileName.slice(dot + 1).toLowerCase() : ""
  if (!AUDIO.has(extension)) return { title: fileName, extension }

  const stem = fileName.slice(0, dot).replace(/_/g, " ").trim()

  // "Artist - Album - 03 - Title": the segment that is just a number.
  const parts = stem.split(/\s+-\s+/)
  const numberIndex = parts.findIndex((p) => /^\d{1,3}$/.test(p.trim()))
  if (numberIndex >= 0 && numberIndex < parts.length - 1) {
    return { position: String(Number(parts[numberIndex])), title: parts.slice(numberIndex + 1).join(" - "), extension }
  }

  const vinyl = stem.match(/^([A-H]\d{1,2})[\s.\-)]+(.+)$/)
  if (vinyl) return { position: vinyl[1], title: vinyl[2], extension }

  const numbered = stem.match(/^(?:(\d)[-.])?(\d{1,3})[\s.\-)]+(.+)$/)
  if (numbered) {
    const position = numbered[1] ? `${numbered[1]}-${Number(numbered[2])}` : String(Number(numbered[2]))
    return { position, title: numbered[3].replace(/^[-.\s]+/, ""), extension }
  }

  return { title: parts.at(-1) ?? stem, extension }
}

/**
 * A comparison key for song titles: case, accents, punctuation and bracketed extras
 * ("(Remastered)", "[Live]") don't count, and "&" reads as "and".
 */
export function titleKey(title: string): string {
  return title
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/\(.*?\)|\[.*?\]/g, "")
    .replace(/&/g, "and")
    .replace(/colour/g, "color")
    .replace(/[^a-z0-9]+/g, "")
}

/** Whether two title keys name the same song, allowing for a prefix or suffix on one of them. */
export function sameTitle(a: string, b: string): boolean {
  if (!a || !b) return false
  return a === b || (b.length >= 4 && a.includes(b)) || (a.length >= 4 && b.includes(a))
}
