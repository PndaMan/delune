import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { Dialog } from "@base-ui/react/dialog"
import {
  AudioWaveform,
  Check,
  ChevronRight,
  CircleAlert,
  Disc3,
  FolderInput,
  LoaderCircle,
  Search,
  Trash2,
  TriangleAlert,
  X,
} from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { PlayButton, ReviewPlayerProvider, usePlayer } from "@/components/review-player"
import { useMusicViews } from "@/components/music-views"
import { useAlbum } from "@/lib/music"
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
    <ReviewPlayerProvider>
    <PageFrame title="Review" wide>
      {waiting.length === 0 ? (
        <EmptyState
          illumination={0.5}
          title="Nothing waiting for review"
          action={
            <Button nativeButton={false} render={<Link to="/" search={{}} />}>
              Find an album
            </Button>
          }
        >
          Every download stops here before it reaches your library. You'll see how each file checks out and exactly
          where it will be saved, then approve it or throw it away.
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
      {/* Room for the player bar. */}
      <div className="h-24" aria-hidden />
    </PageFrame>
    </ReviewPlayerProvider>
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
  const views = useMusicViews()
  const artwork = useArtwork(job.parent, job.title)
  const library = useLibraryAlbum(open ? job.parent : null, open ? job.title : null)
  const inLibrary = library.data?.state === "in-library" ? library.data : null
  // Navidrome lists the tracks once it has scanned; until then, the album's own tracklist.
  const album = useAlbum(open && !inLibrary?.tracks.length ? (job.parent ?? null) : null, open ? job.title : null)
  const remove = useRemoveDownload()
  const requester = requesterLabel(me, job.requested_by)
  const artist = inLibrary?.artist ?? artwork.data?.artist ?? job.parent ?? null
  const title = inLibrary?.album ?? artwork.data?.album ?? job.title
  const tracks: { key: string; number: string; title: string; durationSecs?: number | null }[] = inLibrary?.tracks
    .length
    ? inLibrary.tracks.map((t, i) => ({
        key: `${t.disc}-${t.track}-${i}`,
        number: `${t.disc && t.disc > 1 ? `${t.disc}-` : ""}${t.track ?? i + 1}`,
        title: t.title,
      }))
    : (album.data?.tracks ?? []).map((t) => ({
        key: `${t.position}-${t.title}`,
        number: String(t.position),
        title: t.title,
        durationSecs: t.duration_secs,
      }))
  const folder = job.imported_to?.split("/").filter(Boolean).at(-1) ?? job.imported_to

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup className="fixed inset-x-0 bottom-0 z-50 flex max-h-[92dvh] flex-col overflow-hidden rounded-t-3xl border-t bg-card outline-none transition-transform duration-200 data-ending-style:translate-y-full data-starting-style:translate-y-full sm:inset-0 sm:m-auto sm:h-fit sm:max-h-[86dvh] sm:w-[min(92vw,620px)] sm:rounded-3xl sm:border sm:shadow-[0_40px_120px_-20px_rgb(0_0_0/0.8)] sm:data-ending-style:translate-y-0 sm:data-starting-style:translate-y-0">
          <Dialog.Close
            className="absolute top-3 right-3 z-10 flex size-9 items-center justify-center rounded-full bg-background/70 text-muted-foreground outline-none backdrop-blur hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            aria-label="Close"
          >
            <X className="size-4" />
          </Dialog.Close>

          <div className="flex items-start gap-4 p-5 pr-14 sm:gap-5 sm:p-6 sm:pr-14">
            <Cover src={artwork.data?.cover} alt="" className="size-20 shrink-0 rounded-xl shadow-lg sm:size-24" />
            <div className="min-w-0 flex-1">
              <Dialog.Title className="type-title line-clamp-2 text-[21px] leading-tight sm:text-[24px]">
                {title}
              </Dialog.Title>
              <Dialog.Description className="mt-0.5 truncate text-muted-foreground" render={<div />}>
                {artist ? (
                  <Link
                    to="/artist/$name"
                    params={{ name: artist }}
                    onClick={onClose}
                    className="underline-offset-4 hover:text-foreground hover:underline"
                  >
                    {artist}
                  </Link>
                ) : (
                  "Unknown artist"
                )}
                {inLibrary?.year ? `, ${inLibrary.year}` : ""}
              </Dialog.Description>
              <p className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13.5px] text-q-lossless">
                <Check className="size-4" />
                In your library
                {inLibrary?.quality_label && (
                  <span className="text-muted-foreground">as {inLibrary.quality_label}</span>
                )}
                <span className="text-muted-foreground">· added {importedWhen(job.imported_at)}</span>
              </p>
            </div>
          </div>

          {tracks.length > 0 && (
            <ol className="scrollbar-themed max-h-[38dvh] min-h-0 overflow-y-auto border-t p-2 sm:p-3">
              {tracks.map((track) => (
                <li key={track.key}>
                  <button
                    type="button"
                    onClick={() =>
                      views.openTrack({ artist, title: track.title, album: title, durationSecs: track.durationSecs })
                    }
                    className="flex w-full items-center gap-4 rounded-lg px-3 py-1.5 text-left outline-none hover:bg-accent/60 focus-visible:bg-accent/60"
                  >
                    <span className="w-7 shrink-0 text-right text-[13px] text-muted-foreground/70">{track.number}</span>
                    <span className="min-w-0 flex-1 truncate text-[14.5px]">{track.title}</span>
                    {track.durationSecs ? (
                      <span className="shrink-0 text-[13px] text-muted-foreground">
                        {formatTrackTime(track.durationSecs)}
                      </span>
                    ) : null}
                  </button>
                </li>
              ))}
            </ol>
          )}

          <p className="border-t px-5 py-3 text-[13px] text-muted-foreground sm:px-6">
            {folder ? (
              <>
                Filed under <span className="text-foreground">{folder}</span>
              </>
            ) : (
              "In your library"
            )}
            , from{" "}
            <Link
              to="/soulseek/users/$username"
              params={{ username: job.username }}
              onClick={onClose}
              className="underline-offset-4 hover:text-foreground hover:underline"
            >
              {job.username}
            </Link>
            {requester && <>, for {requester}</>}
            {!tracks.length && !library.isPending && " · Navidrome lists the songs once its scan finishes"}
          </p>

          <div className="flex flex-wrap items-center gap-2 border-t px-5 py-4 pb-[max(1rem,env(safe-area-inset-bottom))] sm:px-6">
            <Button variant="outline" onClick={() => views.openAlbum({ artist, title })}>
              <Disc3 /> About this album
            </Button>
            <Button
              variant="ghost"
              nativeButton={false}
              render={<Link to="/" search={{ q: [artist, title].filter(Boolean).join(" ") }} onClick={onClose} />}
            >
              <Search /> Find another copy
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

/** "today at 09:08", "yesterday", or a date, which reads better than a full timestamp. */
function importedWhen(at: number | null): string {
  if (!at) return "recently"
  const when = new Date(at * 1000)
  const time = when.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })
  const days = Math.floor((Date.now() - at * 1000) / 86_400_000)
  if (days < 1 && when.getDate() === new Date().getDate()) return `today at ${time}`
  if (days < 2) return `yesterday at ${time}`
  return when.toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" })
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
    meta: { quiet: true },
    mutationFn: () => api.importRelease(job.id),
    onSuccess: () => client.invalidateQueries({ queryKey: ["downloads"] }),
  })
  const discard = useRemoveDownload()
  const [confirmDiscard, setConfirmDiscard] = useState(false)
  const actions = (
    <>
      <Button
        variant="ghost"
        onClick={() => setConfirmDiscard(true)}
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

  const artistName = report.data?.album_artist ?? artwork.data?.artist ?? job.parent ?? null
  const views = useMusicViews()

  return (
    <li className="overflow-clip rounded-3xl border bg-card/60">
      <header className="flex flex-wrap items-center gap-5 p-5 sm:p-6">
        <button
          type="button"
          onClick={() => views.openAlbum({ artist: artistName, title: report.data?.album ?? job.title })}
          aria-label={`About ${report.data?.album ?? job.title}`}
          className="rounded-2xl outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Cover
            src={artwork.data?.cover}
            pending={artwork.isPending}
            alt=""
            className="size-24 rounded-2xl shadow-lg sm:size-28"
          />
        </button>
        <div className="min-w-0 flex-1">
          <h2 className="type-title line-clamp-2 text-[22px] sm:truncate sm:text-[24px]">
            {report.data?.album ?? artwork.data?.album ?? job.title}
          </h2>
          <p className="truncate text-muted-foreground">
            {artistName ? (
              <Link
                to="/artist/$name"
                params={{ name: artistName }}
                className="underline-offset-4 hover:text-foreground hover:underline"
              >
                {artistName}
              </Link>
            ) : (
              "Unknown artist"
            )}
            {report.data?.year ? `, ${report.data.year}` : ""}
          </p>
          {requester && <p className="mt-0.5 text-[13.5px] text-muted-foreground">Requested by {requester}</p>}
          <Verdict job={job} report={report.data ?? null} />
        </div>
        <div className="hidden items-center gap-2 sm:flex">{actions}</div>
      </header>

      {confirmDiscard && (
        <div className="mx-5 mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 sm:mx-6">
          <p className="min-w-0 flex-1 text-[14px]">Discard this download and delete its files?</p>
          <Button variant="ghost" size="sm" onClick={() => setConfirmDiscard(false)}>
            Keep it
          </Button>
          <Button variant="destructive" size="sm" disabled={discard.isPending} onClick={() => discard.mutate(job.id)}>
            Discard
          </Button>
        </div>
      )}

      {importRelease.isError && (
        <p className="mx-6 mb-4 rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm">
          {importRelease.error.message}
        </p>
      )}

      {report.isError && (
        <div className="mx-5 mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 sm:mx-6">
          <p className="min-w-0 flex-1 text-[14px]">
            Couldn't load the check for this download: {report.error.message}
          </p>
          <Button variant="outline" size="sm" onClick={() => void report.refetch()} disabled={report.isFetching}>
            Try again
          </Button>
        </div>
      )}
      {report.data && <ReportBody report={report.data} jobId={job.id} />}
      {job.review !== "ready" && (
        <p className="flex items-center gap-2 border-t px-6 py-5 text-sm text-muted-foreground">
          <AudioWaveform className="size-4 animate-pulse" />
          Playing every file through and checking its sound. This takes a few seconds per album.
        </p>
      )}
      {/* Phones: the decision stays in reach while scrolling a long tracklist. */}
      <div className="sticky bottom-[calc(var(--chrome-bottom)+var(--player-height,0px))] z-10 flex items-center gap-2 border-t bg-card/90 px-4 py-3 backdrop-blur-md sm:hidden">
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

function ReportBody({ report, jobId }: { report: ReviewReport; jobId: string }) {
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
          <TrackRow key={track.file} track={track} jobId={jobId} album={report.album} />
        ))}
      </ol>
    </div>
  )
}

function TrackRow({ track, jobId, album }: { track: ReviewTrack; jobId: string; album: string }) {
  const name = track.destination.split("/").at(-1)
  const tier = tierOf(track.quality)
  const player = usePlayer()
  const playable = {
    jobId,
    file: track.file,
    title: track.title,
    subtitle: `${track.artist} · ${album} · ${track.quality_label ?? ""}`,
    cutoffHz: track.cutoff_hz,
    sampleRate: track.quality?.sample_rate,
  }
  const active = player?.current?.jobId === jobId && player.current.file === track.file
  return (
    <li
      className={cn(
        "grid grid-cols-[28px_minmax(0,1fr)_auto] items-start gap-x-3 rounded-xl px-2 py-2",
        active && "bg-primary/10",
      )}
      title={`From ${track.file}`}
    >
      <span className="flex justify-end">
        <PlayButton track={playable} label={track.track} />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-[14.5px]">{track.title}</span>
        <span className="block truncate text-[12px] text-muted-foreground/70">{name}</span>
        {track.problem && (
          <span
            className={cn("mt-0.5 block text-[12.5px]", track.suspect_transcode ? "text-q-hires" : "text-destructive")}
          >
            {track.problem}
          </span>
        )}
      </span>
      <span className="text-right text-[12.5px]">
        <span className={cn("block", TIER_TEXT[tier])}>{track.quality_label}</span>
        <button
          type="button"
          onClick={() => player?.showSpectrogram(playable)}
          title="Show the spectrogram"
          className="block text-muted-foreground/70 underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring"
        >
          {track.cutoff_hz ? `up to ${Math.round(track.cutoff_hz / 1000)} kHz, ` : ""}
          {formatTrackTime(track.duration_secs)}
        </button>
      </span>
    </li>
  )
}
