import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { Dialog } from "@base-ui/react/dialog"
import { AudioWaveform, Check, ChevronRight, CircleAlert, FolderInput, LoaderCircle, Search, Trash2, TriangleAlert, X } from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { api, type DownloadJob, type ReviewReport, type ReviewTrack } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { useDownloads, useRemoveDownload } from "@/lib/downloads"
import { useLibraryAlbum } from "@/lib/library"
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
  const [open, setOpen] = useState(false)
  return (
    <li>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex w-full items-center gap-4 px-4 py-3 text-left transition-colors outline-none hover:bg-accent/50 focus-visible:bg-accent/50"
      >
        <Cover src={artwork.data?.thumb} alt="" className="size-10 rounded-lg" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[15px]">{artwork.data?.album ?? job.title}</p>
          <p className="truncate text-[13px] text-muted-foreground">{artwork.data?.artist ?? job.parent}</p>
        </div>
        <span className="flex items-center gap-1.5 text-[13px] text-q-lossless">
          <Check className="size-4" /> <span className="hidden sm:inline">In your library</span>
        </span>
        <ChevronRight className="size-4 text-muted-foreground" />
      </button>
      <ImportedDialog job={job} open={open} onClose={() => setOpen(false)} />
    </li>
  )
}

/** What an import brought in: where it went, and what the library holds now. */
function ImportedDialog({ job, open, onClose }: { job: DownloadJob; open: boolean; onClose: () => void }) {
  const me = useMe()
  const artwork = useArtwork(job.parent, job.title)
  const library = useLibraryAlbum(open ? job.parent : null, open ? job.title : null)
  const remove = useRemoveDownload()
  const requester = requesterLabel(me, job.requested_by)
  const tracks = library.data?.state === "in-library" ? library.data.tracks : []

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md" />
        <Dialog.Popup className="fixed inset-0 z-50 m-auto flex h-[100dvh] w-full flex-col overflow-hidden bg-card outline-none sm:h-auto sm:max-h-[85dvh] sm:w-[min(92vw,640px)] sm:rounded-3xl sm:border">
          <div className="flex items-start gap-5 p-6">
            <Cover src={artwork.data?.cover} alt="" className="size-28 shrink-0 rounded-2xl shadow-lg" />
            <div className="min-w-0 flex-1">
              <Dialog.Title className="type-title line-clamp-2 text-[24px]">
                {library.data?.album ?? artwork.data?.album ?? job.title}
              </Dialog.Title>
              <Dialog.Description className="truncate text-muted-foreground">
                {library.data?.artist ?? artwork.data?.artist ?? job.parent}
                {library.data?.year ? `, ${library.data.year}` : ""}
              </Dialog.Description>
              <p className="mt-3 flex items-center gap-1.5 text-[14px] text-q-lossless">
                <Check className="size-4" /> In your library
                {library.data?.quality_label && <span className="text-muted-foreground">as {library.data.quality_label}</span>}
              </p>
            </div>
            <Dialog.Close className="rounded-full p-2 text-muted-foreground hover:text-foreground" aria-label="Close">
              <X className="size-5" />
            </Dialog.Close>
          </div>

          <dl className="grid grid-cols-2 gap-x-6 gap-y-3 border-t px-6 py-4 text-sm">
            <div className="col-span-2 min-w-0">
              <dt className="text-[12px] text-muted-foreground">Imported to</dt>
              <dd className="truncate" title={job.imported_to ?? undefined}>
                {job.imported_to ?? "Your library"}
              </dd>
            </div>
            {job.imported_at && (
              <div>
                <dt className="text-[12px] text-muted-foreground">When</dt>
                <dd>{new Date(job.imported_at * 1000).toLocaleString()}</dd>
              </div>
            )}
            <div className="min-w-0">
              <dt className="text-[12px] text-muted-foreground">From</dt>
              <dd className="truncate">
                <Link to="/soulseek/users/$username" params={{ username: job.username }} className="hover:underline">
                  {job.username}
                </Link>
              </dd>
            </div>
            {requester && (
              <div>
                <dt className="text-[12px] text-muted-foreground">Requested by</dt>
                <dd>{requester}</dd>
              </div>
            )}
          </dl>

          <div className="scrollbar-themed min-h-0 flex-1 overflow-y-auto border-t px-3 py-3">
            {library.isPending ? (
              <p className="px-3 py-4 text-sm text-muted-foreground">Looking it up in Navidrome</p>
            ) : tracks.length ? (
              <ol>
                {tracks.map((t, i) => (
                  <li key={`${t.disc}-${t.track}-${i}`} className="flex items-center gap-3 rounded-lg px-3 py-1.5 text-[14.5px]">
                    <span className="w-8 text-right text-[13px] text-muted-foreground/70">
                      {t.disc && t.disc > 1 ? `${t.disc}-` : ""}
                      {t.track ?? i + 1}
                    </span>
                    <span className="truncate">{t.title}</span>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="px-3 py-4 text-sm text-muted-foreground">
                Navidrome hasn't listed it yet. It appears once its scan finishes.
              </p>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2 border-t px-6 py-4 pb-[max(1rem,env(safe-area-inset-bottom))]">
            <Button variant="outline" nativeButton={false} render={<Link to="/" search={{ q: [job.parent, job.title].filter(Boolean).join(" ") }} />}>
              <Search /> Search again
            </Button>
            <Button
              variant="ghost"
              className="ml-auto text-muted-foreground"
              onClick={() => remove.mutate(job.id, { onSuccess: onClose })}
              disabled={remove.isPending}
            >
              Remove from this list
            </Button>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
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
  const actions = (
    <>
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
    </>
  )

  return (
    <li className="overflow-clip rounded-3xl border bg-card/60">
      <header className="flex flex-wrap items-center gap-5 p-5 sm:p-6">
        <Cover src={artwork.data?.cover} pending={artwork.isPending} alt="" className="size-24 rounded-2xl shadow-lg sm:size-28" />
        <div className="min-w-0 flex-1">
          <h2 className="type-title line-clamp-2 text-[22px] sm:truncate sm:text-[24px]">{report.data?.album ?? artwork.data?.album ?? job.title}</h2>
          <p className="truncate text-muted-foreground">
            {report.data?.album_artist ?? artwork.data?.artist ?? job.parent}
            {report.data?.year ? `, ${report.data.year}` : ""}
          </p>
          {requester && <p className="mt-0.5 text-[13.5px] text-muted-foreground">Requested by {requester}</p>}
          <Verdict job={job} report={report.data ?? null} />
        </div>
        <div className="hidden items-center gap-2 sm:flex">{actions}</div>
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
      {/* Phones: the decision stays in reach while scrolling a long tracklist. */}
      <div className="sticky bottom-[calc(4.75rem+env(safe-area-inset-bottom))] z-10 flex items-center gap-2 border-t bg-card/90 px-4 py-3 backdrop-blur-md sm:hidden">
        {actions}
      </div>
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
