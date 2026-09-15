import { Dialog } from "@base-ui/react/dialog"
import { Link } from "@tanstack/react-router"
import {
  ArrowDownToLine,
  Bell,
  BellRing,
  Check,
  MessageSquarePlus,
  CircleCheck,
  FileImage,
  File as FileIcon,
  LoaderCircle,
  TriangleAlert,
  X,
} from "lucide-react"
import { useLayoutEffect, useRef, useState } from "react"

import { Cover } from "@/components/cover"
import { Button } from "@/components/ui/button"
import type { Candidate, CandidateFile, PeerHistory } from "@/lib/api"
import { useAccentColour, useArtwork } from "@/lib/artwork"
import { describeJob, jobForCandidate, useDownloads, useStartDownload } from "@/lib/downloads"
import { coverStatus, type LibraryMatch, type Ownership, ownership, useLibraryAlbum } from "@/lib/library"
import { formatBytes, formatRuntime, formatSpeed, formatTrackTime, plural } from "@/lib/format"
import { describeQuality, TIER_BG, TIER_TEXT, tierOf } from "@/lib/quality"
import { parseTrackName } from "@/lib/track-name"
import { matchLink, useResolved } from "@/lib/tracklist"
import { useFollow, useFollows, useUnfollow } from "@/lib/automation"
import { useHiddenUsers } from "@/lib/hidden-users"
import { REQUEST_STATUS, useCreateRequest, useRequests } from "@/lib/requests"
import { useMe } from "@/lib/session"
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

/** Phones scroll the whole sheet; the scrollbar only appears while scrolling. */
const sheetScroll = "overflow-y-auto scrollbar-themed"

function ReleaseDetail({ candidate: c }: { candidate: Candidate }) {
  const tier = tierOf(c.quality)
  const artwork = useArtwork(c.parent, c.title)
  const accent = useAccentColour(artwork.data?.thumb)
  const audio = c.files.filter((f) => f.audio)
  const other = c.files.filter((f) => !f.audio)
  const library = useLibraryAlbum(c.parent, c.title)
  const owned = ownership(c, library.data)
  const downloads = useDownloads()
  const status = coverStatus(jobForCandidate(downloads.data ?? [], c), owned)
  const linkMatch = matchLink(c, useResolved())
  const linkedTrack = linkMatch?.kind === "track" ? linkMatch.file : undefined
  // Every track is picked by default; untick any you don't want.
  const [excluded, setExcluded] = useState<Set<string>>(new Set())
  const toggle = (path: string) =>
    setExcluded((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  const picked = excluded.size ? audio.filter((f) => !excluded.has(f.path)) : null

  return (
    <div
      className={cn(
        "relative flex h-full min-h-0 flex-col lg:grid lg:grid-cols-[minmax(320px,36%)_minmax(0,1fr)] lg:overflow-hidden",
        sheetScroll,
      )}
    >
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
            status={status}
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
          <div className="min-w-0">
            <dt className="text-[12px] text-muted-foreground">From</dt>
            <dd className="truncate text-[14.5px]">
              <Link
                to="/soulseek/users/$username"
                params={{ username: c.username }}
                className="underline-offset-4 hover:underline"
                title={`Browse everything ${c.username} shares`}
              >
                {c.username}
              </Link>
            </dd>
          </div>
        </dl>
        {c.peer && <TrackRecord peer={c.peer} username={c.username} />}

        {owned && <LibraryNote owned={owned} library={library.data} />}

        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1">
          <FollowArtist artist={artwork.data?.artist ?? c.parent} />
          <HideSharer username={c.username} />
        </div>

        {c.mixed_quality && (
          <p className="mt-4 flex gap-2 text-[13px] text-q-hires">
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
            Not every file is the same quality; the label shows the lowest.
          </p>
        )}

        <div className="hidden lg:mt-auto lg:block lg:pt-6">
          <DownloadAction candidate={c} owned={owned} picked={picked} onPickAll={() => setExcluded(new Set())} />
        </div>
      </aside>

      <section className="relative flex shrink-0 flex-col px-3 lg:min-h-0 lg:shrink pt-2 pb-5 sm:px-6 lg:pt-[72px]">
        <Tracklist files={audio} owned={owned} highlight={linkedTrack?.path} excluded={excluded} onToggle={toggle} />
        {other.length > 0 && (
          <ul className="mx-3 mt-3 flex flex-wrap gap-2 border-t pt-4">
            {other.map((f) => (
              <li
                key={f.path}
                className="flex items-center gap-1.5 rounded-lg bg-muted/50 px-2.5 py-1 text-[12.5px] text-muted-foreground"
                title={f.path}
              >
                {/\.(jpe?g|png|webp|gif)$/i.test(f.name) ? (
                  <FileImage className="size-3.5" />
                ) : (
                  <FileIcon className="size-3.5" />
                )}
                {f.name}
                <span className="text-muted-foreground/60">{formatBytes(f.size)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Phones: the one action stays in reach while the tracklist scrolls. */}
      <div className="sticky bottom-0 z-10 mt-auto border-t bg-card/90 px-5 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] backdrop-blur-md lg:hidden">
        <DownloadAction candidate={c} owned={owned} picked={picked} onPickAll={() => setExcluded(new Set())} />
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
function Tracklist({
  files,
  owned,
  highlight,
  excluded,
  onToggle,
}: {
  files: CandidateFile[]
  owned: Ownership | null
  highlight?: string
  excluded: Set<string>
  onToggle: (path: string) => void
}) {
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
        // Desktop: this is the only part of the release view that scrolls.
        "lg:overflow-y-auto lg:pr-1 scrollbar-themed",
        layout.overflow && "lg:[mask-image:linear-gradient(to_bottom,black_94%,transparent)]",
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
          const have = owned !== null && !owned.missing.has(file.name)
          const on = !excluded.has(file.path)
          return (
            <li
              key={file.path}
              onClick={() => onToggle(file.path)}
              className={cn(
                "group grid cursor-pointer grid-cols-[20px_26px_minmax(0,1fr)_auto] items-center gap-x-3 rounded-lg px-3 transition-colors select-none hover:bg-accent/50",
                file.path === highlight && "bg-primary/12 ring-1 ring-primary/30 ring-inset",
                !on && "opacity-45",
              )}
              title={have ? `${file.name} (already in your library)` : file.name}
            >
              <span
                role="checkbox"
                aria-checked={on}
                aria-label={`Download ${title}`}
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === " " || e.key === "Enter") {
                    e.preventDefault()
                    onToggle(file.path)
                  }
                }}
                className={cn(
                  "flex size-[18px] items-center justify-center rounded-md border transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  on ? "border-primary bg-primary text-primary-foreground" : "border-muted-foreground/40",
                )}
              >
                {on && <Check className="size-3" strokeWidth={3} />}
              </span>
              <span className="text-right text-[13px] text-muted-foreground/70">{position ?? i + 1}</span>
              <span className={cn("flex min-w-0 items-center gap-2 text-[14.5px]", have && "text-muted-foreground")}>
                <span className="truncate">{title}</span>
                {have && <CircleCheck className="size-3.5 shrink-0 text-q-lossless" aria-label="In your library" />}
                {owned && !have && (
                  <span className="shrink-0 rounded-full bg-q-hires/15 px-1.5 text-[11px] font-medium text-q-hires">
                    Missing
                  </span>
                )}
              </span>
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

function LibraryNote({ owned, library }: { owned: Ownership; library: LibraryMatch | undefined }) {
  const quality = library?.quality_label ? ` as ${library.quality_label}` : ""
  return (
    <p className={cn("mt-4 flex gap-2 text-[13.5px]", owned.complete ? "text-q-lossless" : "text-q-hires")}>
      <CircleCheck className="mt-0.5 size-4 shrink-0" />
      {owned.complete
        ? `Already in your library${quality}.`
        : `Your library has ${owned.owned} of these ${owned.owned + owned.missing.size} tracks${quality}. This copy fills in the ${owned.missing.size} missing.`}
    </p>
  )
}

function DownloadAction({
  candidate,
  owned,
  picked,
  onPickAll,
}: {
  candidate: Candidate
  owned: Ownership | null
  /** The tracks chosen, when not all of them are. */
  picked: CandidateFile[] | null
  onPickAll: () => void
}) {
  const [confirming, setConfirming] = useState<CandidateFile[] | "folder" | null>(null)
  const start = useStartDownload()
  const match = matchLink(candidate, useResolved())
  const track = match?.kind === "track" ? match.file : undefined
  // A pasted track link downloads just that track, with the folder's artwork.
  const extras = candidate.files.filter((f) => !f.audio)
  const trackFiles = picked
    ? [...picked, ...extras]
    : track
      ? [track, ...extras.filter((f) => /\.(jpe?g|png|webp)$/i.test(f.name))]
      : undefined
  const begin = (files: CandidateFile[] | undefined) => {
    const alreadyOwned = picked
      ? owned !== null && picked.every((f) => !owned.missing.has(f.name))
      : files
        ? owned !== null && !!track && !owned.missing.has(track.name)
        : owned?.complete
    if (alreadyOwned) return setConfirming(files ?? "folder")
    start.mutate({ candidate, files })
  }
  const downloads = useDownloads()
  const job = jobForCandidate(downloads.data ?? [], candidate)
  const me = useMe()

  if (job) {
    const progress = job.total_bytes ? Math.round((job.bytes / job.total_bytes) * 100) : 0
    const done = job.status === "ready" || job.status === "imported"
    return (
      <div className="rounded-xl border bg-background/40 px-4 py-3">
        <div className="flex items-center gap-3">
          {done ? (
            <Check className="size-4 text-q-lossless" />
          ) : (
            <LoaderCircle className="size-4 animate-spin text-primary" />
          )}
          <p className="min-w-0 flex-1 truncate text-[14px]">{describeJob(job)}</p>
          <Button
            variant="outline"
            size="sm"
            nativeButton={false}
            render={<Link to={job.status === "ready" ? "/review" : "/downloads"} />}
          >
            {job.status === "ready" ? "Review" : job.status === "imported" ? "Done" : "Progress"}
          </Button>
        </div>
        {!done && (
          <div className="mt-2.5 h-1 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full bg-primary transition-[width] duration-500"
              style={{ width: `${Math.max(progress, 2)}%` }}
            />
          </div>
        )}
      </div>
    )
  }

  // People who can't download ask someone who manages delune instead.
  if (!me.permissions.download) {
    return <RequestAction candidate={candidate} files={trackFiles} picked={picked} />
  }

  // Owning every track already: ask once before fetching a second copy.
  if (confirming) {
    return (
      <div className="rounded-xl border border-q-hires/40 bg-q-hires/10 px-4 py-3">
        <p className="text-[14px]">
          {confirming === "folder"
            ? "You already have every track on this album. Download another copy anyway?"
            : picked && picked.length > 1
              ? "You already have these tracks. Download another copy anyway?"
              : "You already have this track. Download another copy anyway?"}
        </p>
        <div className="mt-3 flex gap-2">
          <Button
            className="h-10 flex-1 rounded-lg"
            onClick={() => {
              setConfirming(null)
              start.mutate({ candidate, files: confirming === "folder" ? undefined : confirming })
            }}
          >
            Download anyway
          </Button>
          <Button variant="outline" className="h-10 flex-1 rounded-lg" onClick={() => setConfirming(null)}>
            Keep mine
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div>
      <Button
        size="lg"
        variant={owned?.complete && !track && !picked ? "outline" : "default"}
        className="h-12 w-full rounded-xl text-[15px] font-semibold"
        disabled={start.isPending || picked?.length === 0}
        onClick={() => begin(trackFiles)}
      >
        {start.isPending ? <LoaderCircle className="animate-spin" /> : <ArrowDownToLine />}
        {picked
          ? picked.length === 0
            ? "Pick at least one track"
            : picked.length === 1
              ? `Download “${parseTrackName(picked[0].name).title}”`
              : `Download ${picked.length} of ${candidate.audio_files} tracks`
          : track
            ? `Download “${parseTrackName(track.name).title}”`
            : "Download for review"}
      </Button>
      {picked && (
        <button
          type="button"
          onClick={onPickAll}
          className="mt-2 w-full rounded-lg py-1.5 text-[13.5px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          Pick every track again
        </button>
      )}
      {!picked && track && candidate.audio_files > 1 && (
        <button
          type="button"
          disabled={start.isPending}
          onClick={() => begin(undefined)}
          className="mt-2 w-full rounded-lg py-1.5 text-[13.5px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          Or the whole folder, {plural(candidate.audio_files, "track")}
        </button>
      )}
      <p className="mt-2 hidden text-center text-[12.5px] text-muted-foreground sm:block">
        {start.isError ? start.error.message : "Nothing reaches your library until you approve it."}
      </p>
    </div>
  )
}

/** Ask for this copy, for someone who manages delune to approve. */
function RequestAction({
  candidate,
  files,
  picked,
}: {
  candidate: Candidate
  files: CandidateFile[] | undefined
  picked: CandidateFile[] | null
}) {
  const me = useMe()
  const artwork = useArtwork(candidate.parent, candidate.title)
  const resolved = useResolved()
  const requests = useRequests()
  const create = useCreateRequest()
  const [note, setNote] = useState("")
  const existing = (requests.data ?? []).find(
    (r) =>
      r.requested_by === me.username &&
      r.download?.username === candidate.username &&
      r.download.folder === candidate.folder &&
      r.status !== "declined",
  )

  if (!me.permissions.request) {
    return (
      <p className="rounded-xl border bg-background/40 px-4 py-3 text-[14px] text-muted-foreground">
        Downloading isn't turned on for your account. Ask an admin if you'd like it.
      </p>
    )
  }
  if (existing) {
    return (
      <div className="flex items-center gap-3 rounded-xl border bg-background/40 px-4 py-3">
        <Check className="size-4 text-q-lossless" />
        <p className="min-w-0 flex-1 text-[14px]">{REQUEST_STATUS[existing.status]}</p>
        <Button variant="outline" size="sm" nativeButton={false} render={<Link to="/downloads" />}>
          Requests
        </Button>
      </div>
    )
  }

  const title = artwork.data?.album ?? candidate.title
  const artist = artwork.data?.artist ?? candidate.parent
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault()
        create.mutate({
          title,
          artist,
          query: resolved?.query ?? [artist, title].filter(Boolean).join(" "),
          link: null,
          download: {
            username: candidate.username,
            folder: candidate.folder,
            title: candidate.title,
            parent: candidate.parent,
            files: (files ?? candidate.files).map((f) => ({ path: f.path, size: f.size })),
          },
          quality_label: candidate.quality_label,
          note: note.trim() || null,
        })
      }}
    >
      <input
        value={note}
        onChange={(e) => setNote(e.target.value)}
        placeholder="Add a note (optional)"
        maxLength={500}
        className="mb-2 h-11 w-full rounded-xl border bg-background/50 px-3.5 text-[14.5px] outline-none focus:border-primary/50"
      />
      <Button
        type="submit"
        size="lg"
        className="h-12 w-full rounded-xl text-[15px] font-semibold"
        disabled={create.isPending || picked?.length === 0}
      >
        {create.isPending ? <LoaderCircle className="animate-spin" /> : <MessageSquarePlus />}
        {picked ? `Request ${plural(picked.length, "track")}` : "Request this album"}
      </Button>
      <p className="mt-2 text-center text-[12.5px] text-muted-foreground">
        {create.isError
          ? create.error.message
          : "Someone who manages delune approves requests. You'll get a notification."}
      </p>
    </form>
  )
}

/** Follow the artist, so new releases land on the wishlist. */
function FollowArtist({ artist }: { artist: string | null | undefined }) {
  const follows = useFollows()
  const follow = useFollow()
  const unfollow = useUnfollow()
  if (!artist) return null
  const existing = follows.data?.find((f) => f.artist.toLowerCase() === artist.toLowerCase())
  return (
    <button
      type="button"
      disabled={follow.isPending || unfollow.isPending}
      onClick={() => (existing ? unfollow.mutate(existing.deezer_id) : follow.mutate(artist))}
      className={cn(
        "flex items-center gap-1.5 text-[12.5px] underline-offset-4 hover:underline",
        existing ? "text-primary" : "text-muted-foreground/80 hover:text-foreground",
      )}
      title={follow.isError ? follow.error.message : undefined}
    >
      {existing ? <BellRing className="size-3.5" /> : <Bell className="size-3.5" />}
      {existing ? `Following ${existing.artist}` : `Follow ${artist}`}
    </button>
  )
}

/** Hide someone's results for good, e.g. after a fake or a failed download. */
function HideSharer({ username }: { username: string }) {
  const { hidden, hide, unhide } = useHiddenUsers()
  const isHidden = hidden.includes(username)
  return (
    <button
      type="button"
      onClick={() => (isHidden ? unhide(username) : hide(username))}
      className="text-[12.5px] text-muted-foreground/70 underline-offset-4 hover:text-foreground hover:underline"
    >
      {isHidden ? `Show ${username}'s results again` : `Hide results from ${username}`}
    </button>
  )
}

/** How downloading from this person has gone before. */
function TrackRecord({ peer, username }: { peer: PeerHistory; username: string }) {
  const total = peer.files_done + peer.files_failed
  const flaky = peer.files_failed > peer.files_done
  const speed = formatSpeed(peer.average_speed)
  return (
    <p className={cn("mt-3 text-[13px]", flaky ? "text-destructive" : "text-muted-foreground")}>
      {flaky
        ? `Most downloads from ${username} failed before (${peer.files_failed} of ${total} files).`
        : `You've downloaded ${plural(peer.files_done, "file")} from ${username} before${
            peer.files_failed ? `, ${peer.files_failed} failed` : ""
          }${speed ? `, at about ${speed}` : ""}.`}
    </p>
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
