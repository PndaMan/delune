import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { ArrowDown, ArrowUp, Ban, Check, ChevronDown, X } from "lucide-react"
import { useMemo, useState } from "react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { useArtwork } from "@/lib/artwork"
import { formatAgo, formatBytes, formatSpeed, plural } from "@/lib/format"
import { initial } from "@/lib/session"
import {
  type HistoryPeriod,
  type TransferHistory,
  type TransferHour,
  type Upload,
  type UploadAlbum,
  type UploadPerson,
  type UploadRecord,
  sharingApi,
  useSharingStatus,
  useTransferHistory,
  useUploads,
} from "@/lib/sharing"
import { albumFromFolder, artistFromFolder, parseTrackName } from "@/lib/track-name"
import { cn } from "@/lib/utils"

const PERIODS: { id: HistoryPeriod; label: string }[] = [
  { id: "7d", label: "7 days" },
  { id: "30d", label: "30 days" },
  { id: "all", label: "All time" },
]

/** What you give back to Soulseek: the balance, who takes what, and what's going out now. */
export function UploadsPage() {
  const client = useQueryClient()
  const [period, setPeriod] = useState<HistoryPeriod>("30d")
  const history = useTransferHistory(period)
  const uploads = useUploads()
  const sharing = useSharingStatus()
  const refresh = () => void client.invalidateQueries({ queryKey: ["uploads"] })
  const cancel = useMutation({ mutationFn: sharingApi.cancel, onSuccess: refresh })
  const ban = useMutation({
    mutationFn: async (username: string) => {
      const current = sharing.data?.settings
      if (!current || current.banned.includes(username)) return
      await sharingApi.update({ ...current, banned: [...current.banned, username] })
    },
    onSuccess: () => void client.invalidateQueries({ queryKey: ["sharing"] }),
  })

  const live = useMemo(() => {
    const all = uploads.data ?? []
    return {
      running: all.filter((u) => u.status === "transferring" || u.status === "connecting"),
      queued: all.filter((u) => u.status === "queued"),
    }
  }, [uploads.data])

  const h = history.data
  const nothingYet = h && !h.all_time_uploaded_bytes && !h.recent.length && !live.running.length && !live.queued.length
  if (sharing.data && !sharing.data.settings.enabled && nothingYet) {
    return (
      <EmptyState
        illumination={0.1}
        title="You're not sharing anything yet"
        action={
          <Button
            variant="outline"
            nativeButton={false}
            render={<Link to="/settings/$section" params={{ section: "sharing" }} />}
          >
            Set up sharing
          </Button>
        }
      >
        Share your library so others can download from you. Many people on Soulseek only share with those who share
        back.
      </EmptyState>
    )
  }

  return (
    <div className="space-y-10 pb-24">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-[14px] text-muted-foreground">
          {sharing.data?.settings.enabled
            ? `Sharing ${plural(sharing.data.files, "file")} from your library`
            : "Sharing is off; nothing new goes out"}
        </p>
        <div role="radiogroup" aria-label="Period" className="flex rounded-full border bg-card/50 p-0.5">
          {PERIODS.map((p) => (
            <button
              key={p.id}
              type="button"
              role="radio"
              aria-checked={period === p.id}
              onClick={() => setPeriod(p.id)}
              className="h-8 rounded-full px-3.5 text-[13.5px] text-muted-foreground transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring aria-checked:bg-foreground aria-checked:text-background"
            >
              {p.label}
            </button>
          ))}
        </div>
      </div>

      {h ? <Balance history={h} period={period} /> : <div className="h-64 animate-pulse rounded-3xl bg-card/50" />}

      <Live running={live.running} queued={live.queued} onCancel={cancel.mutate} onBan={ban.mutate} />

      {h && (h.top_people.length > 0 || h.top_albums.length > 0) && (
        <div className="grid gap-10 lg:grid-cols-2">
          <People people={h.top_people} onBan={ban.mutate} />
          <Albums albums={h.top_albums} />
        </div>
      )}

      {h && <Recent records={h.recent} />}
    </div>
  )
}

/** The seed ratio, the totals behind it, and a chart of the period. */
function Balance({ history: h, period }: { history: TransferHistory; period: HistoryPeriod }) {
  const up = h.uploaded_bytes
  const down = h.downloaded_bytes
  const ratio = down > 0 ? up / down : up > 0 ? Infinity : null
  const verdict =
    ratio === null
      ? "Nothing has moved yet in this period."
      : ratio >= 1
        ? "You've given back more than you've taken. Thank you."
        : ratio >= 0.5
          ? "You're giving back a fair share."
          : "You've taken more than you've given. Sharing more, or longer, evens it out."
  const upShare = up + down > 0 ? up / (up + down) : 0.5

  return (
    <section className="overflow-hidden rounded-3xl border bg-card/60">
      <div className="grid gap-6 p-5 sm:p-7 md:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)] md:gap-10">
        <div className="flex flex-col">
          <p className="text-[13px] text-muted-foreground">Seed ratio</p>
          <p className="mt-1 flex items-baseline gap-2">
            <span
              className={cn(
                "type-display text-[64px] leading-none tabular-nums",
                ratio !== null && ratio >= 1 ? "text-q-lossless" : ratio !== null && ratio < 0.5 ? "text-q-hires" : "",
              )}
            >
              {ratio === null ? "—" : ratio === Infinity ? "∞" : ratio.toFixed(2)}
            </span>
          </p>
          <p className="mt-2 max-w-[34ch] text-[14px] text-pretty text-muted-foreground">{verdict}</p>

          <div className="mt-6" aria-hidden>
            <div className="flex h-2 overflow-hidden rounded-full bg-muted">
              <div className="h-full bg-primary" style={{ width: `${upShare * 100}%` }} />
              <div className="h-full bg-foreground/25" style={{ width: `${(1 - upShare) * 100}%` }} />
            </div>
          </div>
          <dl className="mt-4 grid grid-cols-2 gap-4">
            <Figure icon={<ArrowUp className="size-3.5 text-primary" />} label="Uploaded" value={formatBytes(up)} />
            <Figure icon={<ArrowDown className="size-3.5" />} label="Downloaded" value={formatBytes(down)} />
            <Figure label="Files sent" value={h.files_sent.toLocaleString()} />
            <Figure label="People" value={h.people.toLocaleString()} />
          </dl>
          {period !== "all" && (
            <p className="mt-4 text-[12.5px] text-muted-foreground">
              All time: {formatBytes(h.all_time_uploaded_bytes)} up, {formatBytes(h.all_time_downloaded_bytes)} down
            </p>
          )}
        </div>
        <Chart hours={h.hours} days={period === "7d" ? 7 : period === "30d" ? 30 : 90} />
      </div>
    </section>
  )
}

function Figure({ icon, label, value }: { icon?: React.ReactNode; label: string; value: string }) {
  return (
    <div>
      <dt className="flex items-center gap-1.5 text-[12.5px] text-muted-foreground">
        {icon}
        {label}
      </dt>
      <dd className="mt-0.5 text-[18px] font-medium tabular-nums">{value}</dd>
    </div>
  )
}

type Day = { key: string; label: string; up: number; down: number }

/** Hours grouped into the viewer's own days, the last `days` of them. */
function toDays(hours: TransferHour[], days: number): Day[] {
  const byDay = new Map<string, { up: number; down: number }>()
  for (const h of hours) {
    const key = new Date(h.hour * 1000).toLocaleDateString("en-CA")
    const day = byDay.get(key) ?? { up: 0, down: 0 }
    day.up += h.uploaded_bytes
    day.down += h.downloaded_bytes
    byDay.set(key, day)
  }
  const out: Day[] = []
  const today = new Date()
  today.setHours(12, 0, 0, 0)
  for (let i = days - 1; i >= 0; i--) {
    const date = new Date(today)
    date.setDate(today.getDate() - i)
    const key = date.toLocaleDateString("en-CA")
    const totals = byDay.get(key) ?? { up: 0, down: 0 }
    out.push({ key, label: date.toLocaleDateString(undefined, { day: "numeric", month: "short" }), ...totals })
  }
  return out
}

/** Uploads rise above the line and downloads hang below it, one bar pair a day. */
function Chart({ hours, days }: { hours: TransferHour[]; days: number }) {
  const series = useMemo(() => toDays(hours, days), [hours, days])
  const [picked, setPicked] = useState<string | null>(null)
  const peak = Math.max(1, ...series.map((d) => Math.max(d.up, d.down)))
  const shown = series.find((d) => d.key === picked) ?? null
  const width = 100 / series.length
  const gap = Math.min(width * 0.25, 0.6)

  return (
    <div className="flex min-w-0 flex-col">
      <div className="flex items-baseline justify-between gap-3 text-[13px]">
        <span className="text-muted-foreground">{shown ? shown.label : `Last ${days} days`}</span>
        {shown ? (
          <span className="tabular-nums">
            <span className="text-primary">↑ {formatBytes(shown.up)}</span>
            <span className="ml-3 text-muted-foreground">↓ {formatBytes(shown.down)}</span>
          </span>
        ) : (
          <span className="text-muted-foreground/70">Tap a day</span>
        )}
      </div>
      <div className="relative mt-3 h-48 sm:h-56" onMouseLeave={() => setPicked(null)}>
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" className="absolute inset-0 size-full" aria-hidden>
          <line x1="0" x2="100" y1="60" y2="60" className="stroke-border" strokeWidth="0.4" vectorEffect="non-scaling-stroke" />
          {series.map((d, i) => {
            const x = i * width + gap / 2
            const w = width - gap
            const upH = (d.up / peak) * 58
            const downH = (d.down / peak) * 38
            const dim = picked !== null && picked !== d.key
            return (
              <g key={d.key} opacity={dim ? 0.35 : 1}>
                {upH > 0 && <rect x={x} y={60 - upH} width={w} height={upH} rx="0.4" className="fill-primary" />}
                {downH > 0 && <rect x={x} y={60.8} width={w} height={downH} rx="0.4" className="fill-foreground/25" />}
              </g>
            )
          })}
        </svg>
        <div className="absolute inset-0 flex">
          {series.map((d) => (
            <button
              key={d.key}
              type="button"
              aria-label={`${d.label}: ${formatBytes(d.up)} up, ${formatBytes(d.down)} down`}
              onMouseEnter={() => setPicked(d.key)}
              onFocus={() => setPicked(d.key)}
              onClick={() => setPicked((current) => (current === d.key ? null : d.key))}
              className="h-full flex-1 outline-none focus-visible:bg-accent/40"
            />
          ))}
        </div>
      </div>
      <div className="mt-2 flex justify-between text-[11.5px] text-muted-foreground/70">
        <span>{series[0]?.label}</span>
        <span>Today</span>
      </div>
    </div>
  )
}

function SectionTitle({ title, count, action }: { title: string; count?: number; action?: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3 pb-3">
      <h2 className="type-title text-[19px]">{title}</h2>
      {!!count && <span className="text-sm text-muted-foreground">{count.toLocaleString()}</span>}
      <div className="ml-auto">{action}</div>
    </div>
  )
}

function Live({
  running,
  queued,
  onCancel,
  onBan,
}: {
  running: Upload[]
  queued: Upload[]
  onCancel: (id: number) => void
  onBan: (username: string) => void
}) {
  const [showQueue, setShowQueue] = useState(false)
  const waitingPeople = new Set(queued.map((u) => u.username)).size
  return (
    <section>
      <SectionTitle title="Sending now" count={running.length} />
      {running.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-5 py-6 text-center text-sm text-muted-foreground">
          Nobody is downloading from you right now.
        </p>
      ) : (
        <ul className="overflow-hidden rounded-2xl border bg-card/50">
          {running.map((u) => (
            <LiveRow key={u.id} upload={u} onCancel={onCancel} onBan={onBan} />
          ))}
        </ul>
      )}
      {queued.length > 0 && (
        <div className="mt-3">
          <button
            type="button"
            onClick={() => setShowQueue((v) => !v)}
            aria-expanded={showQueue}
            className="flex w-full items-center gap-2 rounded-xl px-1 py-2 text-left text-[14px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ChevronDown className={cn("size-4 transition-transform", showQueue && "rotate-180")} />
            {plural(queued.length, "file")} waiting, for {plural(waitingPeople, "person", "people")}
          </button>
          {showQueue && (
            <ul className="mt-1 overflow-hidden rounded-2xl border bg-card/50">
              {queued.slice(0, 200).map((u) => (
                <LiveRow key={u.id} upload={u} onCancel={onCancel} onBan={onBan} />
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  )
}

function trackOf(filename: string) {
  const parts = filename.split("\\")
  const name = parts.at(-1) ?? filename
  const album = parts.at(-2)
  return { title: parseTrackName(name).title, album: album ? albumFromFolder(album) : null }
}

function LiveRow({
  upload: u,
  onCancel,
  onBan,
}: {
  upload: Upload
  onCancel: (id: number) => void
  onBan: (username: string) => void
}) {
  const { title, album } = trackOf(u.filename)
  const progress = u.size ? Math.min(1, u.bytes / u.size) : 0
  const detail =
    u.status === "transferring"
      ? [`${formatBytes(u.bytes)} of ${formatBytes(u.size)}`, formatSpeed(u.speed)].filter(Boolean).join(" · ")
      : u.status === "connecting"
        ? "Connecting"
        : formatBytes(u.size)

  return (
    <li className="border-b px-4 py-3 last:border-b-0 sm:px-5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[15px]">{title}</p>
          <p className="truncate text-[13px] text-muted-foreground">
            <Link
              to="/soulseek/users/$username"
              params={{ username: u.username }}
              className="text-foreground/85 hover:underline"
            >
              {u.username}
            </Link>
            {album && <span> · {album}</span>}
          </p>
        </div>
        <span className="hidden shrink-0 text-[13px] text-muted-foreground tabular-nums sm:block">{detail}</span>
        <BlockButton username={u.username} onBan={onBan} />
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => onCancel(u.id)}
          aria-label="Cancel upload"
          className="text-muted-foreground"
        >
          <X />
        </Button>
      </div>
      <p className="mt-1 text-[12.5px] text-muted-foreground tabular-nums sm:hidden">{detail}</p>
      {u.status !== "queued" && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-muted">
          <div
            className="h-full bg-primary transition-[width] duration-500"
            style={{ width: `${Math.max(progress * 100, 2)}%` }}
          />
        </div>
      )}
    </li>
  )
}

/** Blocking someone takes a second tap, so a stray one doesn't. */
function BlockButton({
  username,
  onBan,
  className,
}: {
  username: string
  onBan: (username: string) => void
  className?: string
}) {
  const [asking, setAsking] = useState(false)
  if (asking) {
    return (
      <Button
        variant="destructive"
        size="sm"
        autoFocus
        onClick={() => {
          setAsking(false)
          onBan(username)
        }}
        onBlur={() => setAsking(false)}
      >
        Block
      </Button>
    )
  }
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      onClick={() => setAsking(true)}
      aria-label={`Block ${username}`}
      title={`Block ${username} from downloading`}
      className={cn("text-muted-foreground", className)}
    >
      <Ban />
    </Button>
  )
}

function People({ people, onBan }: { people: UploadPerson[]; onBan: (username: string) => void }) {
  const most = Math.max(1, ...people.map((p) => p.bytes))
  return (
    <section>
      <SectionTitle title="Who downloads from you" />
      {people.length === 0 ? (
        <p className="text-sm text-muted-foreground">Nobody in this period.</p>
      ) : (
        <ul className="space-y-1">
          {people.map((p) => (
            <li key={p.username} className="group flex items-center gap-3 rounded-xl px-2 py-2 hover:bg-accent/50">
              <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/15 text-[15px] font-semibold text-primary">
                {initial(p.username)}
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-2">
                  <Link
                    to="/soulseek/users/$username"
                    params={{ username: p.username }}
                    className="truncate text-[15px] hover:underline"
                  >
                    {p.username}
                  </Link>
                  <span className="ml-auto shrink-0 text-[13px] tabular-nums">{formatBytes(p.bytes)}</span>
                </div>
                <div className="mt-1 h-1 overflow-hidden rounded-full bg-muted">
                  <div className="h-full rounded-full bg-primary/70" style={{ width: `${(p.bytes / most) * 100}%` }} />
                </div>
                <p className="mt-1 text-[12.5px] text-muted-foreground">
                  {plural(p.files, "file")} · last {formatAgo(p.last_at)}
                </p>
              </div>
              <BlockButton
                username={p.username}
                onBan={onBan}
                className="sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100"
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function Albums({ albums }: { albums: UploadAlbum[] }) {
  return (
    <section>
      <SectionTitle title="Most wanted from you" />
      {albums.length === 0 ? (
        <p className="text-sm text-muted-foreground">Nothing in this period.</p>
      ) : (
        <ul className="space-y-1">
          {albums.map((a) => (
            <AlbumRow key={a.folder} album={a} />
          ))}
        </ul>
      )}
    </section>
  )
}

function AlbumRow({ album: a }: { album: UploadAlbum }) {
  const artist = artistFromFolder(a.parent)
  const artwork = useArtwork(artist, a.title)
  return (
    <li className="flex items-center gap-3 rounded-xl px-2 py-2 hover:bg-accent/50">
      <Cover src={artwork.data?.thumb} pending={artwork.isPending} alt="" className="size-12 rounded-lg" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px]">{artwork.data?.album ?? albumFromFolder(a.title)}</p>
        <p className="truncate text-[13px] text-muted-foreground">{artwork.data?.artist ?? artist ?? "Unknown artist"}</p>
      </div>
      <div className="shrink-0 text-right">
        <p className="text-[13px] tabular-nums">{plural(a.people, "person", "people")}</p>
        <p className="text-[12.5px] text-muted-foreground tabular-nums">
          {plural(a.files, "file")} · {formatBytes(a.bytes)}
        </p>
      </div>
    </li>
  )
}

const RECENT_STEP = 15

function Recent({ records }: { records: UploadRecord[] }) {
  const [shown, setShown] = useState(RECENT_STEP)
  if (!records.length) return null
  return (
    <section>
      <SectionTitle title="Recently" />
      <ul className="overflow-hidden rounded-2xl border bg-card/50">
        {records.slice(0, shown).map((r, i) => {
          const { title, album } = trackOf(r.filename)
          const sent = r.status === "completed"
          return (
            <li key={`${r.finished_at}-${i}`} className="flex items-center gap-3 border-b px-4 py-2.5 last:border-b-0 sm:px-5">
              <span
                className={cn(
                  "flex size-7 shrink-0 items-center justify-center rounded-full",
                  sent ? "bg-q-lossless/15 text-q-lossless" : "bg-muted text-muted-foreground",
                )}
                aria-label={sent ? "Sent" : r.status === "failed" ? "Failed" : "Stopped"}
              >
                {sent ? <Check className="size-3.5" /> : <X className="size-3.5" />}
              </span>
              <div className="min-w-0 flex-1">
                <p className="truncate text-[14.5px]">{title}</p>
                <p className="truncate text-[12.5px] text-muted-foreground">
                  <Link
                    to="/soulseek/users/$username"
                    params={{ username: r.username }}
                    className="text-foreground/85 hover:underline"
                  >
                    {r.username}
                  </Link>
                  {album && <span> · {album}</span>}
                  {!sent && <span> · {r.reason ?? (r.status === "failed" ? "failed" : "stopped")}</span>}
                </p>
              </div>
              <div className="shrink-0 text-right text-[12.5px] text-muted-foreground tabular-nums">
                <p>{formatAgo(r.finished_at)}</p>
                {sent && <p className="hidden sm:block">{formatBytes(r.size)}</p>}
              </div>
            </li>
          )
        })}
      </ul>
      {records.length > shown && (
        <Button variant="ghost" className="mt-2 w-full" onClick={() => setShown((n) => n + RECENT_STEP * 2)}>
          Show more
        </Button>
      )}
    </section>
  )
}
