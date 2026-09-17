import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { CircleCheck, Copy, FolderInput, LoaderCircle, RefreshCw, RotateCcw, TriangleAlert } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { toApiError } from "@/lib/api"
import type { HealthFinding, HealthFixed, LibraryHealth, TrashBatch, TrashRestored } from "@/lib/api.generated"
import { formatAgo, plural } from "@/lib/format"
import { toast } from "@/lib/toast"
import { cn } from "@/lib/utils"

/** A fix that touches more files than this asks for a second tap. */
const CONFIRM_OVER = 20

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

      <TrashList batches={data.trash} onDone={refresh} />
    </div>
  )
}

function FindingRow({ finding, onDone }: { finding: HealthFinding; onDone: () => void }) {
  const [confirming, setConfirming] = useState(false)
  const fix = useMutation({
    mutationFn: () => send<HealthFixed>("/library/health/fix", { id: finding.id }),
    onSuccess: (fixed) => {
      const done = [
        fixed.moved && `moved ${plural(fixed.moved, "track")}`,
        fixed.trashed && `${fixed.trashed} to the trash`,
      ]
        .filter(Boolean)
        .join(", ")
      toast(`Tidied: ${done}. You can put it back below.`)
      onDone()
    },
    onError: (e) => {
      toast(e.message, "error")
      onDone()
    },
  })
  const split = finding.kind === "split-album"
  const { artist, album } = parts(finding.folders[0])
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
        {split ? <FolderInput className="size-[18px]" /> : <Copy className="size-[18px]" />}
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-[15px] font-medium text-pretty">
          {split
            ? `${album} by ${artist} is in ${finding.folders.length} folders`
            : `${plural(finding.duplicates.length, "track")} twice in ${album || artist}`}
        </p>
        {split ? (
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
        ) : (
          <ul className="mt-1.5 space-y-0.5 text-[13px] text-muted-foreground">
            {finding.duplicates.map((copies) => (
              <li key={copies[0]} className="truncate" title={copies.join("\n")}>
                {copies.map((c) => c.split("/").at(-1)).join(" · ")}
              </li>
            ))}
          </ul>
        )}
        <p className="mt-1.5 text-[13px] text-pretty text-muted-foreground">
          {split
            ? `Moves ${plural(finding.files, "track")} into the first folder. Where both have a track, the better copy stays.`
            : "Keeps the best copy of each."}
        </p>
      </div>
      <Button
        variant={confirming ? "default" : "outline"}
        onClick={act}
        disabled={fix.isPending}
        className="shrink-0 max-sm:w-full"
      >
        {fix.isPending && <LoaderCircle className="animate-spin" />}
        {confirming ? `Tap again: ${plural(finding.files, "file")}` : split ? "Merge" : "Keep the best"}
      </Button>
    </li>
  )
}

function TrashList({ batches, onDone }: { batches: TrashBatch[]; onDone: () => void }) {
  const restore = useMutation({
    mutationFn: (id: string) => send<TrashRestored>(`/library/trash/${encodeURIComponent(id)}/restore`),
    onSuccess: (result) => {
      toast(
        result.left.length
          ? `Put back, except ${plural(result.left.length, "file")} whose place is taken again`
          : "Put back where it was",
      )
      onDone()
    },
    onError: (e) => toast(e.message, "error"),
  })
  if (!batches.length) return null
  return (
    <section className="px-5 py-4">
      <h3 className="text-[12.5px] font-semibold tracking-wide text-muted-foreground uppercase">In the trash</h3>
      <ul className="mt-2 space-y-1">
        {batches.map((batch) => (
          <li key={batch.id} className="flex items-center gap-3 rounded-xl px-2 py-2">
            <div className="min-w-0 flex-1">
              <p className="text-[14.5px]">{plural(batch.files, "file")}</p>
              <p className="text-[13px] text-muted-foreground">Taken out {formatAgo(batch.created_at)}</p>
            </div>
            <Button variant="ghost" size="sm" onClick={() => restore.mutate(batch.id)} disabled={restore.isPending}>
              <RotateCcw /> Put back
            </Button>
          </li>
        ))}
      </ul>
    </section>
  )
}
