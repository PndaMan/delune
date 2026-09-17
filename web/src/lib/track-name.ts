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

  const raw = fileName.slice(0, dot).trim()
  // Scene releases: "01-radiohead-creep_(original_version).flac".
  const scene = !raw.includes(" ") ? raw.match(/^(\d{1,3})-([^-].*)$/) : null
  if (scene) {
    const parts = scene[2].split("-").map((p) => p.replace(/_/g, " ").trim()).filter(Boolean)
    const title = parts.length > 1 ? parts.slice(1).join(" - ") : parts[0]
    if (title) return { position: String(Number(scene[1])), title: tidyCase(title), extension }
  }

  const stem = raw.replace(/_/g, " ").trim()

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

/** Scene file names are all lower case; "creep (original version)" reads as "Creep (Original Version)". */
function tidyCase(title: string): string {
  if (title !== title.toLowerCase()) return title
  return title.replace(/(^|[\s([])(\p{L})/gu, (_, before: string, letter: string) => before + letter.toUpperCase())
}

/** A scene release folder: "Hum-Youd_Prefer_An_Astronaut-1995-FLAC". Its name is for machines. */
export function isSceneName(folder: string): boolean {
  return !folder.includes(" ") && folder.includes("-") && (folder.includes("_") || folder.split("-").length > 3)
}

/** Bracketed words that are notes, not part of the song's name: "(feat. …)", "[2011 Remaster]". */
const NOTE = /^\s*(feat|ft\b|featuring|with\b|prod\b)|remaster|explicit|clean|bonus|album version|mono|stereo/i

/**
 * A comparison key for song titles: case, accents, punctuation and bracketed notes
 * ("(Remastered)", "[feat. X]") don't count, and "&" reads as "and". Brackets that
 * name a version, like "(Dapa remix)", stay: that's a different song.
 */
export function titleKey(title: string): string {
  return title
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/\(([^)]*)\)|\[([^\]]*)\]/g, (_, round: string | undefined, square: string | undefined) => {
      const inner = round ?? square ?? ""
      return NOTE.test(inner) ? "" : ` ${inner} `
    })
    .replace(/&/g, "and")
    .replace(/colour/g, "color")
    .replace(/[^a-z0-9]+/g, "")
}

/** Words that make a longer title another version of a song rather than the same one. */
const VERSION = /remix|mix|edit|version|live|acoustic|instrumental|demo|rework|vip|dub|extended/

/** Whether two title keys name the same song, allowing for a prefix or suffix on one of them. */
export function sameTitle(a: string, b: string): boolean {
  if (!a || !b) return false
  const [long, short] = a.length >= b.length ? [a, b] : [b, a]
  return long === short || (short.length >= 4 && long.includes(short) && !VERSION.test(long.replace(short, "")))
}

/** Folder names that say where music is kept, not who made it. */
const GENERIC_FOLDERS =
  /^(@@\w+|music|musik|musique|mp3s?|flacs?|lossless|albums?|downloads?|complete|shared?|soulseek|slsk|new|misc|various|va|library|media|audio|collection)$/i

/**
 * The artist a shared folder's parent names, unless it's just a storage folder like
 * "Music" or "FLAC", which would send lookups (lyrics, artwork) to the wrong place.
 */
export function artistFromFolder(parent: string | null | undefined): string | null {
  const name = parent?.trim()
  if (!name || GENERIC_FOLDERS.test(name)) return null
  return name
}

/** An album name from a folder: "2001 - Toxicity [FLAC]" → "Toxicity". */
export function albumFromFolder(folder: string): string {
  const cleaned = folder
    .replace(/^\(?\d{4}\)?\s*[-–.]\s*/, "")
    .replace(/\s*[[(](?:\d{4}|flac|mp3|320|v0|web|cd|vinyl|24[- ]?bit|16[- ]?bit|lossless)[^\])]*[\])]\s*/gi, " ")
    .trim()
  return cleaned || folder
}
