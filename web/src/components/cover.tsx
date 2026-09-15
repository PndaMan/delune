import { Disc3 } from "lucide-react"
import { useState } from "react"

import { cn } from "@/lib/utils"

type Props = {
  src: string | null | undefined
  alt: string
  className?: string
  /** Loading the artwork lookup itself (not the image). */
  pending?: boolean
}

/** Album art with a quiet placeholder while loading or when there's no match. */
export function Cover({ src, alt, className, pending = false }: Props) {
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
          className={cn("absolute inset-0 size-full object-cover transition-opacity duration-300", ready ? "opacity-100" : "opacity-0")}
        />
      )}
    </span>
  )
}
