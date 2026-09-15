import { Link } from "@tanstack/react-router"
import { ChevronDown, CircleAlert, CircleCheck, LoaderCircle, Trash2 } from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import type { DownloadJob, JobFile } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { describeJob, useDownloads, useRemoveDownload } from "@/lib/downloads"
import { formatBytes } from "@/lib/format"
import { parseTrackName } from "@/lib/track-name"
import { cn } from "@/lib/utils"
import { PageFrame } from "@/pages/placeholder-pages"

export function DownloadsPage() {
  const downloads = useDownloads()
  const jobs = downloads.data ?? []

  return (
    <PageFrame title="Downloads" wide>
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
          action={<Button nativeButton={false} render={<Link to="/" search={{}} />}>Find an album</Button>}
        >
          Open a release from search and choose Download for review. Its files arrive here one at a time and move to
          Review once they're all in.
        </EmptyState>
      ) : (
        <ul className="mt-6 space-y-3 pb-24">
          {jobs.map((job) => (
            <JobCard key={job.id} job={job} />
          ))}
        </ul>
      )}
    </PageFrame>
  )
}

export function JobCard({ job }: { job: DownloadJob }) {
  const [open, setOpen] = useState(false)
  const artwork = useArtwork(job.parent, job.title)
  const remove = useRemoveDownload()
  const progress = job.total_bytes ? job.bytes / job.total_bytes : 0
  const running = job.status === "queued" || job.status === "downloading"

  return (
    <li className="overflow-hidden rounded-2xl border bg-card/60">
      <div className="flex items-center gap-5 p-4 sm:p-5">
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} alt="" className="size-16 rounded-xl sm:size-20" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[16px] font-semibold">{artwork.data?.album ?? job.title}</p>
          <p className="truncate text-sm text-muted-foreground">
            {artwork.data?.artist ?? job.parent}
            <span className="sm:hidden">{running || job.status === "failed" ? `, ${Math.round(progress * 100)}%` : ""}</span>
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
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setOpen(!open)}
            aria-expanded={open}
            aria-label={open ? "Hide files" : "Show files"}
          >
            <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => remove.mutate(job.id)}
            disabled={remove.isPending}
            aria-label={running ? "Cancel download" : "Remove download and its files"}
            title={running ? "Cancel download" : "Remove download and its files"}
          >
            <Trash2 />
          </Button>
        </div>
      </div>

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
          file.status === "done" ? "text-q-lossless" : file.status === "failed" ? "text-destructive" : "text-muted-foreground",
        )}
      >
        {file.status === "transferring" ? `${pct}%` : FILE_STATUS[file.status]}
        {file.status === "queued" && file.place_in_queue ? `, #${file.place_in_queue}` : ""}
      </span>
      <span className="hidden text-right text-[13px] text-muted-foreground sm:block">{formatBytes(file.size)}</span>
    </li>
  )
}
