import { memo } from "react"

import { Cover } from "@/components/cover"
import type { Candidate } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { formatBytes, formatRuntime, formatSpeed, plural } from "@/lib/format"
import { describeQuality, TIER_BG, TIER_TEXT, tierOf } from "@/lib/quality"
import { cn } from "@/lib/utils"

type Props = {
  candidate: Candidate
  selected: boolean
  onOpen: () => void
  onHover: () => void
}

export const ResultRow = memo(function ResultRow({ candidate: c, selected, onOpen, onHover }: Props) {
  const tier = tierOf(c.quality)
  const speed = formatSpeed(c.avg_speed)
  const artwork = useArtwork(c.parent, c.title)

  return (
    <button
      type="button"
      onClick={onOpen}
      onMouseMove={onHover}
      data-selected={selected || undefined}
      className={cn(
        "group relative grid h-full w-full items-center gap-x-4 rounded-xl pr-5 pl-3 text-left outline-none",
        "grid-cols-[52px_minmax(0,1fr)_88px] md:grid-cols-[52px_112px_minmax(0,1fr)_84px_80px_84px_118px]",
        "transition-colors hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-ring data-selected:bg-accent",
      )}
    >
      <span
        className={cn("absolute top-4 bottom-4 left-0 w-[3px] rounded-r-full opacity-70 group-data-selected:opacity-100", TIER_BG[tier])}
        aria-hidden
      />

      <Cover
        src={artwork.data?.thumb}
        pending={artwork.isPending}
        alt=""
        className="size-[52px] rounded-lg shadow-[0_6px_16px_-8px_rgb(0_0_0/0.7)]"
      />

      <span className="order-last min-w-0 text-right md:order-none md:text-left">
        <span className={cn("block truncate text-[14.5px] font-semibold", TIER_TEXT[tier])}>{c.quality_label ?? "Unknown"}</span>
        <span className="block truncate text-[12.5px] text-muted-foreground">
          {c.mixed_quality ? "Mixed quality" : describeQuality(c.quality)}
        </span>
      </span>

      <span className="min-w-0">
        <span className="block truncate text-[15px] font-medium">{c.title}</span>
        <span className="flex min-w-0 gap-3 text-[13px] text-muted-foreground">
          {c.parent && <span className="truncate">{c.parent}</span>}
          <span className="hidden shrink-0 truncate text-muted-foreground/60 lg:inline">shared by {c.username}</span>
          <span className="shrink-0 md:hidden">{plural(c.audio_files, "track")}</span>
        </span>
      </span>

      <span className="hidden text-sm text-muted-foreground md:block">{plural(c.audio_files, "track")}</span>
      <span className="hidden text-sm text-muted-foreground md:block">{formatRuntime(c.duration_secs) ?? "—"}</span>
      <span className="hidden text-sm text-muted-foreground md:block">{formatBytes(c.total_bytes)}</span>
      <span className="hidden md:block">
        {c.free_slot ? (
          <span className="flex items-center gap-1.5 text-sm text-q-lossless">
            <span className="size-1.5 rounded-full bg-q-lossless" aria-hidden />
            Ready
          </span>
        ) : (
          <span className="text-sm text-muted-foreground">{plural(c.queue_length, "person", "people")} ahead</span>
        )}
        <span className="block text-[12.5px] text-muted-foreground/70">{speed ?? "Speed unknown"}</span>
      </span>
    </button>
  )
})
