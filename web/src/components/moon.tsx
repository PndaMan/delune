import { useId } from "react"

import { cn } from "@/lib/utils"

type MoonProps = {
  /** Lit fraction of the disc, 0 (new) to 1 (full). */
  illumination: number
  /** Waxing moons are lit on the right, waning on the left. */
  waxing?: boolean
  size?: number
  glow?: boolean
  className?: string
  /** Accessible description; omit for decorative moons. */
  label?: string
}

const R = 46
const C = 50

/**
 * The lit part of the disc. The terminator is half an ellipse whose horizontal
 * radius shrinks to zero at quarter moon and grows back to the full radius at new
 * and full moon; which side it bulges towards decides crescent or gibbous.
 */
function litPath(p: number): string {
  if (p <= 0.005) return ""
  if (p >= 0.995) return `M ${C} ${C - R} A ${R} ${R} 0 1 1 ${C} ${C + R} A ${R} ${R} 0 1 1 ${C} ${C - R} Z`
  const rx = R * Math.abs(1 - 2 * p)
  const sweep = p < 0.5 ? 0 : 1
  return `M ${C} ${C - R} A ${R} ${R} 0 0 1 ${C} ${C + R} A ${rx} ${R} 0 0 ${sweep} ${C} ${C - R} Z`
}

/** Craters as [cx, cy, r, opacity]. Hand-placed so the moon reads at any size. */
const CRATERS: [number, number, number, number][] = [
  [36, 34, 9, 0.09],
  [60, 26, 5, 0.07],
  [66, 52, 11, 0.08],
  [42, 64, 7, 0.08],
  [28, 52, 4, 0.06],
  [56, 74, 5, 0.06],
  [74, 36, 3, 0.05],
  [48, 44, 3, 0.05],
]

export function Moon({ illumination, waxing = true, size = 40, glow = false, className, label }: MoonProps) {
  const id = useId().replace(/:/g, "")
  const p = Math.min(1, Math.max(0, illumination))
  const lit = litPath(p)

  return (
    <svg
      viewBox="0 0 100 100"
      width={size}
      height={size}
      className={cn("shrink-0 overflow-visible", className)}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <defs>
        <radialGradient id={`${id}-surface`} cx="38%" cy="35%" r="75%">
          <stop offset="0%" style={{ stopColor: "var(--moon)", stopOpacity: 1 }} />
          <stop offset="70%" style={{ stopColor: "var(--moon)", stopOpacity: 0.92 }} />
          <stop offset="100%" style={{ stopColor: "var(--moon)", stopOpacity: 0.72 }} />
        </radialGradient>
        <clipPath id={`${id}-lit`}>
          <path d={lit} style={{ transition: "d 700ms cubic-bezier(.2,.7,.2,1)" }} />
        </clipPath>
        {glow && (
          <filter id={`${id}-glow`} x="-60%" y="-60%" width="220%" height="220%">
            <feGaussianBlur stdDeviation="9" />
          </filter>
        )}
      </defs>

      {glow && (
        <circle
          cx={C}
          cy={C}
          r={R * 1.25}
          filter={`url(#${id}-glow)`}
          style={{ fill: "var(--moon-glow)", opacity: 0.35 + p * 0.65, transition: "opacity 700ms" }}
        />
      )}

      <g transform={waxing ? undefined : `translate(${2 * C} 0) scale(-1 1)`}>
        {/* The dark side, faintly visible, so a new moon still has a shape. */}
        <circle cx={C} cy={C} r={R} style={{ fill: "var(--moon)", fillOpacity: 0.06, stroke: "var(--moon)", strokeOpacity: 0.16 }} strokeWidth={0.8} />
        <g clipPath={`url(#${id}-lit)`}>
          <circle cx={C} cy={C} r={R} fill={`url(#${id}-surface)`} />
          {CRATERS.map(([x, y, r, o]) => (
            <circle key={`${x}-${y}`} cx={x} cy={y} r={r} fill="black" fillOpacity={o} />
          ))}
        </g>
      </g>
    </svg>
  )
}
