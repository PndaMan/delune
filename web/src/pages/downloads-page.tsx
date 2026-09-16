import { Link } from "@tanstack/react-router"
import {
  ArrowUpToLine,
  ChevronDown,
  CircleAlert,
  CircleCheck,
  LoaderCircle,
  Pause,
  Play,
  Search,
  Trash2,
} from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { useMusicViews } from "@/components/music-views"
import { RequestsSection } from "@/components/requests-section"
import { WishlistSection } from "@/components/wishlist-section"
import { Button } from "@/components/ui/button"
import type { DownloadJob, JobFile } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { describeJob, useDownloads, useRemoveDownload, useToggleDownload } from "@/lib/downloads"
import { requesterLabel, useMe } from "@/lib/session"
import { formatBytes } from "@/lib/format"
import { parseTrackName } from "@/lib/track-name"
import { cn } from "@/lib/utils"
import { PageFrame } from "@/pages/placeholder-pages"

export function DownloadsPage() {
  const downloads = useDownloads()
  // Imported albums are done; they're listed under Review's "Recently imported".
  const jobs = (downloads.data ?? []).filter((job) => job.status !== "imported")

  return (
    <PageFrame title="Downloads" wide>
      <Link
        to="/history"
        className="text-[14px] text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
      >
        See everything you've downloaded and requested
      </Link>
      <RequestsSection />
      {downloads.isPending ? (
        <div className="mt-8 space-y-3">
          {[0, 1].map((i) => (
            <div key={i} className="h-28 animate-pulse rounded-2xl bg-muted/40" />
          ))}
        </div>
      ) : jobs.length === 0 ? (
        <EmptyState
          illumination={0.25}
          title="Nothing downloading"
          action={
            <Button nativeButton={false} render={<Link to="/" search={{}} />}>
              Find an album
            </Button>
          }
        >
          Open a release from search and choose Download for review. Its files arrive here and move to Review once
          they're all in.
        </EmptyState>
      ) : (
        <ul className="mt-6 space-y-3">
          {jobs.map((job) => (
            <JobCard key={job.id} job={job} />
          ))}
        </ul>
      )}
      <WishlistSection />
    </PageFrame>
  )
}

export function JobCard({ job }: { job: DownloadJob }) {
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const artwork = useArtwork(job.parent, job.title)
  const views = useMusicViews()
  const remove = useRemoveDownload()
  const toggle = useToggleDownload()
  const stopped = job.status === "failed" || job.status === "cancelled"
  const progress = job.total_bytes ? job.bytes / job.total_bytes : 0
  const running = job.status === "queued" || job.status === "downloading"
  const requester = requesterLabel(useMe(), job.requested_by)

  return (
    <li className="overflow-hidden rounded-2xl border bg-card/60">
      <div className="flex items-center gap-5 p-4 sm:p-5">
        <button
          type="button"
          onClick={() =>
            views.openAlbum({ artist: artwork.data?.artist ?? job.parent, title: artwork.data?.album ?? job.title })
          }
          aria-label={`About ${artwork.data?.album ?? job.title}`}
          className="rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Cover
            src={artwork.data?.thumb}
            pending={artwork.isPending}
            alt=""
            className="size-16 rounded-xl sm:size-20"
          />
        </button>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[16px] font-semibold">{artwork.data?.album ?? job.title}</p>
          <p className="truncate text-sm text-muted-foreground">
            {artwork.data?.artist ?? job.parent}
            {requester && <span className="text-muted-foreground/70">, for {requester}</span>}
            <span className="sm:hidden">
              {running || job.status === "failed" ? `, ${Math.round(progress * 100)}%` : ""}
            </span>
          </p>
          <p
            className={cn(
              "mt-2 flex items-center gap-2 text-[13.5px]",
              job.status === "ready" && "text-q-lossless",
              job.status === "failed" && "text-destructive",
              running && "text-muted-foreground",
            )}
          >
            {job.status === "ready" ? (
              <CircleCheck className="size-4 shrink-0" />
            ) : job.status === "failed" ? (
              <CircleAlert className="size-4 shrink-0" />
            ) : running ? (
              <LoaderCircle className="size-4 shrink-0 animate-spin" />
            ) : null}
            <span className="line-clamp-2 sm:truncate">{describeJob(job)}</span>
          </p>
        </div>
        <div className="hidden text-right text-sm text-muted-foreground sm:block">
          <p className="text-foreground">{Math.round(progress * 100)}%</p>
          <p>
            {formatBytes(job.bytes)} of {formatBytes(job.total_bytes)}
          </p>
        </div>
        <div className="flex items-center gap-1">
          {stopped && (
            <Button
              variant="ghost"
              size="icon"
              className="hidden sm:inline-flex"
              nativeButton={false}
              render={<Link to="/" search={{ q: [job.parent, job.title].filter(Boolean).join(" ") }} />}
              aria-label="Find another copy"
              title="Find another copy"
            >
              <Search />
            </Button>
          )}
          {job.waiting_for_slot !== null && job.waiting_for_slot > 1 && (
            <Button
              variant="ghost"
              size="icon"
              onClick={() => toggle.mutate({ id: job.id, action: "prioritise" })}
              disabled={toggle.isPending}
              aria-label="Start next"
              title="Start next"
            >
              <ArrowUpToLine />
            </Button>
          )}
          {(running || stopped) && (
            <Button
              variant="ghost"
              size="icon"
              onClick={() => toggle.mutate({ id: job.id, action: running ? "stop" : "resume" })}
              disabled={toggle.isPending}
              aria-label={running ? "Stop download" : "Resume download"}
              title={running ? "Stop, keeping what's arrived" : "Resume"}
            >
              {running ? <Pause /> : <Play />}
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            className="hidden sm:inline-flex"
            onClick={() => setOpen(!open)}
            aria-expanded={open}
            aria-label={open ? "Hide files" : "Show files"}
          >
            <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setConfirming(true)}
            disabled={remove.isPending}
            aria-label={running ? "Cancel download" : "Remove download and its files"}
            title={running ? "Cancel download" : "Remove download and its files"}
          >
            <Trash2 />
          </Button>
        </div>
      </div>

      {confirming && (
        <div className="flex flex-wrap items-center gap-3 border-t bg-destructive/8 px-4 py-3 sm:px-5">
          <p className="min-w-0 flex-1 text-[14px]">
            {running
              ? "Cancel this download and delete what has arrived?"
              : "Remove this download and delete its files?"}
          </p>
          <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
            Keep it
          </Button>
          <Button
            variant="destructive"
            size="sm"
            disabled={remove.isPending}
            onClick={() => remove.mutate(job.id, { onSettled: () => setConfirming(false) })}
          >
            {running ? "Cancel and delete" : "Remove"}
          </Button>
        </div>
      )}

      <div className="h-1 bg-muted/60" aria-hidden>
        <div
          className={cn(
            "h-full transition-[width] duration-500",
            job.status === "failed" ? "bg-destructive" : job.status === "ready" ? "bg-q-lossless" : "bg-primary",
          )}
          style={{ width: `${Math.max(progress * 100, running ? 1.5 : 0)}%` }}
        />
      </div>

      {open && (
        <ol className="px-3 py-3 sm:px-5">
          {job.files.map((file) => (
            <FileRow key={file.path} file={file} />
          ))}
        </ol>
      )}
    </li>
  )
}

const FILE_STATUS: Record<JobFile["status"], string> = {
  waiting: "Waiting",
  connecting: "Connecting",
  queued: "In queue",
  starting: "Starting",
  transferring: "Downloading",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
}

function FileRow({ file }: { file: JobFile }) {
  const { title, position } = parseTrackName(file.name)
  const pct = file.size ? Math.round((file.bytes / file.size) * 100) : 0
  return (
    <li className="grid grid-cols-[28px_minmax(0,1fr)_auto] items-center gap-x-4 rounded-lg px-2 py-2 text-[14px] sm:grid-cols-[28px_minmax(0,1fr)_110px_80px]">
      <span className="text-right text-muted-foreground/70">{position ?? ""}</span>
      <span className="min-w-0">
        <span className="block truncate">{title}</span>
        {file.error && <span className="block truncate text-[12.5px] text-destructive">{file.error}</span>}
      </span>
      <span
        className={cn(
          "text-right text-[13px] sm:text-left",
          file.status === "done"
            ? "text-q-lossless"
            : file.status === "failed"
              ? "text-destructive"
              : "text-muted-foreground",
        )}
      >
        {file.status === "transferring" ? `${pct}%` : FILE_STATUS[file.status]}
        {file.status === "queued" && file.place_in_queue ? `, #${file.place_in_queue}` : ""}
      </span>
      <span className="hidden text-right text-[13px] text-muted-foreground sm:block">{formatBytes(file.size)}</span>
    </li>
  )
}
