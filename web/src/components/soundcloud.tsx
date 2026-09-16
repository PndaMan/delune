import { Link } from "@tanstack/react-router"
import {
  ArrowDownToLine,
  ArrowUpRight,
  BadgeCheck,
  Bell,
  BellRing,
  ChevronDown,
  LoaderCircle,
  Search,
} from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { Button } from "@/components/ui/button"
import { useExternalSource, useFetchWithCommand } from "@/lib/external"
import { formatTrackTime } from "@/lib/format"
import { useMe } from "@/lib/session"
import {
  type SoundcloudArtist,
  type SoundcloudTrack,
  useSetSoundcloudFollow,
  useSoundcloudArtist,
  useSoundcloudTrack,
} from "@/lib/soundcloud"
import { cn } from "@/lib/utils"

const ORANGE = "#ff5500"

/** SoundCloud's cloud, simplified. */
export function SoundcloudGlyph({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden className={cn("size-4", className)}>
      <path
        fill="currentColor"
        d="M11.5 7.2c.7-.3 1.5-.5 2.3-.5 3 0 5.4 2.3 5.6 5.2 1.6.2 2.8 1.5 2.8 3.1 0 1.7-1.4 3.1-3.1 3.1h-7.6a.4.4 0 0 1-.4-.4V7.6c0-.2.1-.3.4-.4ZM9.3 8.4c.3 0 .5.2.5.5v9.3c0 .3-.2.5-.5.5s-.5-.2-.5-.5V8.9c0-.3.2-.5.5-.5Zm-2.2 1.5c.3 0 .5.2.5.5v7.8c0 .3-.2.5-.5.5s-.5-.2-.5-.5v-7.8c0-.3.2-.5.5-.5Zm-2.2.9c.3 0 .5.2.5.5v6.9c0 .3-.2.5-.5.5s-.5-.2-.5-.5v-6.9c0-.3.2-.5.5-.5Zm-2.2 2c.3 0 .5.2.5.5v4.9c0 .3-.2.5-.5.5s-.5-.2-.5-.5v-4.9c0-.3.2-.5.5-.5ZM.5 14.3c.3 0 .5.2.5.5v2.5c0 .3-.2.5-.5.5s-.5-.2-.5-.5v-2.5c0-.3.2-.5.5-.5Z"
      />
    </svg>
  )
}

function compact(n: number) {
  return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(n)
}

function when(at: number | null) {
  if (!at) return null
  const days = Math.floor((Date.now() / 1000 - at) / 86_400)
  if (days < 1) return "today"
  if (days < 2) return "yesterday"
  if (days < 30) return `${days} days ago`
  return new Date(at * 1000).toLocaleDateString(undefined, { month: "short", year: "numeric" })
}

/**
 * An artist's SoundCloud on their page: who they are there, what they posted
 * lately, and a follow that puts new tracks on the wishlist. Hidden when delune
 * can't find an account that's clearly theirs.
 */
export function SoundcloudSection({ artist }: { artist: string }) {
  const found = useSoundcloudArtist(artist)
  if (!found.data) return null
  return <Profile artist={found.data} />
}

function Profile({ artist: a }: { artist: SoundcloudArtist }) {
  const me = useMe()
  const follow = useSetSoundcloudFollow()
  const [showAll, setShowAll] = useState(false)
  const tracks = showAll ? a.recent : a.recent.slice(0, 4)

  return (
    <section className="mt-10">
      <h2 className="type-title mb-4 flex items-center gap-2 text-[21px]">
        <SoundcloudGlyph className="size-6" /> On SoundCloud
      </h2>
      <div className="overflow-hidden rounded-2xl border bg-card/50">
        <div className="flex flex-wrap items-center gap-4 px-5 py-4">
          <Cover src={a.avatar} alt="" className="size-14 shrink-0 rounded-full" />
          <div className="min-w-0 flex-1">
            <p className="flex items-center gap-1.5 truncate text-[16.5px] font-semibold">
              {a.name}
              {a.verified && <BadgeCheck className="size-4 shrink-0" style={{ color: ORANGE }} aria-label="Verified" />}
            </p>
            <p className="text-[13.5px] text-muted-foreground">
              {compact(a.followers)} followers · {a.tracks.toLocaleString()} tracks
            </p>
          </div>
          <div className="flex w-full gap-2 sm:w-auto">
            {me.permissions.download && (
              <Button
                variant="outline"
                className={cn(
                  "flex-1 sm:flex-none",
                  a.following && "border-[#ff5500]/40 bg-[#ff5500]/10 text-[#ff7a33] hover:bg-[#ff5500]/15",
                )}
                disabled={follow.isPending}
                aria-pressed={a.following}
                onClick={() => follow.mutate({ artist: a.following ? String(a.id) : a.permalink, on: !a.following })}
              >
                {follow.isPending ? <LoaderCircle className="animate-spin" /> : a.following ? <BellRing /> : <Bell />}
                {a.following ? "Following new tracks" : "Follow new tracks"}
              </Button>
            )}
            <Button
              variant="ghost"
              className="flex-1 sm:flex-none"
              nativeButton={false}
              render={<a href={a.url} target="_blank" rel="noreferrer" />}
            >
              Open <ArrowUpRight />
            </Button>
          </div>
        </div>
        {tracks.length > 0 && (
          <ul className="border-t p-1.5">
            {tracks.map((track) => (
              <TrackRow key={track.id} track={track} artist={a.name} />
            ))}
          </ul>
        )}
        {a.recent.length > 4 && (
          <button
            type="button"
            onClick={() => setShowAll((v) => !v)}
            className="flex w-full items-center justify-center gap-1 border-t py-2.5 text-[13.5px] text-muted-foreground outline-none hover:text-foreground focus-visible:bg-accent/50"
          >
            {showAll ? "Fewer" : `${a.recent.length - 4} more`}
            <ChevronDown className={cn("size-4 transition-transform", showAll && "rotate-180")} />
          </button>
        )}
      </div>
    </section>
  )
}

function TrackRow({ track, artist }: { track: SoundcloudTrack; artist: string }) {
  const [open, setOpen] = useState(false)
  const meta = [when(track.published_at), track.duration_secs ? formatTrackTime(track.duration_secs) : null]
    .filter(Boolean)
    .join(" · ")
  return (
    <li className={cn("rounded-xl", open && "bg-accent/40")}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left outline-none hover:bg-accent/40 focus-visible:bg-accent/40"
      >
        <Cover src={track.artwork} alt="" className="size-11 shrink-0 rounded-lg" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[15px]">{track.title}</span>
          {meta && <span className="block text-[12.5px] text-muted-foreground">{meta}</span>}
        </span>
        <ChevronDown
          className={cn("size-4 shrink-0 text-muted-foreground transition-transform", open && "rotate-180")}
        />
      </button>
      {open && <TrackActions track={track} artist={artist} />}
    </li>
  )
}

/** What can be done with one track: find it, hear it, and get it if it's free. */
function TrackActions({ track, artist }: { track: SoundcloudTrack; artist: string }) {
  const me = useMe()
  const detail = useSoundcloudTrack(track.url)
  const external = useExternalSource(me.permissions.manage)
  const fetch = useFetchWithCommand()
  const free = detail.data?.free_download
  // SoundCloud's own download button can be fetched by an admin's command; other
  // free downloads sit behind the artist's own page.
  const fetchable = free && free === detail.data?.url && external.data?.enabled

  return (
    <div className="flex flex-wrap items-center gap-2 px-2.5 pt-1 pb-3 pl-[4.25rem]">
      <Button
        size="sm"
        variant="outline"
        nativeButton={false}
        render={
          <Link
            to="/"
            search={{
              q: track.title.toLowerCase().includes(artist.toLowerCase()) ? track.title : `${artist} ${track.title}`,
            }}
          />
        }
      >
        <Search /> Soulseek
      </Button>
      {detail.isPending ? (
        <span className="flex items-center gap-1.5 px-2 text-[13px] text-muted-foreground">
          <LoaderCircle className="size-3.5 animate-spin" /> Checking for a free download
        </span>
      ) : free ? (
        fetchable ? (
          <Button
            size="sm"
            disabled={fetch.isPending || fetch.isSuccess}
            onClick={() => fetch.mutate({ url: free, title: detail.data?.title ?? track.title, artist })}
          >
            {fetch.isPending ? <LoaderCircle className="animate-spin" /> : <ArrowDownToLine />}
            {fetch.isSuccess ? "Fetching for review" : "Free download"}
          </Button>
        ) : (
          <Button size="sm" nativeButton={false} render={<a href={free} target="_blank" rel="noreferrer" />}>
            <ArrowDownToLine /> Free download
          </Button>
        )
      ) : null}
      <Button
        size="sm"
        variant="ghost"
        nativeButton={false}
        render={<a href={track.url} target="_blank" rel="noreferrer" />}
      >
        Listen <ArrowUpRight />
      </Button>
    </div>
  )
}
