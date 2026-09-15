const SYNODIC_DAYS = 29.530588853
// A known new moon: 2000-01-06 18:14 UTC.
const EPOCH_MS = Date.UTC(2000, 0, 6, 18, 14)

export type MoonPhase = {
  /** Lit fraction of the disc, 0 (new) to 1 (full). */
  illumination: number
  waxing: boolean
  name: string
}

/** The moon's phase at `date`, accurate to within a few hours. */
export function moonPhase(date = new Date()): MoonPhase {
  const age = (((date.getTime() - EPOCH_MS) / 86_400_000) % SYNODIC_DAYS + SYNODIC_DAYS) % SYNODIC_DAYS
  const angle = (age / SYNODIC_DAYS) * 2 * Math.PI
  const illumination = (1 - Math.cos(angle)) / 2
  const waxing = age < SYNODIC_DAYS / 2
  return { illumination, waxing, name: phaseName(age) }
}

function phaseName(age: number): string {
  const names = [
    "New moon",
    "Waxing crescent",
    "First quarter",
    "Waxing gibbous",
    "Full moon",
    "Waning gibbous",
    "Last quarter",
    "Waning crescent",
  ]
  return names[Math.floor((age / SYNODIC_DAYS) * 8 + 0.5) % 8]
}
