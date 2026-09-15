import { Dialog } from "@base-ui/react/dialog"
import { Link } from "@tanstack/react-router"
import { ArrowDownToLine, Check, FileImage, File as FileIcon, LoaderCircle, TriangleAlert, X } from "lucide-react"
import { useLayoutEffect, useRef, useState } from "react"

import { Cover } from "@/components/cover"
import { Button } from "@/components/ui/button"
import type { Candidate, CandidateFile } from "@/lib/api"
import { useAccentColour, useArtwork } from "@/lib/artwork"
import { describeJob, jobForCandidate, useDownloads, useStartDownload } from "@/lib/downloads"
import { formatBytes, formatRuntime, formatSpeed, formatTrackTime } from "@/lib/format"
import { describeQuality, TIER_BG, TIER_TEXT, tierOf } from "@/lib/quality"
import { parseTrackName } from "@/lib/track-name"
import { cn } from "@/lib/utils"

type Props = {
  candidate: Candidate | null
  onClose: () => void
}

/**
 * A release, large: artwork and facts on the left, every track on the right. On
 * desktop it's laid out to fit the window without scrolling; long tracklists flow
 * into more columns instead.
 */
export function ReleaseModal({ candidate, onClose }: Props) {
  return (
    <Dialog.Root open={candidate !== null} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup
          className={cn(
            "fixed inset-0 z-50 m-auto flex h-[100dvh] w-full flex-col overflow-hidden bg-card outline-none",
            "sm:h-[min(90dvh,860px)] sm:w-[min(94vw,1280px)] sm:rounded-3xl sm:border sm:shadow-[0_40px_120px_-20px_rgb(0_0_0/0.8)]",
            "transition-[opacity,scale] duration-200 ease-out data-ending-style:scale-[0.97] data-ending-style:opacity-0 data-starting-style:scale-[0.97] data-starting-style:opacity-0",
          )}
        >
          {candidate && <ReleaseDetail candidate={candidate} />}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

const quietScroll = "overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"

function ReleaseDetail({ candidate: c }: { candidate: Candidate }) {
  const tier = tierOf(c.quality)
  const artwork = useArtwork(c.parent, c.title)
  const accent = useAccentColour(artwork.data?.thumb)
  const audio = c.files.filter((f) => f.audio)
  const other = c.files.filter((f) => !f.audio)

  return (
    <div className={cn("relative flex h-full min-h-0 flex-col lg:grid lg:grid-cols-[minmax(320px,36%)_minmax(0,1fr)]", quietScroll, "lg:overflow-hidden")}>
      <div
        className="pointer-events-none absolute inset-y-0 left-0 w-full lg:w-[36%]"
        style={{
          background: `radial-gradient(90% 55% at 30% 12%, ${accent ?? "var(--primary)"} 0%, transparent 70%)`,
          opacity: accent ? 0.3 : 0.12,
        }}
        aria-hidden
      />

      <Dialog.Close
        className="absolute top-4 right-4 z-10 flex size-10 items-center justify-center rounded-full bg-background/60 text-muted-foreground backdrop-blur transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        aria-label="Close"
      >
        <X className="size-5" />
      </Dialog.Close>

      <aside className="relative flex shrink-0 flex-col px-6 lg:min-h-0 lg:shrink pt-8 pb-6 sm:px-9 sm:pt-9 lg:border-r lg:pb-8">
        <div className="flex gap-5 lg:block">
          <Cover
            src={artwork.data?.cover}
            pending={artwork.isPending}
            alt={artwork.data ? `${artwork.data.album} cover` : ""}
            className="aspect-square w-24 shrink-0 rounded-xl shadow-[0_24px_60px_-24px_rgb(0_0_0/0.9)] sm:w-32 lg:w-[min(100%,34vh,360px)] lg:rounded-2xl"
          />
          <div className="min-w-0 pr-10 lg:mt-6 lg:pr-0">
            <p className={cn("flex flex-wrap items-center gap-x-2 text-[13.5px] font-semibold", TIER_TEXT[tier])}>
              <span className={cn("size-2 rounded-full", TIER_BG[tier])} aria-hidden />
              {c.quality_label ?? "Unknown quality"}
              <span className="font-normal text-muted-foreground">{describeQuality(c.quality)}</span>
            </p>
            <Dialog.Title className="type-display mt-2 line-clamp-2 text-[26px] text-balance break-words sm:text-[32px] lg:text-[clamp(26px,3.6vh,38px)]">
              {artwork.data?.album ?? c.title}
            </Dialog.Title>
            <Dialog.Description className="mt-1 truncate text-[17px] text-muted-foreground">
              {artwork.data?.artist ?? c.parent ?? "Unknown artist"}
            </Dialog.Description>
          </div>
        </div>

        <dl className="mt-6 grid grid-cols-3 gap-x-4 gap-y-3 text-sm">
          <Stat label="Tracks" value={String(c.audio_files)} />
          <Stat label="Length" value={formatRuntime(c.duration_secs) ?? "—"} />
          <Stat label="Size" value={formatBytes(c.total_bytes)} />
          <Stat
            label="Availability"
            value={c.free_slot ? "Ready" : `${c.queue_length} ahead`}
            tone={c.free_slot ? "good" : undefined}
          />
          <Stat label="Speed" value={formatSpeed(c.avg_speed) ?? "—"} />
          <Stat label="From" value={c.username} />
        </dl>

        {c.mixed_quality && (
          <p className="mt-4 flex gap-2 text-[13px] text-q-hires">
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
            Not every file is the same quality; the label shows the lowest.
          </p>
        )}

        <div className="hidden lg:mt-auto lg:block lg:pt-6">
          <DownloadAction candidate={c} />
        </div>
      </aside>

      <section className="relative flex shrink-0 flex-col px-3 lg:min-h-0 lg:shrink pt-2 pb-5 sm:px-6 lg:pt-[72px]">
        <Tracklist files={audio} />
        {other.length > 0 && (
          <ul className="mx-3 mt-3 flex flex-wrap gap-2 border-t pt-4">
            {other.map((f) => (
              <li key={f.path} className="flex items-center gap-1.5 rounded-lg bg-muted/50 px-2.5 py-1 text-[12.5px] text-muted-foreground" title={f.path}>
                {/\.(jpe?g|png|webp|gif)$/i.test(f.name) ? <FileImage className="size-3.5" /> : <FileIcon className="size-3.5" />}
                {f.name}
                <span className="text-muted-foreground/60">{formatBytes(f.size)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Phones: the one action stays in reach while the tracklist scrolls. */}
      <div className="sticky bottom-0 z-10 mt-auto border-t bg-card/90 px-5 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] backdrop-blur-md lg:hidden">
        <DownloadAction candidate={c} />
      </div>
    </div>
  )
}

const ROW_HEIGHT = 38
const MIN_COLUMN_WIDTH = 330

/**
 * Tracks flow down and then across into as many columns as fit, so a 30-track
 * album shows at once instead of scrolling. Only beyond what fits in the widest
 * layout does it scroll, quietly, with a fade at the bottom.
 */
function Tracklist({ files }: { files: CandidateFile[] }) {
  const box = useRef<HTMLDivElement>(null)
  const [layout, setLayout] = useState({ columns: 1, rows: files.length, overflow: false })

  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    const measure = () => {
      const wide = window.matchMedia("(min-width: 1024px)").matches
      if (!wide) return setLayout({ columns: 1, rows: files.length, overflow: false })
      const rowsThatFit = Math.max(4, Math.floor(el.clientHeight / ROW_HEIGHT))
      const maxColumns = Math.max(1, Math.min(3, Math.floor(el.clientWidth / MIN_COLUMN_WIDTH)))
      const columns = Math.min(maxColumns, Math.max(1, Math.ceil(files.length / rowsThatFit)))
      const rows = Math.ceil(files.length / columns)
      setLayout({ columns, rows, overflow: rows > rowsThatFit })
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    return () => observer.disconnect()
  }, [files.length])

  const compact = layout.columns > 1

  return (
    <div
      ref={box}
      className={cn(
        "relative lg:min-h-0 lg:flex-1",
        layout.overflow ? cn(quietScroll, "[mask-image:linear-gradient(to_bottom,black_90%,transparent)]") : "lg:overflow-hidden",
      )}
    >
      <ol
        className="grid gap-x-4"
        style={{
          gridAutoFlow: "column",
          gridTemplateColumns: `repeat(${layout.columns}, minmax(0, 1fr))`,
          gridTemplateRows: `repeat(${layout.rows}, ${ROW_HEIGHT}px)`,
        }}
      >
        {files.map((file, i) => {
          const { position, title, extension } = parseTrackName(file.name)
          return (
            <li
              key={file.path}
              className="grid grid-cols-[30px_minmax(0,1fr)_auto] items-center gap-x-3 rounded-lg px-3 transition-colors hover:bg-accent/50"
              title={file.name}
            >
              <span className="text-right text-[13px] text-muted-foreground/70">{position ?? i + 1}</span>
              <span className="truncate text-[14.5px]">{title}</span>
              <span className="flex items-center gap-3 text-[12.5px] text-muted-foreground">
                {!compact && (
                  <span className={cn("hidden sm:inline", TIER_TEXT[tierOf(file.quality)])}>
                    {file.quality_label ?? extension.toUpperCase()}
                  </span>
                )}
                <span className="w-9 text-right">{formatTrackTime(file.duration_secs)}</span>
                {!compact && <span className="hidden w-14 text-right sm:inline">{formatBytes(file.size)}</span>}
              </span>
            </li>
          )
        })}
      </ol>
    </div>
  )
}

function DownloadAction({ candidate }: { candidate: Candidate }) {
  const start = useStartDownload()
  const downloads = useDownloads()
  const job = jobForCandidate(downloads.data ?? [], candidate)

  if (job) {
    const progress = job.total_bytes ? Math.round((job.bytes / job.total_bytes) * 100) : 0
    const done = job.status === "ready" || job.status === "imported"
    return (
      <div className="rounded-xl border bg-background/40 px-4 py-3">
        <div className="flex items-center gap-3">
          {done ? <Check className="size-4 text-q-lossless" /> : <LoaderCircle className="size-4 animate-spin text-primary" />}
          <p className="min-w-0 flex-1 truncate text-[14px]">{describeJob(job)}</p>
          <Button variant="outline" size="sm" nativeButton={false} render={<Link to={job.status === "ready" ? "/review" : "/downloads"} />}>
            {job.status === "ready" ? "Review" : job.status === "imported" ? "Done" : "Progress"}
          </Button>
        </div>
        {!done && (
          <div className="mt-2.5 h-1 overflow-hidden rounded-full bg-muted">
            <div className="h-full bg-primary transition-[width] duration-500" style={{ width: `${Math.max(progress, 2)}%` }} />
          </div>
        )}
      </div>
    )
  }

  return (
    <div>
      <Button
        size="lg"
        className="h-12 w-full rounded-xl text-[15px] font-semibold"
        disabled={start.isPending}
        onClick={() => start.mutate(candidate)}
      >
        {start.isPending ? <LoaderCircle className="animate-spin" /> : <ArrowDownToLine />}
        Download for review
      </Button>
      <p className="mt-2 hidden text-center text-[12.5px] text-muted-foreground sm:block">
        {start.isError ? start.error.message : "Nothing reaches your library until you approve it."}
      </p>
    </div>
  )
}

function Stat({ label, value, tone }: { label: string; value: string; tone?: "good" }) {
  return (
    <div className="min-w-0">
      <dt className="text-[12px] text-muted-foreground">{label}</dt>
      <dd className={cn("truncate text-[14.5px]", tone === "good" && "text-q-lossless")} title={value}>
        {value}
      </dd>
    </div>
  )
}
