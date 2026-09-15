/**
 * Pull a track number and a readable title out of a shared file name.
 *
 *   "Pink Floyd - The Dark Side of the Moon - 03 - Time.flac" → { number: 3, title: "Time" }
 *   "01. Speak to Me.flac"                                   → { number: 1, title: "Speak to Me" }
 *   "2-04 Lift.flac"                                         → { disc: 2, number: 4, title: "Lift" }
 */
export function parseTrackName(fileName: string): { disc?: number; number?: number; title: string; extension: string } {
  const dot = fileName.lastIndexOf(".")
  const extension = dot > 0 ? fileName.slice(dot + 1).toLowerCase() : ""
  let stem = dot > 0 ? fileName.slice(0, dot) : fileName
  stem = stem.replace(/_/g, " ").trim()

  // Split on " - " and look for the segment that is just a number.
  const parts = stem.split(/\s+-\s+/)
  const numberIndex = parts.findIndex((p) => /^\d{1,3}$/.test(p.trim()))
  if (numberIndex >= 0 && numberIndex < parts.length - 1) {
    return { number: Number(parts[numberIndex]), title: parts.slice(numberIndex + 1).join(" - "), extension }
  }

  const match = stem.match(/^(?:(\d)[-.])?(\d{1,3})(?:[\s.\-)]+)(.+)$/)
  if (match) {
    const title = match[3].replace(/^[-.\s]+/, "")
    return { disc: match[1] ? Number(match[1]) : undefined, number: Number(match[2]), title, extension }
  }

  return { title: parts.at(-1) ?? stem, extension }
}
