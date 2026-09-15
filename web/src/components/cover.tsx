import { Check, Disc3 } from "lucide-react"
import { useState } from "react"

import { cn } from "@/lib/utils"

type Props = {
  src: string | null | undefined
  alt: string
  className?: string
  /** Loading the artwork lookup itself (not the image). */
  pending?: boolean
  /**
   * Marks the art: greyed with a progress ring while a copy is downloading, tinted
   * green with a tick once the album is in the library.
   */
  status?: CoverStatus
}

export type CoverStatus = { kind: "downloading"; progress: number } | { kind: "in-library"; partial?: boolean }

/** Album art with a quiet placeholder while loading or when there's no match. */
export function Cover({ src, alt, className, pending = false, status }: Props) {
  const [loaded, setLoaded] = useState<string | null>(null)
  const ready = !!src && loaded === src

  return (
    <span className={cn("relative block shrink-0 overflow-hidden bg-muted", className)}>
      {!ready && (
        <span
          className={cn(
            "absolute inset-0 flex items-center justify-center bg-gradient-to-br from-accent to-muted text-muted-foreground/35",
            pending && "animate-pulse",
          )}
          aria-hidden
        >
          <Disc3 className="size-[38%]" strokeWidth={1.5} />
        </span>
      )}
      {src && (
        <img
          src={src}
          alt={alt}
          loading="lazy"
          decoding="async"
          onLoad={() => setLoaded(src)}
          className={cn(
            "absolute inset-0 size-full object-cover transition-[opacity,filter] duration-300",
            ready ? "opacity-100" : "opacity-0",
            status?.kind === "downloading" && "brightness-75 grayscale",
          )}
        />
      )}
      {status?.kind === "downloading" && (
        <span className="absolute inset-0 flex items-center justify-center bg-primary/25" aria-hidden>
          <svg viewBox="0 0 36 36" className="size-[58%] -rotate-90 drop-shadow-[0_1px_3px_rgb(0_0_0/0.6)]">
            <circle cx="18" cy="18" r="15" fill="none" strokeWidth="3.5" className="stroke-white/25" />
            <circle
              cx="18"
              cy="18"
              r="15"
              fill="none"
              strokeWidth="3.5"
              strokeLinecap="round"
              pathLength={100}
              strokeDasharray={`${Math.max(2, Math.min(100, status.progress * 100))} 100`}
              className="stroke-white transition-[stroke-dasharray] duration-500"
            />
          </svg>
        </span>
      )}
      {status?.kind === "in-library" && (
        <span
          className={cn(
            "absolute inset-0 flex items-end justify-end p-[7%]",
            status.partial ? "bg-gradient-to-tl from-q-hires/55 via-transparent" : "bg-q-lossless/30",
          )}
          aria-hidden
        >
          <span
            className={cn(
              "flex size-[38%] max-h-6 max-w-6 min-h-4 min-w-4 items-center justify-center rounded-full text-background shadow-[0_1px_4px_rgb(0_0_0/0.5)]",
              status.partial ? "bg-q-hires" : "bg-q-lossless",
            )}
          >
            <Check className="size-[65%]" strokeWidth={3} />
          </span>
        </span>
      )}
    </span>
  )
}
