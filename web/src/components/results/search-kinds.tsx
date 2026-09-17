import { Link } from "@tanstack/react-router"
import { ChevronRight, Disc3, Mic2, Music2, Sparkles } from "lucide-react"
import { memo, useMemo } from "react"

import { Cover } from "@/components/cover"
import { useMusicViews } from "@/components/music-views"
import type { Candidate } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { formatBytes, formatSpeed, plural } from "@/lib/format"
import { type AlbumHit, type ArtistHit, useArtist } from "@/lib/music"
import type { TrackHit } from "@/lib/search-kinds"
import { TIER_TEXT, tierOf } from "@/lib/quality"
import { isSceneName } from "@/lib/track-name"
import { cn } from "@/lib/utils"

export type SearchKind = "all" | "artists" | "albums" | "tracks"

const KINDS: { id: SearchKind; label: string; icon: typeof Music2 }[] = [
  { id: "all", label: "All", icon: Sparkles },
  { id: "artists", label: "Artists", icon: Mic2 },
  { id: "albums", label: "Albums", icon: Disc3 },
  { id: "tracks", label: "Tracks", icon: Music2 },
]

export function KindToggle({
  kind,
  onKind,
  counts,
}: {
  kind: SearchKind
  onKind: (kind: SearchKind) => void
  counts: Partial<Record<SearchKind, number>>
}) {
  return (
    <div
      role="tablist"
      aria-label="What to show"
      className="-mx-1 flex gap-1.5 overflow-x-auto px-1 pb-1 [scrollbar-width:none]"
    >
      {KINDS.map(({ id, label, icon: Icon }) => {
        const active = kind === id
        const count = counts[id]
        return (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onKind(id)}
            className={cn(
              "flex h-9 shrink-0 items-center gap-1.5 rounded-full border px-3 text-[14px] font-medium outline-none transition-colors sm:px-3.5",
              "focus-visible:ring-2 focus-visible:ring-ring",
              active
                ? "border-primary/50 bg-primary text-primary-foreground"
                : "border-border bg-card/60 text-muted-foreground hover:text-foreground",
            )}
          >
            <Icon className="size-4 max-sm:hidden" />
            {label}
            {count !== undefined && id !== "all" && (
              <span className={cn("tabular-nums", active ? "opacity-80" : "opacity-60")}>{count}</span>
            )}
          </button>
        )
      })}
    </div>
  )
}

/** The artist a search names, up top: their picture, and the way to their page. */
export function ArtistBubble({ artist }: { artist: ArtistHit }) {
  // Found through an album: look the artist up for their picture.
  const info = useArtist(artist.picture ? "" : artist.name)
  const picture = artist.picture ?? info.data?.picture
  const fans = artist.listeners ?? info.data?.listeners
  return (
    <Link
      to="/artist/$name"
      params={{ name: artist.name }}
      className="group flex w-full items-center gap-4 rounded-full border bg-card/60 py-2 pr-5 pl-2 outline-none transition-colors hover:border-primary/50 hover:bg-primary/10 focus-visible:ring-2 focus-visible:ring-ring sm:w-auto sm:max-w-md"
    >
      <Cover
        src={picture ?? undefined}
        pending={!artist.picture && info.isPending}
        alt=""
        className="size-14 shrink-0 rounded-full shadow-md"
      />
      <span className="min-w-0 flex-1">
        <span className="block text-[12px] font-medium tracking-wide text-muted-foreground uppercase">Artist</span>
        <span className="block truncate text-[18px] font-semibold">{artist.name}</span>
        {fans ? (
          <span className="block text-[12.5px] text-muted-foreground">{fans.toLocaleString()} fans on Deezer</span>
        ) : null}
      </span>
      <ChevronRight className="size-5 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
    </Link>
  )
}

/** Every artist the search could mean. */
export function ArtistGrid({ artists }: { artists: ArtistHit[] }) {
  if (!artists.length) return <p className="px-1 py-10 text-muted-foreground">No artists by that name.</p>
  return (
    <ul className="grid grid-cols-3 gap-x-3 gap-y-6 sm:grid-cols-4 md:grid-cols-6">
      {artists.map((artist) => (
        <li key={artist.name} className="min-w-0">
          <Link
            to="/artist/$name"
            params={{ name: artist.name }}
            className="group block rounded-xl text-center outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Cover
              src={artist.picture ?? undefined}
              alt=""
              className="mx-auto aspect-square w-full rounded-full shadow-md transition-transform group-hover:scale-[1.03]"
            />
            <span className="mt-2 block truncate text-[14px] font-medium">{artist.name}</span>
            {artist.listeners ? (
              <span className="block truncate text-[12px] text-muted-foreground">
                {artist.listeners.toLocaleString()} fans
              </span>
            ) : null}
          </Link>
        </li>
      ))}
    </ul>
  )
}

/** Albums from Deezer: what exists, opened with its tracklist, whoever shares it. */
export function AlbumStrip({ albums, title }: { albums: AlbumHit[]; title: string }) {
  const views = useMusicViews()
  if (!albums.length) return null
  return (
    <section>
      <h2 className="mb-3 text-[13px] font-semibold tracking-wide text-muted-foreground uppercase">{title}</h2>
      <ul className="-mx-4 flex snap-x scroll-px-4 gap-3 overflow-x-auto px-4 pb-2 [scrollbar-width:thin] sm:-mx-8 sm:scroll-px-8 sm:px-8">
        {albums.map((album) => (
          <li key={`${album.artist}${album.title}`} className="w-[120px] shrink-0 snap-start sm:w-[140px]">
            <button
              type="button"
              onClick={() => views.openAlbum({ artist: album.artist, title: album.title })}
              className="group block w-full rounded-xl text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <Cover
                src={album.cover ?? undefined}
                alt=""
                className="aspect-square w-full rounded-xl shadow-md transition-transform group-hover:scale-[1.03]"
              />
              <span className="mt-1.5 block truncate text-[13.5px] font-medium">{album.title}</span>
              <span className="block truncate text-[12px] text-muted-foreground">
                {[album.artist, album.year].filter(Boolean).join(" · ")}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

/** Songs, one row per person sharing them. */
export function TrackList({
  hits,
  onOpen,
  limit,
  onMore,
}: {
  hits: TrackHit[]
  onOpen: (candidate: Candidate) => void
  limit?: number
  onMore?: () => void
}) {
  const shown = useMemo(() => (limit ? hits.slice(0, limit) : hits), [hits, limit])
  if (!hits.length) {
    return (
      <p className="px-1 py-10 text-muted-foreground">
        No single songs by that name yet. Search the song's title with the artist, like “Fred again Jungle”.
      </p>
    )
  }
  return (
    <div>
      <ul className="divide-y divide-border/60 rounded-2xl border bg-card/40">
        {shown.map((hit) => (
          <TrackRow key={hit.id} hit={hit} onOpen={onOpen} />
        ))}
      </ul>
      {onMore && limit && hits.length > limit && (
        <button
          type="button"
          onClick={onMore}
          className="mt-2 flex items-center gap-1 px-1 text-[13.5px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          All {plural(hits.length, "copy", "copies")} of these songs <ChevronRight className="size-4" />
        </button>
      )}
    </div>
  )
}

const TrackRow = memo(function TrackRow({ hit, onOpen }: { hit: TrackHit; onOpen: (c: Candidate) => void }) {
  const { album, file } = hit
  const artwork = useArtwork(album.parent, album.title)
  const where =
    isSceneName(album.title) && artwork.data ? [artwork.data.artist, artwork.data.album] : [album.parent, album.title]
  const tier = tierOf(file.quality)
  const speed = formatSpeed(album.avg_speed)
  return (
    <li>
      <button
        type="button"
        onClick={() => onOpen(hit.candidate)}
        className="flex w-full items-center gap-3 px-3 py-2.5 text-left outline-none transition-colors first:rounded-t-2xl last:rounded-b-2xl hover:bg-accent/60 focus-visible:bg-accent"
      >
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} alt="" className="size-12 shrink-0 rounded-lg" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[15px] font-medium">{hit.title}</span>
          <span className="block truncate text-[12.5px] text-muted-foreground">
            {where.filter(Boolean).join(" · ")}
          </span>
          <span className="block truncate text-[12px] text-muted-foreground/80">
            {album.username}
            {album.free_slot ? " · ready now" : album.queue_length ? ` · ${album.queue_length} waiting` : ""}
            {speed ? ` · ${speed}` : ""}
          </span>
        </span>
        <span className="shrink-0 text-right text-[12.5px]">
          <span className={cn("block", TIER_TEXT[tier])}>{file.quality_label ?? "Unknown"}</span>
          <span className="block text-muted-foreground">{formatBytes(file.size)}</span>
        </span>
      </button>
    </li>
  )
})
