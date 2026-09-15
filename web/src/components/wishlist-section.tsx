import { Link } from "@tanstack/react-router"
import { Pause, Play, Plus, Sparkles, Trash2 } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { chatTime } from "@/lib/chat"
import { plural } from "@/lib/format"
import { requesterLabel, useMe } from "@/lib/session"
import {
  MIN_QUALITY_LABELS,
  type MinQuality,
  useAddToWishlist,
  useRemoveFromWishlist,
  useUpdateWishlist,
  useWishlist,
  type WishlistItem,
} from "@/lib/wishlist"
import { cn } from "@/lib/utils"

/** Things to keep looking for, with what the last search turned up. */
export function WishlistSection() {
  const wishlist = useWishlist()
  const add = useAddToWishlist()
  const [query, setQuery] = useState("")
  const items = wishlist.data ?? []

  return (
    <section className="mt-12 pb-24">
      <div className="flex flex-wrap items-end gap-x-6 gap-y-2">
        <h2 className="type-title text-[24px]">Wishlist</h2>
        <p className="text-sm text-muted-foreground">
          Searched again every few minutes. Good copies download for review on their own.
        </p>
      </div>
      <form
        className="mt-4 flex max-w-xl gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (query.trim()) add.mutate({ query: query.trim() }, { onSuccess: () => setQuery("") })
        }}
      >
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Artist and album to watch for"
          aria-label="Add to wishlist"
          className="h-11 min-w-0 flex-1 rounded-xl border bg-card/70 px-4 text-[15px] outline-none focus:border-primary/50"
        />
        <Button type="submit" variant="outline" className="h-11 rounded-xl" disabled={!query.trim() || add.isPending}>
          <Plus /> Add
        </Button>
      </form>
      {add.isError && <p className="mt-2 text-sm text-destructive">{add.error.message}</p>}
      {items.length > 0 && (
        <ul className="mt-5 overflow-hidden rounded-2xl border bg-card/50">
          {items.map((item) => (
            <WishlistRow key={item.id} item={item} />
          ))}
        </ul>
      )}
    </section>
  )
}

function WishlistRow({ item }: { item: WishlistItem }) {
  const me = useMe()
  const update = useUpdateWishlist()
  const remove = useRemoveFromWishlist()
  const who = requesterLabel(me, item.added_by)

  const status = item.download_id
    ? "Found and downloading for review"
    : item.paused
      ? "Paused"
      : item.last_searched
        ? `Searched ${chatTime(item.last_searched)}: ${item.last_matches ? plural(item.last_matches, "good copy", "good copies") : "nothing good enough yet"}`
        : "Waiting for its first search"

  return (
    <li className="flex flex-col gap-3 border-b px-5 py-4 last:border-b-0 md:flex-row md:items-center">
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-2 text-[15.5px]">
          {item.download_id && <Sparkles className="size-4 shrink-0 text-q-lossless" />}
          <Link to="/" search={{ q: item.query }} className="truncate hover:underline">
            {item.query}
          </Link>
        </p>
        <p className={cn("mt-0.5 text-[13px]", item.download_id ? "text-q-lossless" : "text-muted-foreground")}>
          {status}
          {item.playlist && <span className="text-muted-foreground/70">, from {item.playlist}</span>}
          {who && <span className="text-muted-foreground/70">, for {who}</span>}
          {item.download_id && (
            <>
              {" "}
              <Link to="/downloads" className="underline-offset-4 hover:underline">
                See it
              </Link>
            </>
          )}
        </p>
      </div>
      {!item.download_id && (
        <div className="flex flex-wrap items-center gap-2">
          <select
            value={item.min_quality}
            onChange={(e) => update.mutate({ id: item.id, min_quality: e.target.value as MinQuality })}
            aria-label="Least quality to accept"
            className="h-9 rounded-lg border bg-background/50 px-2 text-[13.5px]"
          >
            {(Object.keys(MIN_QUALITY_LABELS) as MinQuality[]).map((q) => (
              <option key={q} value={q}>
                {MIN_QUALITY_LABELS[q]}
              </option>
            ))}
          </select>
          <label className="flex h-9 cursor-pointer items-center gap-2 rounded-lg px-2 text-[13.5px] text-muted-foreground">
            <Switch checked={item.auto_download} onCheckedChange={(auto_download) => update.mutate({ id: item.id, auto_download })} />
            Auto-download
          </label>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => update.mutate({ id: item.id, paused: !item.paused })}
            aria-label={item.paused ? "Resume" : "Pause"}
            className="text-muted-foreground"
          >
            {item.paused ? <Play /> : <Pause />}
          </Button>
        </div>
      )}
      <Button variant="ghost" size="icon-sm" onClick={() => remove.mutate(item.id)} aria-label="Remove from wishlist" className="self-end text-muted-foreground md:self-auto">
        <Trash2 />
      </Button>
    </li>
  )
}
