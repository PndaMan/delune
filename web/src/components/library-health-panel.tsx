import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { CircleCheck, Copy, Disc3, FolderInput, LoaderCircle, RefreshCw, RotateCcw, TriangleAlert } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { toApiError } from "@/lib/api"
import type { HealthFinding, HealthFixed, LibraryHealth, TrashBatch, TrashRestored } from "@/lib/api.generated"
import { formatAgo, plural } from "@/lib/format"
import { toast } from "@/lib/toast"
import { cn } from "@/lib/utils"

/** A fix that touches more files than this asks for a second tap. */
const CONFIRM_OVER = 20

async function call(method: string, path: string, body?: unknown) {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) throw await toApiError(res)
}

async function send<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method: "POST",
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) throw await toApiError(res)
  return res.json() as Promise<T>
}

function useLibraryHealth() {
  return useQuery({
    queryKey: ["library", "health"],
    queryFn: async ({ signal }) => {
      const res = await fetch("/api/v1/library/health", { signal })
      if (!res.ok) throw await toApiError(res)
      return (await res.json()) as LibraryHealth
    },
    // Checking reads every folder of the library; do it when asked, not on a timer.
    staleTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    retry: false,
  })
}

const parts = (folder: string) => {
  const [artist, ...rest] = folder.split("/")
  // "USB (2025)" reads as "USB": the year is the folder's, not the album's name.
  return { artist, album: rest.join("/").replace(/\s*[([]\d{4}[)\]]$/, "") }
}

/** Albums split over folders, doubled tracks, and the trash fixes go to. */
export function LibraryHealthPanel() {
  const health = useLibraryHealth()
  const client = useQueryClient()
  const refresh = () => void client.invalidateQueries({ queryKey: ["library", "health"] })

  if (health.isPending) {
    return (
      <p className="flex items-center gap-2 px-5 py-8 text-muted-foreground">
        <LoaderCircle className="size-4 animate-spin" /> Looking through your library
      </p>
    )
  }
  if (health.isError) {
    return (
      <div className="flex items-center gap-3 px-5 py-6">
        <p className="min-w-0 flex-1 text-destructive">{health.error.message}</p>
        <Button variant="outline" onClick={() => void health.refetch()}>
          Try again
        </Button>
      </div>
    )
  }

  const data = health.data
  const tidy = data.findings.length === 0
  return (
    <div>
      <div className="flex items-center gap-4 border-b px-5 py-5">
        <span
          className={cn(
            "flex size-12 shrink-0 items-center justify-center rounded-full",
            tidy ? "bg-q-lossless/15" : "bg-q-hires/15",
          )}
        >
          {tidy ? (
            <CircleCheck className="size-6 text-q-lossless" />
          ) : (
            <TriangleAlert className="size-6 text-q-hires" />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[18px] font-semibold">
            {tidy ? "Your library looks tidy" : `${plural(data.findings.length, "thing")} to tidy`}
          </p>
          <p className="text-[13px] text-muted-foreground">
            {plural(data.albums, "album")} · {plural(data.tracks, "track")} checked
          </p>
        </div>
        <Button
          variant="outline"
          onClick={() => void health.refetch()}
          disabled={health.isFetching}
          aria-label="Check again"
          className="max-sm:size-10 max-sm:px-0"
        >
          <RefreshCw className={cn(health.isFetching && "animate-spin")} />
          <span className="max-sm:hidden">Check again</span>
        </Button>
      </div>

      {!tidy && (
        <ul className="divide-y border-b">
          {data.findings.map((finding) => (
            <FindingRow key={finding.id} finding={finding} onDone={refresh} />
          ))}
        </ul>
      )}

      <p className="border-b px-5 py-3 text-[13px] text-pretty text-muted-foreground">
        Nothing is deleted. Tidying and upgrades move what they take out to{" "}
        <span className="text-foreground">.delune-trash</span> in your music folder, which Navidrome doesn't show, and
        it's emptied after {data.trash_days} days. Before a fix, delune checks the files are still as they were.
      </p>

      {data.ignored > 0 && <IgnoredNote count={data.ignored} onDone={refresh} />}

      <TrashList batches={data.trash} onDone={refresh} />
    </div>
  )
}

const file = (path: string) => path.split("/").at(-1) ?? path

/** How each kind of finding reads, and what fixing it does. */
function wording(finding: HealthFinding) {
  const { artist, album } = parts(finding.folders[0])
  switch (finding.kind) {
    case "split-album":
      return {
        icon: FolderInput,
        title: `${finding.album ?? album} by ${finding.album_artist ?? artist} is in ${finding.folders.length} folders`,
        action: "Merge",
        effect: `Moves ${plural(finding.files, "track")} into the first folder, keeps the better copy where both have a track, then makes every track say the same album so players show it once.`,
      }
    case "mixed-album":
      return {
        icon: Disc3,
        title: `${finding.album ?? album} shows up as more than one album`,
        action: "Make it one album",
        effect: [
          finding.retag &&
            `Sets ${plural(finding.retag, "track")} to ${finding.album ?? album}${finding.album_artist ? ` by ${finding.album_artist}` : ""}, with one date and no clashing release ids.`,
          finding.strays.length &&
            `Moves ${plural(finding.strays.length, "track")} from other albums to their own album's folder.`,
        ]
          .filter(Boolean)
          .join(" "),
      }
    default:
      return {
        icon: Copy,
        title: `${plural(finding.duplicates.length, "track")} twice in ${album || artist}`,
        action: "Keep the best",
        effect: "Keeps the best copy of each.",
      }
  }
}

function FindingRow({ finding, onDone }: { finding: HealthFinding; onDone: () => void }) {
  const [confirming, setConfirming] = useState(false)
  const fix = useMutation({
    mutationFn: () => send<HealthFixed>("/library/health/fix", { id: finding.id }),
    onSuccess: (fixed) => {
      const done = [
        fixed.moved && `moved ${plural(fixed.moved, "track")}`,
        fixed.retagged && `retagged ${plural(fixed.retagged, "track")}`,
        fixed.trashed && `${fixed.trashed} to the trash`,
      ]
        .filter(Boolean)
        .join(", ")
      toast(done ? `Done: ${done}. You can undo it below.` : "Nothing needed changing.")
      onDone()
    },
    onError: (e) => {
      toast(e.message, "error")
      onDone()
    },
  })
  const ignore = useMutation({
    mutationFn: () => call("POST", "/library/health/ignore", { key: finding.key }),
    onSuccess: () => {
      toast("Ignored. It won't come up again.")
      onDone()
    },
    onError: (e) => toast(e.message, "error"),
  })
  const words = wording(finding)
  const Icon = words.icon
  const act = () => {
    if (finding.files > CONFIRM_OVER && !confirming) {
      setConfirming(true)
      window.setTimeout(() => setConfirming(false), 4000)
      return
    }
    setConfirming(false)
    fix.mutate()
  }

  return (
    <li className="flex flex-col gap-3 px-5 py-4 sm:flex-row sm:items-start">
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-accent max-sm:hidden">
        <Icon className="size-[18px]" />
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-[15px] font-medium text-pretty">{words.title}</p>
        {finding.kind === "split-album" && (
          <ul className="mt-1.5 space-y-0.5 text-[13px]">
            {finding.folders.map((folder, i) => (
              <li key={folder} className="flex min-w-0 gap-2">
                <span className={cn("shrink-0", i === 0 ? "text-q-lossless" : "text-muted-foreground")}>
                  {i === 0 ? "Keep" : "Merge"}
                </span>
                <span className="truncate text-muted-foreground" title={folder}>
                  {folder}
                </span>
              </li>
            ))}
          </ul>
        )}
        {finding.kind === "mixed-album" && (
          <p className="mt-1 truncate text-[13px] text-muted-foreground" title={finding.folders[0]}>
            {finding.folders[0]}
          </p>
        )}
        {finding.kind === "mixed-album" && finding.strays.length > 0 && (
          <ul className="mt-1 space-y-0.5 text-[13px] text-muted-foreground">
            {finding.strays.map((stray) => (
              <li key={stray} className="truncate" title={stray}>
                Belongs elsewhere: {file(stray)}
              </li>
            ))}
          </ul>
        )}
        {finding.kind === "duplicate-tracks" && (
          <ul className="mt-1.5 space-y-0.5 text-[13px] text-muted-foreground">
            {finding.duplicates.map((copies) => (
              <li key={copies[0]} className="truncate" title={copies.join("\n")}>
                {copies.map(file).join(" · ")}
              </li>
            ))}
          </ul>
        )}
        <p className="mt-1.5 text-[13px] text-pretty text-muted-foreground">{words.effect}</p>
      </div>
      <div className="flex shrink-0 gap-2 max-sm:w-full">
        <Button
          variant="ghost"
          onClick={() => ignore.mutate()}
          disabled={ignore.isPending || fix.isPending}
          className="max-sm:flex-1"
        >
          Ignore
        </Button>
        <Button
          variant={confirming ? "default" : "outline"}
          onClick={act}
          disabled={fix.isPending}
          className="max-sm:flex-1"
        >
          {fix.isPending && <LoaderCircle className="animate-spin" />}
          {confirming ? `Tap again: ${plural(finding.files, "file")}` : words.action}
        </Button>
      </div>
    </li>
  )
}

function IgnoredNote({ count, onDone }: { count: number; onDone: () => void }) {
  const unignore = useMutation({
    mutationFn: () => call("DELETE", "/library/health/ignore"),
    onSuccess: onDone,
    onError: (e) => toast(e.message, "error"),
  })
  return (
    <p className="flex items-center gap-3 border-b px-5 py-3 text-[13px] text-muted-foreground">
      <span className="min-w-0 flex-1">{plural(count, "ignored finding")} not shown</span>
      <Button variant="ghost" size="sm" onClick={() => unignore.mutate()} disabled={unignore.isPending}>
        Show again
      </Button>
    </p>
  )
}

const CHANGE_WORDS: Record<string, string> = { removed: "In the trash", moved: "Moved", retagged: "Tags changed" }

function TrashList({ batches, onDone }: { batches: TrashBatch[]; onDone: () => void }) {
  const restore = useMutation({
    mutationFn: (id: string) => send<TrashRestored>(`/library/trash/${encodeURIComponent(id)}/restore`),
    onSuccess: (result) => {
      toast(
        result.left.length
          ? `Undone, except ${plural(result.left.length, "file")} whose place is taken again`
          : "Undone: everything is back as it was",
      )
      onDone()
    },
    onError: (e) => toast(e.message, "error"),
  })
  if (!batches.length) return null
  return (
    <section className="px-5 py-4">
      <h3 className="text-[12.5px] font-semibold tracking-wide text-muted-foreground uppercase">Recent changes</h3>
      <p className="mt-1 text-[13px] text-muted-foreground">
        Fixes and upgrades, newest first. Undo puts back everything one of them did.
      </p>
      <ul className="mt-2 space-y-1">
        {batches.map((batch) => (
          <TrashRow key={batch.id} batch={batch} onUndo={() => restore.mutate(batch.id)} busy={restore.isPending} />
        ))}
      </ul>
    </section>
  )
}

function TrashRow({ batch, onUndo, busy }: { batch: TrashBatch; onUndo: () => void; busy: boolean }) {
  const counts = Object.entries(
    batch.changes.reduce<Record<string, number>>((n, c) => ({ ...n, [c.kind]: (n[c.kind] ?? 0) + 1 }), {}),
  )
    .map(([kind, n]) => `${n} ${(CHANGE_WORDS[kind] ?? kind).toLowerCase()}`)
    .join(" · ")
  return (
    <li className="rounded-xl px-2 py-2">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="text-[14.5px] text-pretty">{batch.label ?? "Changes to your library"}</p>
          <p className="text-[13px] text-muted-foreground">
            {formatAgo(batch.created_at)} · {counts}
          </p>
        </div>
        <Button variant="ghost" size="sm" onClick={onUndo} disabled={busy} className="shrink-0">
          <RotateCcw /> Undo
        </Button>
      </div>
      <details className="mt-1 text-[13px]">
        <summary className="cursor-pointer text-muted-foreground select-none hover:text-foreground">
          Show {plural(batch.changes.length, "file")}
        </summary>
        <ul className="mt-1.5 space-y-1 border-l pl-3">
          {batch.changes.map((change) => (
            <li key={`${change.kind}:${change.path}`} className="min-w-0">
              <span className="text-muted-foreground">{CHANGE_WORDS[change.kind] ?? change.kind}: </span>
              <span className="break-words">{change.path}</span>
              {change.to && <span className="block break-words text-muted-foreground">→ {change.to}</span>}
            </li>
          ))}
        </ul>
      </details>
    </li>
  )
}
