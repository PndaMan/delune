import { memo } from "react"

import { Cover } from "@/components/cover"
import type { Candidate, DownloadJob } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { jobForAlbum, jobForCandidate, useDownloads } from "@/lib/downloads"
import { coverStatus, type Ownership, ownership, useLibraryAlbum } from "@/lib/library"
import { formatBytes, formatRuntime, formatSpeed, plural } from "@/lib/format"
import { describeQuality, TIER_BG, TIER_TEXT, tierOf } from "@/lib/quality"
import { type LinkMatch, matchLink, useResolved } from "@/lib/tracklist"
import { cn } from "@/lib/utils"

type Props = {
  candidate: Candidate
  selected: boolean
  onOpen: () => void
  onHover: () => void
}

/** Row height used by the virtualised list, per layout. */
export const ROW_HEIGHT = { desktop: 76, mobile: 96 }

export const ResultRow = memo(function ResultRow({ candidate: c, selected, onOpen, onHover }: Props) {
  const tier = tierOf(c.quality)
  const speed = formatSpeed(c.avg_speed)
  const artwork = useArtwork(c.parent, c.title)
  const downloads = useDownloads()
  const jobs = downloads.data ?? []
  const exact = jobForCandidate(jobs, c)
  const sameAlbum = jobForAlbum(jobs, c)
  const library = useLibraryAlbum(c.parent, c.title)
  const owned = ownership(c, library.data)
  const status = coverStatus(exact ?? sameAlbum, owned)
  const linkMatch = matchLink(c, useResolved())
  // A download in flight says the most; after that, what the library holds beats job history.
  const job = exact ?? sameAlbum
  const badge =
    job && !(owned && job.status === "imported") ? <JobBadge exact={exact} sameAlbum={sameAlbum} /> : <LibraryBadge owned={owned} />

  return (
    <button
      type="button"
      onClick={onOpen}
      onMouseMove={onHover}
      data-selected={selected || undefined}
      className={cn(
        "group relative h-full w-full rounded-xl text-left outline-none transition-colors",
        "hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-ring data-selected:bg-accent",
      )}
    >
      <span
        className={cn("absolute top-4 bottom-4 left-0 w-[3px] rounded-r-full opacity-70 group-data-selected:opacity-100", TIER_BG[tier])}
        aria-hidden
      />

      {/* Phones: an album card. One quality line for the whole folder. */}
      <span className="flex h-full items-center gap-3.5 pr-3 pl-3.5 md:hidden">
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} status={status} alt="" className="size-[68px] rounded-xl" />
        <span className="min-w-0 flex-1">
          <span className="line-clamp-2 text-[15px] leading-snug font-medium">{c.title}</span>
          <span className="block truncate text-[13px] text-muted-foreground">{c.parent ?? c.username}</span>
          <span className="mt-1 flex items-center gap-2.5 text-[12.5px] whitespace-nowrap">
            <span className={cn("font-semibold", TIER_TEXT[tier])}>{c.quality_label ?? "Unknown"}</span>
            <span className="text-muted-foreground">
              <MatchSummary match={linkMatch} fallback={plural(c.audio_files, "track")} />
            </span>
            <span className="ml-auto min-w-0 truncate">
              {exact || sameAlbum || owned ? (
                badge
              ) : c.free_slot ? (
                <span className="text-q-lossless">Ready</span>
              ) : (
                <span className="text-muted-foreground">Queued</span>
              )}
            </span>
          </span>
        </span>
      </span>

      {/* Desktop: a dense row with columns. */}
      <span className="hidden h-full grid-cols-[52px_112px_minmax(0,1fr)_84px_80px_84px_118px] items-center gap-x-4 pr-5 pl-3 md:grid">
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} status={status} alt="" className="size-[52px] rounded-lg shadow-[0_6px_16px_-8px_rgb(0_0_0/0.7)]" />
        <span className="min-w-0">
          <span className={cn("block truncate text-[14.5px] font-semibold", TIER_TEXT[tier])}>{c.quality_label ?? "Unknown"}</span>
          <span className="block truncate text-[12.5px] text-muted-foreground">
            {c.mixed_quality ? "Mixed quality" : describeQuality(c.quality)}
          </span>
        </span>
        <span className="min-w-0">
          <span className="flex min-w-0 items-center gap-2">
            <span className="truncate text-[15px] font-medium">{c.title}</span>
            {badge}
          </span>
          <span className="flex min-w-0 gap-3 text-[13px] text-muted-foreground">
            {c.parent && <span className="truncate">{c.parent}</span>}
            <span className="hidden shrink-0 truncate text-muted-foreground/60 lg:inline">shared by {c.username}</span>
          </span>
        </span>
        <span className="text-sm text-muted-foreground">
          <MatchSummary match={linkMatch} fallback={plural(c.audio_files, "track")} />
        </span>
        <span className="text-sm text-muted-foreground">{formatRuntime(c.duration_secs) ?? "—"}</span>
        <span className="text-sm text-muted-foreground">{formatBytes(c.total_bytes)}</span>
        <span>
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
      </span>
    </button>
  )
})

/** Tracks, or for a pasted link, how well this folder matches it. */
function MatchSummary({ match, fallback }: { match: LinkMatch | null; fallback: string }) {
  if (!match) return <>{fallback}</>
  if (match.kind === "track") {
    return match.file ? <span className="whitespace-nowrap text-q-lossless">Has track</span> : <span className="text-muted-foreground/60">{fallback}</span>
  }
  if (match.matched === match.total) return <span className="whitespace-nowrap text-q-lossless" title="Every track on the linked release is here">{match.total} tracks</span>
  return (
    <span className={match.matched === 0 ? "text-muted-foreground/60" : undefined}>
      {match.matched} of {match.total}
    </span>
  )
}

function LibraryBadge({ owned }: { owned: Ownership | null }) {
  if (!owned) return null
  const title = owned.complete
    ? "Every track here is already in your library"
    : `Your library has ${owned.owned} of these tracks. Missing: ${[...owned.missing].slice(0, 6).join(", ")}${owned.missing.size > 6 ? "…" : ""}`
  return (
    <span
      title={title}
      className={cn(
        "shrink-0 rounded-full px-2 py-0.5 text-[11.5px] font-medium whitespace-nowrap",
        owned.complete ? "bg-q-lossless/15 text-q-lossless" : "bg-q-hires/15 text-q-hires",
      )}
    >
      {owned.complete ? "In library" : `${owned.missing.size} missing`}
    </span>
  )
}

function JobBadge({ exact, sameAlbum }: { exact?: DownloadJob; sameAlbum?: DownloadJob }) {
  const job = exact ?? sameAlbum
  if (!job) return null
  const pct = job.total_bytes ? Math.round((job.bytes / job.total_bytes) * 100) : 0
  const label = !exact
    ? job.status === "imported"
      ? "Imported"
      : "Another copy"
    : job.status === "imported"
      ? "Imported"
      : job.status === "ready"
        ? "In review"
        : job.status === "failed"
          ? "Failed"
          : `${pct}%`
  const title = !exact ? "You're already downloading a copy of this album from someone else" : undefined
  const tone =
    job.status === "imported" || job.status === "ready"
      ? "bg-q-lossless/15 text-q-lossless"
      : job.status === "failed"
        ? "bg-destructive/15 text-destructive"
        : "bg-primary/15 text-primary"
  return (
    <span title={title} className={cn("shrink-0 rounded-full px-2 py-0.5 text-[11.5px] font-medium whitespace-nowrap", tone)}>
      {exact && job.status !== "imported" && job.status !== "ready" && job.status !== "failed" ? (
        <span className="hidden md:inline">Downloading </span>
      ) : null}
      {label}
    </span>
  )
}
