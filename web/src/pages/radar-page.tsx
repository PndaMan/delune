import { useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { Bookmark, CalendarClock, Check, CircleCheck, LoaderCircle } from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { useMusicViews } from "@/components/music-views"
import { Button } from "@/components/ui/button"
import { type RadarRelease, useRadar } from "@/lib/alerts"
import { useMe } from "@/lib/session"
import { useAddToWishlist } from "@/lib/wishlist"
import { PageFrame } from "@/pages/placeholder-pages"

const DAY = 86_400_000

function when(release: RadarRelease) {
  const date = new Date(`${release.release_date}T12:00:00`)
  const days = Math.round((date.getTime() - Date.now()) / DAY)
  if (days > 0) return days === 1 ? "Out tomorrow" : days < 14 ? `Out in ${days} days` : `Out ${date.toLocaleDateString(undefined, { day: "numeric", month: "short" })}`
  if (days === 0) return "Out today"
  if (days > -7) return days === -1 ? "Yesterday" : `${-days} days ago`
  return date.toLocaleDateString(undefined, { day: "numeric", month: "short", year: date.getFullYear() === new Date().getFullYear() ? undefined : "numeric" })
}

function group(release: RadarRelease) {
  if (release.upcoming) return "Coming soon"
  const age = Date.now() - new Date(`${release.release_date}T12:00:00`).getTime()
  if (age < 7 * DAY) return "This week"
  if (age < 31 * DAY) return "This month"
  return "Earlier"
}

const GROUPS = ["Coming soon", "This week", "This month", "Earlier"]

/** New and upcoming releases from the artists you follow. */
export function RadarPage() {
  const radar = useRadar()
  const releases = radar.data ?? []

  return (
    <PageFrame title="New releases" wide>
      <p className="mt-2 max-w-[60ch] text-[15px] text-muted-foreground">
        From the artists you follow, over the last four months and what's announced. New albums and EPs go on the
        wishlist by themselves.
      </p>
      {radar.isPending ? (
        <p className="mt-8 flex items-center gap-2 text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" /> Looking up what your artists released
        </p>
      ) : releases.length === 0 ? (
        <EmptyState
          illumination={0.3}
          title="Nothing new yet"
          action={
            <Button nativeButton={false} render={<Link to="/" search={{ wishlist: true }} />}>
              Follow an artist
            </Button>
          }
        >
          Follow artists from their page or any album of theirs, and their new releases show up here.
        </EmptyState>
      ) : (
        <div className="mt-8 space-y-10 pb-24">
          {GROUPS.map((title) => {
            const items = releases.filter((r) => group(r) === title)
            if (!items.length) return null
            return (
              <section key={title}>
                <h2 className="type-title mb-4 text-[20px]">{title}</h2>
                <ul className="grid grid-cols-2 gap-x-4 gap-y-6 sm:grid-cols-3 md:grid-cols-4 xl:grid-cols-5">
                  {items.map((release) => (
                    <ReleaseCard key={`${release.id}`} release={release} />
                  ))}
                </ul>
              </section>
            )
          })}
        </div>
      )}
    </PageFrame>
  )
}

/** A few releases for the home screen. */
export function RadarStrip() {
  const radar = useRadar()
  const me = useMe()
  const releases = (radar.data ?? []).filter((r) => !r.in_library).slice(0, 12)
  if (!me.permissions.search || !releases.length) return null
  return (
    <section>
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h2 className="type-title text-[20px]">New from artists you follow</h2>
        <Link to="/radar" className="text-[13.5px] text-muted-foreground hover:text-foreground">
          All releases →
        </Link>
      </div>
      <ul className="-mx-5 flex snap-x scroll-px-5 gap-4 overflow-x-auto px-5 pb-2 [scrollbar-width:thin]">
        {releases.map((release) => (
          <ReleaseCard key={release.id} release={release} compact />
        ))}
      </ul>
    </section>
  )
}

function ReleaseCard({ release, compact }: { release: RadarRelease; compact?: boolean }) {
  const views = useMusicViews()
  const add = useAddToWishlist()
  const client = useQueryClient()
  const [wished, setWished] = useState(release.wished)
  const status = release.in_library ? (
    <span className="flex items-center gap-1 text-q-lossless">
      <CircleCheck className="size-3.5" /> In your library
    </span>
  ) : release.downloading ? (
    <span className="text-primary">Downloading</span>
  ) : wished ? (
    <span className="flex items-center gap-1 text-muted-foreground">
      <Check className="size-3.5" /> On your wishlist
    </span>
  ) : null

  return (
    <li className={compact ? "w-[132px] shrink-0 snap-start sm:w-[152px]" : "min-w-0"}>
      <button
        type="button"
        onClick={() => views.openAlbum({ artist: release.artist, title: release.title })}
        className="group block w-full rounded-xl text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <div className="relative">
          <Cover
            src={release.cover ?? undefined}
            alt=""
            status={release.in_library ? { kind: "in-library" } : undefined}
            className="aspect-square w-full rounded-xl shadow-md transition-transform group-hover:scale-[1.03]"
          />
          {release.upcoming && (
            <span className="absolute top-2 left-2 flex items-center gap-1 rounded-full bg-background/80 px-2 py-0.5 text-[11.5px] backdrop-blur">
              <CalendarClock className="size-3" /> Soon
            </span>
          )}
        </div>
        <span className="mt-2 block truncate text-[14px] font-medium">{release.title}</span>
        <span className="block truncate text-[12.5px] text-muted-foreground">
          {release.artist} · {release.kind === "ep" ? "EP" : release.kind === "single" ? "Single" : "Album"}
        </span>
        <span className="block text-[12.5px] text-muted-foreground">{when(release)}</span>
      </button>
      <div className="mt-1.5 min-h-8 text-[12.5px]">
        {status ?? (
          <Button
            variant="outline"
            size="sm"
            disabled={add.isPending}
            onClick={() =>
              add.mutate(
                { query: `${release.artist} ${release.title}` },
                {
                  onSuccess: () => {
                    setWished(true)
                    void client.invalidateQueries({ queryKey: ["radar"] })
                  },
                },
              )
            }
          >
            <Bookmark /> {release.upcoming ? "Get it when it's out" : "Wish for it"}
          </Button>
        )}
      </div>
    </li>
  )
}
