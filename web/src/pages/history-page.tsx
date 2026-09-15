import { Link } from "@tanstack/react-router"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import type { DownloadJob } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { useDownloads } from "@/lib/downloads"
import { type MusicRequest, REQUEST_STATUS, useRequests } from "@/lib/requests"
import { useMe } from "@/lib/session"
import { cn } from "@/lib/utils"
import { PageFrame } from "@/pages/placeholder-pages"

type Entry = {
  key: string
  at: number
  who: string | null
  title: string
  artist: string | null
  what: string
  tone: "good" | "bad" | "plain"
  link: "/downloads" | "/review"
}

const FILTERS = [
  ["all", "Everything"],
  ["imported", "Added to the library"],
  ["requests", "Requests"],
  ["problems", "Didn't work out"],
] as const
type Filter = (typeof FILTERS)[number][0]

function fromJob(job: DownloadJob): Entry {
  const imported = job.status === "imported"
  return {
    key: `job-${job.id}`,
    at: job.imported_at ?? job.created_at,
    who: job.requested_by,
    title: job.title,
    artist: job.parent,
    what: imported
      ? `Added to the library${job.imported_to ? ` in ${job.imported_to}` : ""}`
      : job.status === "failed"
        ? "Download failed"
        : job.status === "cancelled"
          ? "Download stopped"
          : job.status === "ready"
            ? "Downloaded, waiting in review"
            : "Downloading",
    tone: imported ? "good" : job.status === "failed" || job.status === "cancelled" ? "bad" : "plain",
    link: job.status === "ready" || imported ? "/review" : "/downloads",
  }
}

function fromRequest(request: MusicRequest): Entry {
  return {
    key: `request-${request.id}`,
    at: request.requested_at,
    who: request.requested_by,
    title: request.title,
    artist: request.artist,
    what: `Requested. ${REQUEST_STATUS[request.status]}${request.reason ? `: ${request.reason}` : ""}`,
    tone:
      request.status === "available"
        ? "good"
        : request.status === "declined" || request.status === "failed"
          ? "bad"
          : "plain",
    link: "/downloads",
  }
}

/** Everything someone asked for, downloaded and added to the library, newest first. */
export function HistoryPage() {
  const me = useMe()
  const downloads = useDownloads()
  const requests = useRequests()
  const [filter, setFilter] = useState<Filter>("all")
  const [person, setPerson] = useState<string>(me.permissions.manage ? "everyone" : me.username)

  const entries = [
    ...(downloads.data ?? []).map(fromJob).filter(() => filter !== "requests"),
    ...(requests.data ?? []).map(fromRequest).filter(() => filter === "all" || filter === "requests"),
  ]
    .filter((e) => filter !== "imported" || e.what.startsWith("Added"))
    .filter((e) => filter !== "problems" || e.tone === "bad")
    .filter((e) => person === "everyone" || e.who === person)
    .sort((a, b) => b.at - a.at)

  const people = [
    ...new Set([
      ...(downloads.data ?? []).map((j) => j.requested_by),
      ...(requests.data ?? []).map((r) => r.requested_by),
    ]),
  ]
    .filter((p): p is string => !!p)
    .sort()

  return (
    <PageFrame title="History" wide>
      <div className="mt-4 flex flex-wrap items-center gap-2">
        {FILTERS.map(([id, label]) => (
          <button
            key={id}
            type="button"
            aria-pressed={filter === id}
            onClick={() => setFilter(id)}
            className="h-9 rounded-full border px-3.5 text-[14px] text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-pressed:border-transparent aria-pressed:bg-foreground aria-pressed:text-background"
          >
            {label}
          </button>
        ))}
        {me.permissions.manage && people.length > 1 && (
          <select
            value={person}
            onChange={(e) => setPerson(e.target.value)}
            aria-label="Whose history"
            className="ml-auto h-9 rounded-full border bg-card/60 px-3 text-[14px] outline-none"
          >
            <option value="everyone">Everyone</option>
            {people.map((p) => (
              <option key={p} value={p}>
                {p === me.username ? "You" : p}
              </option>
            ))}
          </select>
        )}
      </div>

      {downloads.isPending || requests.isPending ? (
        <div className="mt-8 h-40 animate-pulse rounded-2xl bg-muted/40" />
      ) : entries.length === 0 ? (
        <EmptyState
          illumination={0.2}
          title="Nothing here yet"
          action={
            <Button nativeButton={false} render={<Link to="/" search={{}} />}>
              Find an album
            </Button>
          }
        >
          Albums you download, request and add to the library show up here.
        </EmptyState>
      ) : (
        <ol className="mt-6 space-y-2 pb-24">
          {entries.map((entry) => (
            <HistoryRow key={entry.key} entry={entry} showWho={person === "everyone" && entry.who !== me.username} />
          ))}
        </ol>
      )}
    </PageFrame>
  )
}

function HistoryRow({ entry, showWho }: { entry: Entry; showWho: boolean }) {
  const artwork = useArtwork(entry.artist, entry.title)
  const when = new Date(entry.at * 1000).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  })
  return (
    <li>
      <Link
        to={entry.link}
        className="flex items-center gap-4 rounded-2xl px-3 py-2.5 outline-none transition-colors hover:bg-card/60 focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} alt="" className="size-12 rounded-lg" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[15px] font-medium">
            {artwork.data?.album ?? entry.title}
            <span className="font-normal text-muted-foreground">
              , {artwork.data?.artist ?? entry.artist ?? "Unknown artist"}
            </span>
          </p>
          <p
            className={cn(
              "truncate text-[13.5px]",
              entry.tone === "good"
                ? "text-q-lossless"
                : entry.tone === "bad"
                  ? "text-destructive"
                  : "text-muted-foreground",
            )}
          >
            {entry.what}
            {showWho && entry.who && <span className="text-muted-foreground">, for {entry.who}</span>}
          </p>
        </div>
        <span className="hidden shrink-0 text-[13px] text-muted-foreground sm:block">{when}</span>
      </Link>
    </li>
  )
}
