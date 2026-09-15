import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { AudioWaveform, Check, CircleAlert, FolderInput, LoaderCircle, Trash2, TriangleAlert } from "lucide-react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { api, type DownloadJob, type ReviewReport, type ReviewTrack } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { useDownloads, useRemoveDownload } from "@/lib/downloads"
import { requesterLabel, useMe } from "@/lib/session"
import { formatTrackTime, plural } from "@/lib/format"
import { TIER_TEXT, tierOf } from "@/lib/quality"
import { cn } from "@/lib/utils"
import { PageFrame } from "@/pages/placeholder-pages"

export function ReviewPage() {
  const downloads = useDownloads()
  const waiting = (downloads.data ?? []).filter((job) => job.status === "ready")
  const imported = (downloads.data ?? []).filter((job) => job.status === "imported").slice(0, 5)

  return (
    <PageFrame title="Review" wide>
      {waiting.length === 0 ? (
        <EmptyState
          illumination={0.5}
          title="Nothing waiting for review"
          action={<Button nativeButton={false} render={<Link to="/" search={{}} />}>Find an album</Button>}
        >
          Every download stops here before it reaches your library. You'll see how each file checks out and exactly where
          it will be saved, then approve it or throw it away.
        </EmptyState>
      ) : (
        <ul className="mt-6 space-y-6">
          {waiting.map((job) => (
            <ReviewCard key={job.id} job={job} />
          ))}
        </ul>
      )}

      {imported.length > 0 && (
        <section className="mt-14 pb-24">
          <h2 className="text-sm text-muted-foreground">Recently imported</h2>
          <ul className="mt-3 divide-y rounded-2xl border bg-card/40">
            {imported.map((job) => (
              <ImportedRow key={job.id} job={job} />
            ))}
          </ul>
        </section>
      )}
    </PageFrame>
  )
}

function ImportedRow({ job }: { job: DownloadJob }) {
  const artwork = useArtwork(job.parent, job.title)
  return (
    <li className="flex items-center gap-4 px-4 py-3">
      <Cover src={artwork.data?.thumb} alt="" className="size-10 rounded-lg" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px]">{artwork.data?.album ?? job.title}</p>
        <p className="truncate text-[13px] text-muted-foreground">{artwork.data?.artist ?? job.parent}</p>
      </div>
      <span className="flex items-center gap-1.5 text-[13px] text-q-lossless">
        <Check className="size-4" /> In your library
      </span>
    </li>
  )
}

function ReviewCard({ job }: { job: DownloadJob }) {
  const me = useMe()
  const requester = requesterLabel(me, job.requested_by)
  const own = job.requested_by === me.username
  // Someone else's download, or your own when imports need approval: say who has to act.
  const canImport = me.permissions.manage || (own && me.can_import)
  const client = useQueryClient()
  const artwork = useArtwork(job.parent, job.title)
  const report = useQuery({
    queryKey: ["review", job.id, job.review],
    queryFn: ({ signal }) => api.review(job.id, signal),
    enabled: job.review === "ready",
    retry: false,
  })
  const importRelease = useMutation({
    mutationFn: () => api.importRelease(job.id),
    onSuccess: () => client.invalidateQueries({ queryKey: ["downloads"] }),
  })
  const discard = useRemoveDownload()

  return (
    <li className="overflow-hidden rounded-3xl border bg-card/60">
      <header className="flex flex-wrap items-center gap-5 p-5 sm:p-6">
        <Cover src={artwork.data?.cover} pending={artwork.isPending} alt="" className="size-24 rounded-2xl shadow-lg sm:size-28" />
        <div className="min-w-0 flex-1">
          <h2 className="type-title truncate text-[24px]">{report.data?.album ?? artwork.data?.album ?? job.title}</h2>
          <p className="truncate text-muted-foreground">
            {report.data?.album_artist ?? artwork.data?.artist ?? job.parent}
            {report.data?.year ? `, ${report.data.year}` : ""}
          </p>
          {requester && <p className="mt-0.5 text-[13.5px] text-muted-foreground">Requested by {requester}</p>}
          <Verdict job={job} report={report.data ?? null} />
        </div>
        <div className="flex w-full items-center gap-2 sm:w-auto">
          <Button
            variant="ghost"
            onClick={() => discard.mutate(job.id)}
            disabled={discard.isPending || importRelease.isPending}
            className="text-muted-foreground"
          >
            <Trash2 /> Discard
          </Button>
          <Button
            size="lg"
            className="h-11 flex-1 rounded-xl px-5 font-semibold sm:flex-none"
            disabled={!canImport || !report.data || !!report.data.blocked_reason || importRelease.isPending}
            onClick={() => importRelease.mutate()}
          >
            {importRelease.isPending ? <LoaderCircle className="animate-spin" /> : <FolderInput />}
            {!canImport ? "Waiting for an admin" : requester ? "Approve and import" : "Import into library"}
          </Button>
        </div>
      </header>

      {importRelease.isError && (
        <p className="mx-6 mb-4 rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm">
          {importRelease.error.message}
        </p>
      )}

      {report.data && <ReportBody report={report.data} />}
      {job.review !== "ready" && (
        <p className="flex items-center gap-2 border-t px-6 py-5 text-sm text-muted-foreground">
          <AudioWaveform className="size-4 animate-pulse" />
          Playing every file through and checking its sound. This takes a few seconds per album.
        </p>
      )}
    </li>
  )
}

function Verdict({ job, report }: { job: DownloadJob; report: ReviewReport | null }) {
  if (!report) return null
  const problems = report.tracks.filter((t) => t.problem).length
  if (report.blocked_reason) {
    return (
      <p className="mt-2 flex items-center gap-2 text-[14px] text-destructive">
        <CircleAlert className="size-4 shrink-0" /> {report.blocked_reason}
      </p>
    )
  }
  if (problems > 0) {
    return (
      <p className="mt-2 flex items-center gap-2 text-[14px] text-q-hires">
        <TriangleAlert className="size-4 shrink-0" /> {plural(problems, "track")} need a look before you import
      </p>
    )
  }
  return (
    <p className="mt-2 flex items-center gap-2 text-[14px] text-q-lossless">
      <Check className="size-4 shrink-0" /> {plural(report.tracks.length, "track")} from {job.username} play cleanly
    </p>
  )
}

function ReportBody({ report }: { report: ReviewReport }) {
  const folder = report.tracks[0]?.destination.split("/").slice(0, -1).join("/")
  return (
    <div className="border-t">
      {(report.warnings.length > 0 || report.conflicts.length > 0) && (
        <ul className="space-y-1.5 px-6 pt-4 text-[13.5px]">
          {report.warnings.map((warning) => (
            <li key={warning} className="flex gap-2 text-muted-foreground">
              <TriangleAlert className="mt-0.5 size-3.5 shrink-0 text-q-hires" /> {warning}
            </li>
          ))}
          {report.conflicts.slice(0, 3).map((conflict) => (
            <li key={conflict} className="flex gap-2 text-destructive">
              <CircleAlert className="mt-0.5 size-3.5 shrink-0" /> Already in your library: {conflict}
            </li>
          ))}
        </ul>
      )}

      <p className="px-6 pt-4 text-[13px] text-muted-foreground">
        Will be saved to <span className="text-foreground">{folder}</span>
        {report.library_dir ? <span className="text-muted-foreground/60"> in {report.library_dir}</span> : null}
        {report.cover ? ", with cover art" : ""}
      </p>

      <ol className="grid gap-x-6 px-3 py-3 sm:px-4 lg:grid-cols-2">
        {report.tracks.map((track) => (
          <TrackRow key={track.file} track={track} />
        ))}
      </ol>
    </div>
  )
}

function TrackRow({ track }: { track: ReviewTrack }) {
  const name = track.destination.split("/").at(-1)
  const tier = tierOf(track.quality)
  return (
    <li className="grid grid-cols-[28px_minmax(0,1fr)_auto] items-start gap-x-3 rounded-xl px-2 py-2" title={`From ${track.file}`}>
      <span className="pt-0.5 text-right text-[13px] text-muted-foreground/70">{track.track}</span>
      <span className="min-w-0">
        <span className="block truncate text-[14.5px]">{track.title}</span>
        <span className="block truncate text-[12px] text-muted-foreground/70">{name}</span>
        {track.problem && (
          <span className={cn("mt-0.5 block text-[12.5px]", track.suspect_transcode ? "text-q-hires" : "text-destructive")}>
            {track.problem}
          </span>
        )}
      </span>
      <span className="text-right text-[12.5px]">
        <span className={cn("block", TIER_TEXT[tier])}>{track.quality_label}</span>
        <span className="block text-muted-foreground/70">
          {track.cutoff_hz ? `up to ${Math.round(track.cutoff_hz / 1000)} kHz, ` : ""}
          {formatTrackTime(track.duration_secs)}
        </span>
      </span>
    </li>
  )
}
