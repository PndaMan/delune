import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { Check, ListPlus, LoaderCircle } from "lucide-react"
import { useMemo, useState } from "react"

import { Choice } from "@/components/choice"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { type ResolvedLink, toApiError } from "@/lib/api"
import { formatTrackTime, plural } from "@/lib/format"
import { MIN_QUALITY_LABELS, type MinQuality } from "@/lib/wishlist"
import { cn } from "@/lib/utils"

type Mode = "albums" | "songs"

/** "Hey Jude (Remastered 2015)" → "Hey Jude": folder names rarely carry edition notes. */
const clean = (title: string) =>
  title
    .replace(/\s*[([][^)\]]*[)\]]/g, "")
    .replace(/\s+-\s+.*(remaster|version|edit|mono|stereo|live).*$/i, "")
    .trim()

/**
 * A pasted playlist: choose songs and add them to the wishlist, which searches
 * Soulseek for each and downloads good copies for review.
 */
export function PlaylistImport({ link }: { link: ResolvedLink }) {
  const client = useQueryClient()
  const [mode, setMode] = useState<Mode>("songs")
  const [quality, setQuality] = useState<MinQuality>("lossless")
  const [auto, setAuto] = useState(true)
  const [skipped, setSkipped] = useState<Set<number>>(new Set())

  const chosen = link.tracks.filter((_, i) => !skipped.has(i))
  const requests = useMemo(() => {
    const seen = new Set<string>()
    const out: { query: string; track?: string; playlist: string; auto_download: boolean; min_quality: MinQuality }[] = []
    for (const t of chosen) {
      const artist = t.artist?.split(",")[0]?.trim() ?? ""
      // Whole albums where the service says which album; the song alone otherwise.
      if (mode === "albums" && t.album) {
        const query = `${artist} ${clean(t.album)}`.trim()
        if (seen.has(query.toLowerCase())) continue
        seen.add(query.toLowerCase())
        out.push({ query, playlist: link.title, auto_download: auto, min_quality: quality })
      } else {
        out.push({ query: `${artist} ${clean(t.title)}`.trim(), track: clean(t.title), playlist: link.title, auto_download: auto, min_quality: quality })
      }
    }
    return out
  }, [chosen, mode, auto, quality, link.title])

  const add = useMutation({ meta: { quiet: true },
    mutationFn: async () => {
      const res = await fetch("/api/v1/wishlist/batch", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(requests),
      })
      if (!res.ok) throw await toApiError(res)
      return ((await res.json()) as { added: number }).added
    },
    onSuccess: () => void client.invalidateQueries({ queryKey: ["wishlist"] }),
  })

  if (link.tracks.length === 0) {
    return <p className="py-10 text-muted-foreground">This playlist is empty, or its service didn't list the songs.</p>
  }
  const hasAlbums = link.tracks.some((t) => t.album)

  return (
    <div className="pb-24">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-3 rounded-2xl border bg-card/50 px-5 py-4">
        {hasAlbums && (
          <div role="radiogroup" aria-label="Add as" className="flex gap-0.5 rounded-xl border bg-background/40 p-1">
            {(["songs", "albums"] as Mode[]).map((m) => (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={mode === m}
                onClick={() => setMode(m)}
                className={cn("h-8 rounded-lg px-3 text-[13.5px]", mode === m ? "bg-accent text-foreground" : "text-muted-foreground")}
              >
                {m === "songs" ? "Just the songs" : "Whole albums"}
              </button>
            ))}
          </div>
        )}
        <Choice
          value={quality}
          onChange={setQuality}
          options={(Object.keys(MIN_QUALITY_LABELS) as MinQuality[]).map((q) => ({ value: q, label: MIN_QUALITY_LABELS[q] }))}
          label="Least quality to accept"
          className="h-10 rounded-xl px-3 text-[14px]"
        />
        <label className="flex cursor-pointer items-center gap-2 text-[14px] text-muted-foreground">
          <Switch checked={auto} onCheckedChange={setAuto} /> Download for review when found
        </label>
        <div className="ml-auto flex items-center gap-3">
          {add.isSuccess ? (
            <p className="flex items-center gap-1.5 text-[14px] text-q-lossless">
              <Check className="size-4" /> Added {plural(add.data, "search")}.{" "}
              <Link to="/downloads" className="underline underline-offset-4">
                See the wishlist
              </Link>
            </p>
          ) : (
            <Button onClick={() => add.mutate()} disabled={!requests.length || add.isPending} className="h-10 rounded-xl">
              {add.isPending ? <LoaderCircle className="animate-spin" /> : <ListPlus />}
              Add {plural(requests.length, mode === "albums" ? "search" : "song")} to the wishlist
            </Button>
          )}
        </div>
      </div>
      {add.isError && <p className="mt-3 text-sm text-destructive">{add.error.message}</p>}

      <ol className="mt-5 overflow-hidden rounded-2xl border bg-card/40">
        {link.tracks.map((t, i) => {
          const on = !skipped.has(i)
          return (
            <li key={`${t.title}-${i}`} className="border-b last:border-b-0">
              <label className={cn("flex cursor-pointer items-center gap-4 px-4 py-2.5", !on && "opacity-50")}>
                <input
                  type="checkbox"
                  checked={on}
                  onChange={() =>
                    setSkipped((current) => {
                      const next = new Set(current)
                      if (next.has(i)) next.delete(i)
                      else next.add(i)
                      return next
                    })
                  }
                  className="size-4 accent-[var(--primary)]"
                />
                <span className="w-6 text-right text-[13px] text-muted-foreground/70">{i + 1}</span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[15px]">{t.title}</span>
                  <span className="block truncate text-[13px] text-muted-foreground">
                    {[t.artist, t.album].filter(Boolean).join(", ")}
                  </span>
                </span>
                <span className="text-[13px] text-muted-foreground">{formatTrackTime(t.duration_secs)}</span>
              </label>
            </li>
          )
        })}
      </ol>
    </div>
  )
}
