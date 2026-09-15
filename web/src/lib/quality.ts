import type { Candidate, Quality } from "@/lib/api"

export type Tier = "hires" | "lossless" | "lossy" | "unknown"

export const TIERS: { id: Tier; label: string }[] = [
  { id: "hires", label: "Hi-res" },
  { id: "lossless", label: "Lossless" },
  { id: "lossy", label: "Lossy" },
]

const LOSSLESS = new Set(["flac", "alac", "wav", "aiff"])

export function tierOf(quality: Quality | null): Tier {
  if (!quality) return "unknown"
  if (!LOSSLESS.has(quality.codec)) return "lossy"
  if ((quality.bit_depth ?? 16) > 16 || (quality.sample_rate ?? 44_100) > 48_000) return "hires"
  return "lossless"
}

/** Tailwind classes for text and the row edge, per tier. */
export const TIER_TEXT: Record<Tier, string> = {
  hires: "text-q-hires",
  lossless: "text-q-lossless",
  lossy: "text-q-lossy",
  unknown: "text-q-unknown",
}

export const TIER_BG: Record<Tier, string> = {
  hires: "bg-q-hires",
  lossless: "bg-q-lossless",
  lossy: "bg-q-lossy",
  unknown: "bg-q-unknown",
}

/** A plain-language description, e.g. "24 bit, 96 kHz" or "320 kbps". */
export function describeQuality(quality: Quality | null): string {
  if (!quality) return "Quality not reported"
  const parts: string[] = []
  if (LOSSLESS.has(quality.codec)) {
    if (quality.bit_depth) parts.push(`${quality.bit_depth} bit`)
    if (quality.sample_rate) parts.push(`${formatKhz(quality.sample_rate)} kHz`)
    return parts.length ? parts.join(", ") : "Lossless"
  }
  if (quality.bitrate_kbps) parts.push(`${quality.bitrate_kbps} kbps`)
  if (quality.vbr) parts.push("variable")
  return parts.length ? parts.join(", ") : "Bitrate not reported"
}

function formatKhz(hz: number) {
  return hz % 1000 === 0 ? String(hz / 1000) : (hz / 1000).toFixed(1)
}

export type SortKey = "quality" | "speed" | "tracks"

type Compare = (a: Candidate, b: Candidate) => number

/** Median audio file count across results: what a full release looks like in this search. */
export function typicalTracks(results: Candidate[]): number {
  if (!results.length) return 1
  const counts = results.map((c) => c.audio_files).sort((a, b) => a - b)
  return Math.max(1, counts[Math.floor(counts.length / 2)])
}

function looksComplete(c: Candidate, typical: number) {
  return c.audio_files >= Math.min(typical, Math.max(1, Math.floor((typical * 6) / 10)))
}

const isLossless = (c: Candidate) => tierOf(c.quality) === "hires" || tierOf(c.quality) === "lossless"

/**
 * Mirrors `Candidate::compare_in` in delune-core: lossless first, then complete
 * folders over fragments, then resolution, consistency and availability.
 */
export function compareBest(typical: number): Compare {
  return (a, b) =>
    Number(isLossless(b)) - Number(isLossless(a)) ||
    Number(looksComplete(b, typical)) - Number(looksComplete(a, typical)) ||
    b.quality_rank - a.quality_rank ||
    Number(a.mixed_quality) - Number(b.mixed_quality) ||
    Number(b.free_slot) - Number(a.free_slot) ||
    a.queue_length - b.queue_length ||
    b.avg_speed - a.avg_speed ||
    a.id.localeCompare(b.id)
}

export const SORTS: Record<SortKey, { label: string; compare: (typical: number) => Compare }> = {
  quality: { label: "Best quality", compare: compareBest },
  speed: {
    label: "Fastest",
    compare: (typical) => (a, b) =>
      Number(b.free_slot) - Number(a.free_slot) ||
      a.queue_length - b.queue_length ||
      b.avg_speed - a.avg_speed ||
      compareBest(typical)(a, b),
  },
  tracks: { label: "Most tracks", compare: (typical) => (a, b) => b.audio_files - a.audio_files || compareBest(typical)(a, b) },
}
