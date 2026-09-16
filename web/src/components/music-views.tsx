import { Dialog } from "@base-ui/react/dialog"
import { Link } from "@tanstack/react-router"
import { CircleCheck, Copy, LoaderCircle, Quote, Search, Sparkles, X } from "lucide-react"
import { createContext, use, useMemo, useState } from "react"

import { BandcampOfferLine } from "@/components/bandcamp"
import { Cover } from "@/components/cover"
import { FollowButton } from "@/components/follow-button"
import { Button } from "@/components/ui/button"
import { useArtwork } from "@/lib/artwork"
import { formatRuntime, formatTrackTime, plural } from "@/lib/format"
import { plainFrom, useAlbum, useLyrics } from "@/lib/music"
import { useMe } from "@/lib/session"
import { useAddToWishlist } from "@/lib/wishlist"
import { cn } from "@/lib/utils"

export type AlbumRef = { artist: string | null; title: string }
export type TrackRef = { artist: string | null; title: string; album?: string | null; durationSecs?: number | null }

type MusicViews = {
  /** Open an album: its tracklist, whether you have it, and what to do about it. */
  openAlbum: (album: AlbumRef) => void
  /** Open one song: where it's from, how long it is, and its words. */
  openTrack: (track: TrackRef) => void
}

const Views = createContext<MusicViews>({ openAlbum: () => {}, openTrack: () => {} })

/** Anywhere inside the app: `useMusicViews().openAlbum({ artist, title })`. */
export function useMusicViews() {
  return use(Views)
}

/** Holds the album and song views, so any row anywhere can open them. */
export function MusicViewsProvider({ children }: { children: React.ReactNode }) {
  const [album, setAlbum] = useState<AlbumRef | null>(null)
  const [track, setTrack] = useState<TrackRef | null>(null)
  const views = useMemo<MusicViews>(() => ({ openAlbum: setAlbum, openTrack: setTrack }), [])
  return (
    <Views.Provider value={views}>
      {children}
      <AlbumDialog album={album} onClose={() => setAlbum(null)} onTrack={setTrack} />
      <TrackDialog track={track} onClose={() => setTrack(null)} />
    </Views.Provider>
  )
}

function Shell({
  open,
  onClose,
  label,
  children,
  wide,
}: {
  open: boolean
  onClose: () => void
  label: string
  children: React.ReactNode
  wide?: boolean
}) {
  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup
          aria-label={label}
          className={cn(
            "fixed inset-x-0 bottom-0 z-50 flex max-h-[92dvh] flex-col overflow-hidden rounded-t-3xl border-t bg-card outline-none",
            "transition-transform duration-200 data-ending-style:translate-y-full data-starting-style:translate-y-full",
            "sm:inset-0 sm:m-auto sm:h-fit sm:max-h-[86dvh] sm:rounded-3xl sm:border sm:shadow-[0_40px_120px_-20px_rgb(0_0_0/0.8)] sm:data-ending-style:translate-y-0 sm:data-starting-style:translate-y-0",
            wide ? "sm:w-[min(94vw,760px)]" : "sm:w-[min(94vw,560px)]",
          )}
        >
          <Dialog.Close
            aria-label="Close"
            className="absolute top-3 right-3 z-10 flex size-9 items-center justify-center rounded-full bg-background/70 text-muted-foreground outline-none backdrop-blur hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <X className="size-4" />
          </Dialog.Close>
          {children}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function AlbumDialog({
  album,
  onClose,
  onTrack,
}: {
  album: AlbumRef | null
  onClose: () => void
  onTrack: (track: TrackRef) => void
}) {
  return (
    <Shell open={album !== null} onClose={onClose} label={album?.title ?? "Album"} wide>
      {album && <AlbumBody album={album} onClose={onClose} onTrack={onTrack} />}
    </Shell>
  )
}

/**
 * One album: cover, tracklist, whether it's in the library, and what to do next.
 * Shown inside a dialog in the app, and as a page when a link opens it directly.
 */
export function AlbumBody({
  album,
  onClose,
  onTrack,
}: {
  album: AlbumRef
  onClose?: () => void
  onTrack: (track: TrackRef) => void
}) {
  const me = useMe()
  const info = useAlbum(album?.artist, album?.title ?? null)
  const artwork = useArtwork(album?.artist ?? null, album?.title ?? null)
  const addToWishlist = useAddToWishlist()
  const cover = info.data?.cover ?? artwork.data?.cover ?? artwork.data?.thumb
  const title = info.data?.title ?? album?.title ?? ""
  const artist = info.data?.artist ?? album?.artist ?? null
  const owned = info.data?.in_library.state === "in-library"
  const runtime = info.data?.tracks.reduce((total, track) => total + (track.duration_secs ?? 0), 0)

  return (
    <>
      <div className="flex gap-4 border-b p-5 sm:gap-5 sm:p-6">
        <Cover src={cover} pending={info.isPending} alt="" className="size-24 rounded-xl sm:size-32" />
        <div className="min-w-0 flex-1 pr-10">
          {/* A plain heading, so these bodies also work as pages outside a dialog. */}
          <h2 className="type-title text-[21px] leading-tight text-balance sm:text-[26px]">{title}</h2>
          {artist && (
            <Link
              to="/artist/$name"
              params={{ name: artist }}
              onClick={onClose}
              className="mt-1 block truncate text-[16px] text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
            >
              {artist}
            </Link>
          )}
          <p className="mt-1.5 text-[13.5px] text-muted-foreground">
            {[
              info.data?.year,
              info.data?.tracks.length ? plural(info.data.tracks.length, "track") : null,
              runtime ? formatRuntime(runtime) : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
          {owned && (
            <p className="mt-2 flex items-center gap-1.5 text-[13.5px] text-q-lossless">
              <CircleCheck className="size-4" /> In your library
            </p>
          )}
          {artist && (
            <div className="mt-3">
              <FollowButton artist={artist} size="small" />
            </div>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {info.isPending ? (
          <div className="space-y-2 p-5">
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="h-8 animate-pulse rounded-lg bg-muted/40" />
            ))}
          </div>
        ) : info.data?.tracks.length ? (
          <ol className="p-2 sm:p-3">
            {info.data.tracks.map((track) => (
              <li key={`${track.position}-${track.title}`}>
                <button
                  type="button"
                  onClick={() =>
                    onTrack({
                      artist: track.artist ?? artist,
                      title: track.title,
                      album: title,
                      durationSecs: track.duration_secs,
                    })
                  }
                  className="flex w-full items-center gap-4 rounded-lg px-3 py-2 text-left outline-none hover:bg-accent/60 focus-visible:bg-accent/60"
                >
                  <span className="w-6 shrink-0 text-right text-[13px] text-muted-foreground/70">{track.position}</span>
                  <span className="min-w-0 flex-1 truncate text-[14.5px]">{track.title}</span>
                  <span className="shrink-0 text-[13px] text-muted-foreground">
                    {formatTrackTime(track.duration_secs)}
                  </span>
                </button>
              </li>
            ))}
          </ol>
        ) : (
          <p className="px-5 py-6 text-[14.5px] text-muted-foreground">
            No tracklist for this one. Searching Soulseek will still find it.
          </p>
        )}
      </div>

      {!owned && <BandcampOfferLine artist={artist} title={title} />}
      <div className="flex flex-wrap gap-2 border-t p-4 sm:p-5">
        <Button
          nativeButton={false}
          render={<Link to="/" search={{ q: [artist, title].filter(Boolean).join(" ") }} onClick={onClose} />}
          className="flex-1"
        >
          <Search /> Find on Soulseek
        </Button>
        {me.permissions.download && !owned && (
          <Button
            variant="outline"
            className="flex-1"
            disabled={addToWishlist.isPending || addToWishlist.isSuccess}
            onClick={() => addToWishlist.mutate({ query: [artist, title].filter(Boolean).join(" ") })}
          >
            {addToWishlist.isPending ? (
              <LoaderCircle className="animate-spin" />
            ) : addToWishlist.isSuccess ? (
              <CircleCheck />
            ) : (
              <Sparkles />
            )}
            {addToWishlist.isSuccess ? "On your wishlist" : "Keep looking for it"}
          </Button>
        )}
      </div>
    </>
  )
}

function TrackDialog({ track, onClose }: { track: TrackRef | null; onClose: () => void }) {
  return (
    <Shell open={track !== null} onClose={onClose} label={track?.title ?? "Song"}>
      {track && <SongBody track={track} onClose={onClose} />}
    </Shell>
  )
}

/** Lyrics arrive as lines; blank lines are where one verse ends and the next begins. */
function verses(lines: string[]) {
  const found: string[][] = [[]]
  for (const line of lines) {
    if (line.trim()) found[found.length - 1].push(line)
    else if (found[found.length - 1].length) found.push([])
  }
  return found.filter((verse) => verse.length)
}

/**
 * One song: its cover, where it's from, and its words set to be read — verse by
 * verse in a narrow column, with the album art washed in behind the title.
 */
export function SongBody({ track, onClose }: { track: TrackRef; onClose?: () => void }) {
  const lyrics = useLyrics(track.artist, track.title, track.durationSecs)
  const artwork = useArtwork(track.artist, track.album ?? track.title)
  const cover = artwork.data?.cover ?? artwork.data?.thumb
  const lines = lyrics.data ? plainFrom(lyrics.data) : []
  const [copied, setCopied] = useState(false)

  const copy = () => {
    void navigator.clipboard?.writeText(`${track.title}\n${track.artist ?? ""}\n\n${lines.join("\n")}`.trim())
    setCopied(true)
    window.setTimeout(() => setCopied(false), 2000)
  }

  return (
    <>
      <div className="relative overflow-hidden border-b">
        {cover && (
          <div
            className="pointer-events-none absolute inset-0 scale-125 bg-cover bg-center opacity-20 blur-2xl"
            style={{ backgroundImage: `url(${cover})` }}
            aria-hidden
          />
        )}
        <div className="relative flex gap-4 p-5 pr-14 sm:p-6 sm:pr-14">
          <Cover src={cover} pending={artwork.isPending} alt="" className="size-16 shrink-0 rounded-xl sm:size-20" />
          <div className="min-w-0 flex-1">
            <h2 className="type-title text-[20px] leading-tight text-balance sm:text-[23px]">{track.title}</h2>
            <p className="mt-1 text-[15px] text-muted-foreground">
              {track.artist && (
                <Link
                  to="/artist/$name"
                  params={{ name: track.artist }}
                  onClick={onClose}
                  className="underline-offset-4 hover:text-foreground hover:underline"
                >
                  {track.artist}
                </Link>
              )}
              {track.album && <span> · {track.album}</span>}
              {track.durationSecs ? <span> · {formatTrackTime(track.durationSecs)}</span> : null}
            </p>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-6 sm:px-6">
        {lyrics.isPending ? (
          <div className="mx-auto max-w-[42ch] space-y-2.5">
            {[0, 1, 2, 3, 4, 5, 6].map((i) => (
              <div
                key={i}
                className="h-4 animate-pulse rounded bg-muted/40"
                style={{ width: `${55 + ((i * 37) % 40)}%` }}
              />
            ))}
          </div>
        ) : lines.length ? (
          <div className="mx-auto max-w-[42ch] space-y-6">
            {verses(lines).map((verse, index) => (
              <p key={index} className="text-[16px] leading-[1.7] text-foreground/90">
                {verse.map((line, i) => (
                  // A line too long for the column is indented where it carries on,
                  // so it doesn't read as a line of its own.
                  <span key={i} className="block -indent-4 pl-4 [text-wrap:pretty]">
                    {line}
                  </span>
                ))}
              </p>
            ))}
          </div>
        ) : (
          <div className="mx-auto max-w-[42ch] py-6 text-center">
            <Quote className="mx-auto size-6 text-muted-foreground/40" strokeWidth={1.6} />
            <p className="mt-3 text-[14.5px] text-muted-foreground">
              No words for this one on LRCLIB, the open lyrics database delune asks. Songs get added all the time, so
              it's worth another look later.
            </p>
          </div>
        )}
      </div>

      {lines.length > 0 && (
        <div className="flex items-center gap-3 border-t px-5 py-3 sm:px-6">
          <p className="min-w-0 flex-1 text-[12.5px] text-muted-foreground">
            Words from{" "}
            <a
              href="https://lrclib.net"
              target="_blank"
              rel="noreferrer"
              className="underline-offset-4 hover:text-foreground hover:underline"
            >
              LRCLIB
            </a>
            {lyrics.data?.synced ? ", timed to the song" : ""}
          </p>
          <Button variant="ghost" size="sm" onClick={copy}>
            {copied ? <CircleCheck /> : <Copy />} {copied ? "Copied" : "Copy"}
          </Button>
        </div>
      )}
    </>
  )
}
