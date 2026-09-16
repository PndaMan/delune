import { Link } from "@tanstack/react-router"
import { LoaderCircle, RefreshCw } from "lucide-react"
import { useQueryClient } from "@tanstack/react-query"
import { useState } from "react"

import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { formatAgo, formatBytes, plural } from "@/lib/format"
import { formatListeningTime, type StatShare, useLibraryStats } from "@/lib/library-stats"
import { useMe } from "@/lib/session"
import { cn } from "@/lib/utils"
import { PageFrame } from "@/pages/placeholder-pages"

const TIER_COLOUR: Record<string, string> = {
  hires: "bg-q-hires",
  lossless: "bg-q-lossless",
  lossy: "bg-muted-foreground/50",
}

/** The library at a glance, and how it has grown through delune. */
export function StatsPage() {
  const stats = useLibraryStats()
  const client = useQueryClient()
  const me = useMe()
  const [refreshing, setRefreshing] = useState(false)
  const s = stats.data

  const recount = async () => {
    setRefreshing(true)
    try {
      const res = await fetch("/api/v1/stats?fresh=true")
      if (res.ok) client.setQueryData(["library", "stats"], await res.json())
    } finally {
      setRefreshing(false)
    }
  }

  if (stats.isPending) {
    return (
      <PageFrame title="Your library" wide>
        <p className="mt-8 flex items-center gap-2 text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" /> Counting everything in your library
        </p>
      </PageFrame>
    )
  }
  if (!s) {
    return (
      <PageFrame title="Your library" wide>
        <EmptyState illumination={0.1} title="Couldn't count the library">
          {stats.error?.message}
        </EmptyState>
      </PageFrame>
    )
  }

  const lossless = s.qualities.filter((q) => q.tier !== "lossy").reduce((n, q) => n + q.count, 0)
  return (
    <PageFrame title="Your library" wide>
      <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
        <p className="text-[14px] text-muted-foreground">Counted {formatAgo(s.computed_at)}</p>
        <Button variant="outline" size="sm" onClick={() => void recount()} disabled={refreshing}>
          <RefreshCw className={cn(refreshing && "animate-spin")} /> Count again
        </Button>
      </div>

      <dl className="mt-5 grid grid-cols-2 gap-px overflow-hidden rounded-3xl border bg-border sm:grid-cols-5">
        {[
          ["Albums", s.albums.toLocaleString()],
          ["Artists", s.artists.toLocaleString()],
          ["Songs", s.songs.toLocaleString()],
          ["Listening", formatListeningTime(s.seconds)],
          ["Size", formatBytes(s.bytes)],
        ].map(([label, value]) => (
          <div key={label} className="bg-card/80 px-5 py-4 last:col-span-2 sm:last:col-span-1">
            <dt className="text-[12.5px] text-muted-foreground">{label}</dt>
            <dd className="type-title mt-1 text-[26px] tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>

      <div className="mt-10 grid gap-10 pb-24 lg:grid-cols-2">
        <Card title="Sound quality" note={`${Math.round((lossless / Math.max(s.songs, 1)) * 100)}% lossless`}>
          <div className="flex h-3 overflow-hidden rounded-full bg-muted" aria-hidden>
            {s.qualities.map((q) => (
              <span
                key={q.label}
                className={cn("h-full", TIER_COLOUR[q.tier ?? "lossy"], q.label.includes("under") && "opacity-60")}
                style={{ width: `${(q.count / Math.max(s.songs, 1)) * 100}%` }}
              />
            ))}
          </div>
          <Bars items={s.qualities} unit="song" tiered showBytes />
        </Card>

        <Card title="Most albums">
          <Bars items={s.top_artists} unit="album" linkArtist />
        </Card>

        <Card title="By decade">
          <Columns items={s.decades} />
        </Card>

        {s.genres.length > 0 && (
          <Card title="Genres">
            <Bars items={s.genres} unit="song" />
          </Card>
        )}

        {s.imports.length > 0 && (
          <Card title="Added through delune" note={plural(s.imports.reduce((n, m) => n + m.count, 0), "album")}>
            <Columns items={s.imports.slice(-12).map((m) => ({ ...m, label: monthLabel(m.label) }))} />
          </Card>
        )}

        {s.importers.length > 1 && (
          <Card title="Who added the most">
            <Bars items={s.importers} unit="album" showBytes />
          </Card>
        )}

        {me.permissions.manage && s.top_peers.length > 0 && (
          <Card title="Downloaded most from">
            <Bars items={s.top_peers} unit="file" showBytes linkPeer />
          </Card>
        )}
      </div>
    </PageFrame>
  )
}

function monthLabel(month: string) {
  const [year, m] = month.split("-").map(Number)
  return new Date(year, m - 1, 1).toLocaleDateString(undefined, { month: "short", year: "2-digit" })
}

function Card({ title, note, children }: { title: string; note?: string; children: React.ReactNode }) {
  return (
    <section className="rounded-3xl border bg-card/50 p-5 sm:p-6">
      <div className="mb-4 flex items-baseline justify-between gap-3">
        <h2 className="type-title text-[19px]">{title}</h2>
        {note && <span className="text-[13.5px] text-muted-foreground">{note}</span>}
      </div>
      {children}
    </section>
  )
}

function Bars({
  items,
  unit,
  tiered,
  showBytes,
  linkArtist,
  linkPeer,
}: {
  items: StatShare[]
  unit: string
  tiered?: boolean
  showBytes?: boolean
  linkArtist?: boolean
  linkPeer?: boolean
}) {
  const most = Math.max(1, ...items.map((i) => i.count))
  return (
    <ul className="mt-3 space-y-3">
      {items.map((item) => {
        const label = linkArtist ? (
          <Link to="/artist/$name" params={{ name: item.label }} className="hover:underline">
            {item.label}
          </Link>
        ) : linkPeer ? (
          <Link to="/soulseek/users/$username" params={{ username: item.label }} className="hover:underline">
            {item.label}
          </Link>
        ) : (
          item.label
        )
        return (
          <li key={item.label}>
            <div className="flex items-baseline gap-3 text-[14.5px]">
              <span className="min-w-0 flex-1 truncate">{label}</span>
              <span className="shrink-0 text-[13px] text-muted-foreground tabular-nums">
                {plural(item.count, unit)}
                {showBytes && item.bytes > 0 ? ` · ${formatBytes(item.bytes)}` : ""}
              </span>
            </div>
            <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className={cn("h-full rounded-full", tiered ? TIER_COLOUR[item.tier ?? "lossy"] : "bg-primary/75")}
                style={{ width: `${(item.count / most) * 100}%` }}
              />
            </div>
          </li>
        )
      })}
    </ul>
  )
}

/** Vertical bars with labels underneath, for things in order (decades, months). */
function Columns({ items }: { items: StatShare[] }) {
  const most = Math.max(1, ...items.map((i) => i.count))
  return (
    <div className="flex h-44 items-end gap-1.5 sm:gap-2">
      {items.map((item) => (
        <div key={item.label} className="flex h-full min-w-0 flex-1 flex-col items-center justify-end gap-1.5">
          <span className="text-[11.5px] text-muted-foreground tabular-nums">{item.count}</span>
          <div
            className="w-full max-w-10 rounded-t-md bg-primary/75"
            style={{ height: `${Math.max((item.count / most) * 100, 3)}%` }}
            title={`${item.label}: ${item.count}`}
          />
          <span className="w-full truncate text-center text-[11px] text-muted-foreground">{item.label}</span>
        </div>
      ))}
    </div>
  )
}
