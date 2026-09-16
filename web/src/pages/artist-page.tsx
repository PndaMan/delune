import { Link } from "@tanstack/react-router"
import { CircleCheck, Search } from "lucide-react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { useMusicViews } from "@/components/music-views"
import { Button } from "@/components/ui/button"
import { useFollow, useFollows, useUnfollow } from "@/lib/automation"
import { plural } from "@/lib/format"
import { type ArtistAlbum, useArtist } from "@/lib/music"
import { useMe } from "@/lib/session"
import { cn } from "@/lib/utils"
import { PageFrame } from "@/pages/placeholder-pages"

/** One artist: what your library has by them, and everything else they've released. */
export function ArtistPage({ name }: { name: string }) {
  const artist = useArtist(name)
  const me = useMe()
  const views = useMusicViews()
  const follows = useFollows()
  const follow = useFollow()
  const unfollow = useUnfollow()
  const following = (follows.data ?? []).find((f) => f.artist.toLowerCase() === name.toLowerCase())

  if (artist.isError) {
    return (
      <PageFrame title={name} wide>
        <EmptyState
          illumination={0.2}
          title={`Nothing found for “${name}”`}
          action={
            <Button nativeButton={false} render={<Link to="/" search={{ q: name }} />}>
              Search Soulseek anyway
            </Button>
          }
        >
          delune looks artists up on Deezer's public catalogue. A different spelling might find them.
        </EmptyState>
      </PageFrame>
    )
  }

  const info = artist.data
  const releases = (info?.albums ?? []).filter((album) => album.kind === "album")
  const rest = (info?.albums ?? []).filter((album) => album.kind !== "album")

  return (
    <PageFrame title={info?.name ?? name} wide>
      <div className="flex items-start gap-4 sm:gap-5">
        <Cover
          src={info?.picture ?? undefined}
          pending={artist.isPending}
          alt=""
          className="size-16 shrink-0 rounded-full sm:size-20"
        />
        <div className="min-w-0 flex-1">
          <p className="text-[15px] text-pretty text-muted-foreground">
            {[
              info?.in_library.length ? `${plural(info.in_library.length, "album")} in your library` : null,
              info?.albums.length ? `${info.albums.length} released` : null,
              info?.listeners ? `${info.listeners.toLocaleString()} followers on Deezer` : null,
            ]
              .filter(Boolean)
              .join(" · ") || (artist.isPending ? "Looking them up" : "")}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button nativeButton={false} render={<Link to="/" search={{ q: info?.name ?? name }} />} variant="outline">
              <Search /> Search Soulseek
            </Button>
            {me.permissions.download &&
              (following ? (
                <Button
                  variant="outline"
                  onClick={() => unfollow.mutate(following.deezer_id)}
                  disabled={unfollow.isPending}
                >
                  Following
                </Button>
              ) : (
                <Button variant="outline" onClick={() => follow.mutate(info?.name ?? name)} disabled={follow.isPending}>
                  Follow for new releases
                </Button>
              ))}
          </div>
        </div>
      </div>

      {!!info?.in_library.length && (
        <Section title="In your library">
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
            {info.in_library.map((album) => (
              <AlbumTile
                key={album.id}
                artist={info.name}
                title={album.title}
                year={album.year}
                owned
                onOpen={() => views.openAlbum({ artist: info.name, title: album.title })}
              />
            ))}
          </div>
        </Section>
      )}

      {artist.isPending ? (
        <div className="mt-10 grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="aspect-square animate-pulse rounded-2xl bg-muted/40" />
          ))}
        </div>
      ) : (
        <>
          {!!releases.length && (
            <Section title="Albums">
              <Discography albums={releases} artist={info?.name ?? name} onOpen={views.openAlbum} />
            </Section>
          )}
          {!!rest.length && (
            <Section title="Singles and EPs">
              <Discography albums={rest} artist={info?.name ?? name} onOpen={views.openAlbum} />
            </Section>
          )}
        </>
      )}
    </PageFrame>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mt-10">
      <h2 className="type-title mb-4 text-[21px]">{title}</h2>
      {children}
    </section>
  )
}

function Discography({
  albums,
  artist,
  onOpen,
}: {
  albums: ArtistAlbum[]
  artist: string
  onOpen: (album: { artist: string; title: string }) => void
}) {
  return (
    <div className="grid grid-cols-2 gap-4 pb-6 sm:grid-cols-3 lg:grid-cols-4">
      {albums.map((album) => (
        <AlbumTile
          key={`${album.title}-${album.year}`}
          artist={artist}
          title={album.title}
          year={album.year}
          cover={album.cover}
          owned={album.in_library}
          onOpen={() => onOpen({ artist, title: album.title })}
        />
      ))}
    </div>
  )
}

function AlbumTile({
  artist,
  title,
  year,
  cover,
  owned,
  onOpen,
}: {
  artist: string
  title: string
  year: number | null
  cover?: string | null
  owned?: boolean
  onOpen: () => void
}) {
  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={`${title} by ${artist}`}
      className="group rounded-2xl p-2 text-left outline-none transition-colors hover:bg-card/70 focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Cover src={cover ?? undefined} alt="" className="aspect-square w-full rounded-xl" />
      <p className={cn("mt-2 line-clamp-2 text-[14.5px]", owned && "text-q-lossless")}>{title}</p>
      <p className="mt-0.5 flex items-center gap-1 text-[13px] text-muted-foreground">
        {year ?? ""}
        {owned && (
          <>
            <CircleCheck className="size-3.5 text-q-lossless" />
            <span className="text-q-lossless">In library</span>
          </>
        )}
      </p>
    </button>
  )
}
