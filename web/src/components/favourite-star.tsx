import { Star } from "lucide-react"
import { useState } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useFavourites, useSetFavourite } from "@/lib/favourites"
import { cn } from "@/lib/utils"

const SPARKS = [0, 60, 120, 180, 240, 300]

/**
 * Star a Soulseek user. Starring them keeps their shares saved in delune, so the
 * next visit opens instantly instead of waiting on their connection. The star pops,
 * throws a few sparks and turns gold the moment it's pressed.
 */
export function FavouriteStar({ username, className }: { username: string; className?: string }) {
  const favourites = useFavourites()
  const setFavourite = useSetFavourite()
  const [burst, setBurst] = useState(0)
  const starred = (favourites.data ?? []).some((f) => f.username.toLowerCase() === username.toLowerCase())

  const toggle = () => {
    setFavourite.mutate({ username, on: !starred })
    if (!starred) setBurst((n) => n + 1)
  }

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            onClick={toggle}
            aria-pressed={starred}
            aria-label={starred ? `Remove ${username} from favourites` : `Add ${username} to favourites`}
            className={cn(
              "relative flex size-11 shrink-0 items-center justify-center rounded-full border outline-none transition-colors",
              "focus-visible:ring-2 focus-visible:ring-ring",
              starred
                ? "border-yellow-400/40 bg-yellow-400/10 text-yellow-400 hover:bg-yellow-400/15"
                : "border-border bg-card/60 text-muted-foreground hover:border-yellow-400/40 hover:text-yellow-400",
              className,
            )}
          />
        }
      >
        {/* Keyed on each starring, so the animation plays every time rather than once. */}
        <span key={burst} className="pointer-events-none absolute inset-0" aria-hidden>
          {burst > 0 &&
            SPARKS.map((angle) => (
              <span
                key={angle}
                className="absolute top-1/2 left-1/2 size-1.5 animate-[star-spark_0.6s_ease-out_forwards] rounded-full bg-yellow-400 opacity-0"
                style={{ "--angle": `${angle}deg` } as React.CSSProperties}
              />
            ))}
        </span>
        <Star
          key={`star-${burst}`}
          className={cn(
            "size-5 transition-[fill] duration-200",
            starred && "fill-yellow-400",
            burst > 0 && starred && "animate-[star-pop_0.45s_cubic-bezier(0.3,1.6,0.5,1)]",
          )}
          strokeWidth={1.8}
        />
      </TooltipTrigger>
      <TooltipContent>
        {starred
          ? "A favourite: their shares are kept, so they open instantly"
          : "Favourite: keep their shares to hand"}
      </TooltipContent>
    </Tooltip>
  )
}
