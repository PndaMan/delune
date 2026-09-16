import { Link } from "@tanstack/react-router"
import { ArrowRight, Inbox } from "lucide-react"

import { Cover } from "@/components/cover"
import { useMusicViews } from "@/components/music-views"
import type { DownloadJob } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { describeJob, useDownloads } from "@/lib/downloads"
import { formatBytes, plural } from "@/lib/format"
import { formatListeningTime, useLibraryStats, useRecentAlbums } from "@/lib/library-stats"
import { useMe } from "@/lib/session"
import { albumFromFolder, artistFromFolder } from "@/lib/track-name"
import { cn } from "@/lib/utils"
import { RadarStrip } from "@/pages/radar-page"

/**
 * The home screen below the search box: what's on its way, what's waiting for you,
 * what arrived lately, and the library at a glance. Each part hides when empty.
 */
export function HomeFeed() {
  const me = useMe()
  const downloads = useDownloads()
  const jobs = downloads.data ?? []
  const active = jobs.filter((j) => j.status === "downloading" || j.status === "queued")
  const waiting = jobs.filter((j) => j.status === "ready")

  return (
    <div className="mx-auto w-full max-w-[1100px] space-y-10 px-5 pb-16">
      {waiting.length > 0 && (
        <Link
          to="/review"
          className="flex items-center gap-4 rounded-2xl border border-q-hires/30 bg-q-hires/8 px-5 py-4 outline-none transition-colors hover:bg-q-hires/12 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-q-hires/15 text-q-hires">
            <Inbox className="size-5" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-[15.5px] font-medium">
              {plural(waiting.length, "album")} waiting for review
            </span>
            <span className="block truncate text-[13.5px] text-muted-foreground">
              {waiting
                .slice(0, 3)
                .map((j) => albumFromFolder(j.title))
                .join(", ")}
              {waiting.length > 3 ? " and more" : ""}
            </span>
          </span>
          <ArrowRight className="size-5 shrink-0 text-muted-foreground" />
        </Link>
      )}

      {active.length > 0 && (
        <section>
          <Heading title="On the way" to="/downloads" link="All downloads" />
          <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {active.slice(0, 6).map((job) => (
              <ActiveCard key={job.id} job={job} />
            ))}
          </ul>
        </section>
      )}

      <RadarStrip />
      <RecentlyAdded />
      {me.permissions.search && <LibraryGlance />}
    </div>
  )
}

function Heading({ title, to, link }: { title: string; to?: string; link?: string }) {
  return (
    <div className="mb-3 flex items-baseline justify-between gap-3">
      <h2 className="type-title text-[20px]">{title}</h2>
      {to && link && (
        <Link
          to={to}
          className="flex items-center gap-1 text-[13.5px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          {link} <ArrowRight className="size-3.5" />
        </Link>
      )}
    </div>
  )
}

function ActiveCard({ job }: { job: DownloadJob }) {
  const artist = artistFromFolder(job.parent)
  const artwork = useArtwork(artist, job.title)
  const progress = job.total_bytes ? job.bytes / job.total_bytes : 0
  return (
    <li>
      <Link
        to="/downloads"
        className="flex items-center gap-3 rounded-2xl border bg-card/60 p-3 outline-none transition-colors hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Cover
          src={artwork.data?.thumb}
          pending={artwork.isPending}
          status={{ kind: "downloading", progress }}
          alt=""
          className="size-14 rounded-xl"
        />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[15px] font-medium">
            {artwork.data?.album ?? albumFromFolder(job.title)}
          </span>
          <span className="block truncate text-[13px] text-muted-foreground">{describeJob(job)}</span>
          <span className="mt-2 block h-1 overflow-hidden rounded-full bg-muted">
            <span
              className="block h-full bg-primary transition-[width] duration-700"
              style={{ width: `${Math.max(progress * 100, 2)}%` }}
            />
          </span>
        </span>
      </Link>
    </li>
  )
}

function RecentlyAdded() {
  const recent = useRecentAlbums()
  const views = useMusicViews()
  const albums = recent.data ?? []
  if (!albums.length) return null
  return (
    <section>
      <Heading title="Recently added" />
      <ul className="-mx-5 flex snap-x scroll-px-5 gap-4 overflow-x-auto px-5 pb-2 [scrollbar-width:thin]">
        {albums.map((album) => (
          <li key={album.id} className="w-[132px] shrink-0 snap-start sm:w-[152px]">
            <button
              type="button"
              onClick={() => views.openAlbum({ artist: album.artist, title: album.title })}
              className="group w-full rounded-xl text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <Cover
                src={album.cover ?? undefined}
                alt=""
                className="aspect-square w-full rounded-xl shadow-md transition-transform group-hover:scale-[1.03]"
              />
              <span className="mt-2 block truncate text-[14px] font-medium">{album.title}</span>
              <span className="block truncate text-[12.5px] text-muted-foreground">
                {[album.artist, album.year].filter(Boolean).join(" · ")}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  )
}

const TIER_COLOUR: Record<string, string> = {
  hires: "bg-q-hires",
  lossless: "bg-q-lossless",
  lossy: "bg-muted-foreground/50",
}

/** The library in one card: how much, and how much of it is lossless. */
function LibraryGlance() {
  const stats = useLibraryStats()
  const s = stats.data
  if (!s || s.songs === 0) return null
  const byTier = (tier: string) => s.qualities.filter((q) => q.tier === tier).reduce((n, q) => n + q.count, 0)
  const tiers = ["hires", "lossless", "lossy"].map((tier) => ({ tier, count: byTier(tier) }))
  const lossless = Math.round(((tiers[0].count + tiers[1].count) / s.songs) * 100)
  return (
    <section>
      <Heading title="Your library" to="/stats" link="All stats" />
      <Link
        to="/stats"
        className="block rounded-3xl border bg-card/60 p-5 outline-none transition-colors hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring sm:p-6"
      >
        <dl className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
          <Figure label="Albums" value={s.albums.toLocaleString()} />
          <Figure label="Songs" value={s.songs.toLocaleString()} />
          <Figure label="Listening" value={formatListeningTime(s.seconds)} />
          <Figure label="Size" value={formatBytes(s.bytes)} />
        </dl>
        <div className="mt-5 flex h-2.5 overflow-hidden rounded-full bg-muted" aria-hidden>
          {tiers.map((t) => (
            <span
              key={t.tier}
              className={cn("h-full", TIER_COLOUR[t.tier])}
              style={{ width: `${(t.count / s.songs) * 100}%` }}
            />
          ))}
        </div>
        <p className="mt-2 text-[13.5px] text-muted-foreground">
          {lossless}% lossless
          {tiers[0].count > 0 && <> · {plural(tiers[0].count, "hi-res song")}</>}
        </p>
      </Link>
    </section>
  )
}

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[12.5px] text-muted-foreground">{label}</dt>
      <dd className="type-title mt-0.5 text-[24px] tabular-nums">{value}</dd>
    </div>
  )
}
