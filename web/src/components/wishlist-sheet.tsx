import { Dialog } from "@base-ui/react/dialog"
import { Link } from "@tanstack/react-router"
import { Bell, BellOff, Check, CircleCheck, Inbox, LoaderCircle, Pause, Play, Search, Trash2, X } from "lucide-react"
import { useDeferredValue, useState } from "react"

import { Cover } from "@/components/cover"
import { useMusicViews } from "@/components/music-views"
import { Button } from "@/components/ui/button"
import { useFollow, useFollows, useUnfollow } from "@/lib/automation"
import { plural } from "@/lib/format"
import { type AlbumHit, type ArtistHit, useMusicSearch } from "@/lib/music"
import {
  MIN_QUALITY_LABELS,
  type MinQuality,
  type WishlistItem,
  useAddToWishlist,
  useRemoveFromWishlist,
  useUpdateWishlist,
  useWishlist,
} from "@/lib/wishlist"
import { cn } from "@/lib/utils"

/**
 * Everything delune is watching for, in one place: search for an artist or album,
 * add it, and see how the hunt is going. Opened from the search bar.
 */
export function WishlistSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [query, setQuery] = useState("")
  const search = useMusicSearch(useDeferredValue(query))
  const wishlist = useWishlist()
  const follows = useFollows()
  const items = wishlist.data ?? []
  const waiting = items.filter((item) => !item.download_id && !item.paused)
  const found = items.filter((item) => item.download_id)
  const paused = items.filter((item) => item.paused && !item.download_id)
  const typing = query.trim().length >= 2

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup className="fixed inset-x-0 bottom-0 z-50 flex h-[92dvh] flex-col overflow-hidden rounded-t-3xl border-t bg-card outline-none transition-transform duration-200 data-ending-style:translate-y-full data-starting-style:translate-y-full sm:inset-0 sm:m-auto sm:h-[min(88dvh,820px)] sm:w-[min(94vw,680px)] sm:rounded-3xl sm:border sm:shadow-[0_40px_120px_-20px_rgb(0_0_0/0.8)] sm:data-ending-style:translate-y-0 sm:data-starting-style:translate-y-0">
          <header className="border-b p-5 pb-4 sm:p-6 sm:pb-4">
            <div className="flex items-center gap-3">
              <Dialog.Title className="type-title flex-1 text-[22px]">Wishlist</Dialog.Title>
              <Dialog.Close
                aria-label="Close"
                className="flex size-9 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                <X className="size-4" />
              </Dialog.Close>
            </div>
            <p className="mt-1 text-[14px] text-muted-foreground">
              delune keeps looking, and downloads a good copy for review as soon as one turns up.
            </p>
            <div className="relative mt-4">
              <Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="An artist or album to watch for"
                aria-label="Search for an artist or album"
                autoComplete="off"
                className="h-12 w-full rounded-xl border bg-background/60 pr-10 pl-10 text-[15.5px] outline-none focus:border-primary/50"
              />
              {query && (
                <button
                  type="button"
                  onClick={() => setQuery("")}
                  aria-label="Clear"
                  className="absolute top-1/2 right-2 flex size-8 -translate-y-1/2 items-center justify-center rounded-lg text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <X className="size-4" />
                </button>
              )}
            </div>
          </header>

          <div className="scrollbar-themed min-h-0 flex-1 overflow-y-auto px-2 pb-[max(1rem,env(safe-area-inset-bottom))] sm:px-3">
            {typing ? (
              <Results
                search={search.data}
                pending={search.isFetching}
                failed={search.isError}
                query={query}
                onAdded={() => setQuery("")}
              />
            ) : (
              <>
                <Group title="Waiting" count={waiting.length}>
                  {waiting.map((item) => (
                    <WishRow key={item.id} item={item} />
                  ))}
                </Group>
                <Group title="Found" count={found.length}>
                  {found.map((item) => (
                    <WishRow key={item.id} item={item} />
                  ))}
                </Group>
                <Group title="Paused" count={paused.length}>
                  {paused.map((item) => (
                    <WishRow key={item.id} item={item} />
                  ))}
                </Group>
                <Group title="Following" count={follows.data?.length ?? 0}>
                  {(follows.data ?? []).map((follow) => (
                    <FollowRow key={follow.deezer_id} follow={follow} onClose={onClose} />
                  ))}
                </Group>
                {!items.length && !follows.data?.length && (
                  <p className="px-4 py-10 text-center text-[15px] text-muted-foreground">
                    Nothing on the list yet. Search above for an album to watch for, or an artist to follow.
                  </p>
                )}
              </>
            )}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Group({ title, count, children }: { title: string; count: number; children: React.ReactNode }) {
  if (!count) return null
  return (
    <section className="pt-4">
      <h3 className="px-3 pb-1.5 text-[12.5px] tracking-wide text-muted-foreground uppercase">
        {title} <span className="text-muted-foreground/60">{count}</span>
      </h3>
      <ul>{children}</ul>
    </section>
  )
}

/** What matched what someone typed: artists to follow, albums to watch for. */
function Results({
  search,
  pending,
  failed,
  query,
  onAdded,
}: {
  search: { artists: ArtistHit[]; albums: AlbumHit[] } | undefined
  pending: boolean
  failed: boolean
  query: string
  onAdded: () => void
}) {
  const add = useAddToWishlist()
  const nothing = search && !search.artists.length && !search.albums.length

  return (
    <div className="pb-4">
      {search?.artists.length ? (
        <Group title="Artists" count={search.artists.length}>
          {search.artists.map((artist) => (
            <ArtistResult key={artist.name} artist={artist} />
          ))}
        </Group>
      ) : null}
      {search?.albums.length ? (
        <Group title="Albums" count={search.albums.length}>
          {search.albums.map((album) => (
            <AlbumResult key={`${album.artist}-${album.title}`} album={album} onAdded={onAdded} />
          ))}
        </Group>
      ) : null}
      {pending && !search && (
        <div className="space-y-2 p-3">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-14 animate-pulse rounded-xl bg-muted/40" />
          ))}
        </div>
      )}
      {failed && !search && (
        <div className="px-4 py-8 text-center">
          <p className="text-[15px] text-muted-foreground">
            Couldn't reach the catalogue delune looks names up in. You can still watch for it by name.
          </p>
          <Button
            variant="outline"
            className="mt-3"
            disabled={add.isPending}
            onClick={() => add.mutate({ query: query.trim() }, { onSuccess: onAdded })}
          >
            Watch for “{query.trim()}”
          </Button>
        </div>
      )}
      {nothing && !pending && (
        <div className="px-4 py-8 text-center">
          <p className="text-[15px] text-muted-foreground">Nothing found for “{query}”.</p>
          <Button
            variant="outline"
            className="mt-3"
            disabled={add.isPending}
            onClick={() => add.mutate({ query: query.trim() }, { onSuccess: onAdded })}
          >
            Watch for “{query.trim()}” anyway
          </Button>
        </div>
      )}
    </div>
  )
}

function ArtistResult({ artist }: { artist: ArtistHit }) {
  const follows = useFollows()
  const follow = useFollow()
  const unfollow = useUnfollow()
  const following = (follows.data ?? []).find((f) => f.artist.toLowerCase() === artist.name.toLowerCase())
  return (
    <li className="flex items-center gap-3 rounded-xl px-3 py-2">
      <Cover src={artist.picture ?? undefined} alt="" className="size-11 rounded-full" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px]">{artist.name}</p>
        <p className="text-[13px] text-muted-foreground">
          {artist.listeners ? `${artist.listeners.toLocaleString()} followers` : "Artist"}
        </p>
      </div>
      {following ? (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => unfollow.mutate(following.deezer_id)}
          disabled={unfollow.isPending}
        >
          <BellOff /> Following
        </Button>
      ) : (
        <Button variant="outline" size="sm" onClick={() => follow.mutate(artist.name)} disabled={follow.isPending}>
          {follow.isPending ? <LoaderCircle className="animate-spin" /> : <Bell />} Follow
        </Button>
      )}
    </li>
  )
}

function AlbumResult({ album, onAdded }: { album: AlbumHit; onAdded: () => void }) {
  const add = useAddToWishlist()
  const wishlist = useWishlist()
  const views = useMusicViews()
  const query = `${album.artist} ${album.title}`
  const already = (wishlist.data ?? []).some((item) => item.query.toLowerCase() === query.toLowerCase())
  return (
    <li className="flex items-center gap-3 rounded-xl px-3 py-2">
      <button
        type="button"
        onClick={() => views.openAlbum({ artist: album.artist, title: album.title })}
        aria-label={`About ${album.title}`}
        className="rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Cover src={album.cover ?? undefined} alt="" className="size-11 rounded-lg" />
      </button>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px]">{album.title}</p>
        <p className="truncate text-[13px] text-muted-foreground">
          {album.artist}
          {album.track_count ? `, ${plural(album.track_count, "track")}` : ""}
        </p>
      </div>
      {already ? (
        <span className="flex items-center gap-1.5 pr-2 text-[13px] text-q-lossless">
          <Check className="size-4" /> On the list
        </span>
      ) : (
        <Button
          variant="outline"
          size="sm"
          disabled={add.isPending}
          onClick={() => add.mutate({ query }, { onSuccess: onAdded })}
        >
          {add.isPending ? <LoaderCircle className="animate-spin" /> : null} Watch for it
        </Button>
      )}
    </li>
  )
}

function WishRow({ item }: { item: WishlistItem }) {
  const update = useUpdateWishlist()
  const remove = useRemoveFromWishlist()
  const views = useMusicViews()
  const best = item.best
  const status = item.download_id
    ? "Downloading for review"
    : item.paused
      ? "Paused"
      : item.last_searched
        ? item.last_matches
          ? `${plural(item.last_matches, "copy", "copies")} found, waiting for a good one`
          : "Nobody sharing it yet"
        : "Searching soon"

  return (
    <li className="flex items-center gap-3 rounded-xl px-3 py-2 hover:bg-accent/40">
      <button
        type="button"
        onClick={() => views.openAlbum({ artist: best?.parent ?? null, title: best?.title ?? item.query })}
        aria-label={`About ${item.query}`}
        className="rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Cover src={undefined} alt="" className="size-11 rounded-lg" />
      </button>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px]">{item.query}</p>
        <p
          className={cn(
            "truncate text-[13px]",
            item.download_id ? "text-q-lossless" : item.paused ? "text-muted-foreground/70" : "text-muted-foreground",
          )}
        >
          {status}
          {item.track && <span className="text-muted-foreground/70"> · one song</span>}
        </p>
      </div>
      <select
        value={item.min_quality}
        onChange={(e) => update.mutate({ id: item.id, min_quality: e.target.value as MinQuality })}
        aria-label={`Quality for ${item.query}`}
        className="hidden h-8 rounded-lg border bg-background/50 px-2 text-[13px] outline-none sm:block"
      >
        {Object.entries(MIN_QUALITY_LABELS).map(([value, label]) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
      {item.download_id ? (
        <Button
          variant="ghost"
          size="icon"
          nativeButton={false}
          render={<Link to="/downloads" />}
          aria-label="See the download"
        >
          <Inbox />
        </Button>
      ) : (
        <Button
          variant="ghost"
          size="icon"
          aria-label={item.paused ? "Resume searching" : "Pause searching"}
          onClick={() => update.mutate({ id: item.id, paused: !item.paused })}
          disabled={update.isPending}
        >
          {item.paused ? <Play /> : <Pause />}
        </Button>
      )}
      <Button
        variant="ghost"
        size="icon"
        aria-label={`Remove ${item.query}`}
        onClick={() => remove.mutate(item.id)}
        disabled={remove.isPending}
      >
        <Trash2 />
      </Button>
    </li>
  )
}

function FollowRow({
  follow,
  onClose,
}: {
  follow: { artist: string; picture: string | null; deezer_id: number; seen: number[] }
  onClose: () => void
}) {
  const unfollow = useUnfollow()
  return (
    <li className="flex items-center gap-3 rounded-xl px-3 py-2 hover:bg-accent/40">
      <Cover src={follow.picture ?? undefined} alt="" className="size-11 rounded-full" />
      <Link
        to="/artist/$name"
        params={{ name: follow.artist }}
        onClick={onClose}
        className="min-w-0 flex-1 outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <p className="truncate text-[15px] underline-offset-4 hover:underline">{follow.artist}</p>
        <p className="text-[13px] text-muted-foreground">
          {follow.seen.length
            ? `${plural(follow.seen.length, "release")} picked up so far`
            : "Watching for new releases"}
        </p>
      </Link>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => unfollow.mutate(follow.deezer_id)}
        disabled={unfollow.isPending}
        className="text-muted-foreground"
      >
        <CircleCheck /> Following
      </Button>
    </li>
  )
}
