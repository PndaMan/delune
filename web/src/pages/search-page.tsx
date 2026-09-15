import { getRouteApi } from "@tanstack/react-router"
import { RotateCw, X } from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"

import { EmptyState } from "@/components/empty-state"
import { Moon } from "@/components/moon"
import { ReleaseModal } from "@/components/release-modal"
import { Cover } from "@/components/cover"
import { ResultFilters, type TierFilter } from "@/components/results/result-filters"
import { ResultHeader, ResultList } from "@/components/results/result-list"
import { SearchField, type SearchFieldHandle } from "@/components/search-field"
import { describeSoulseek, useSoulseekStatus } from "@/components/soulseek-indicator"
import { Button } from "@/components/ui/button"
import { type Candidate, PROVIDER_NAMES, type ResolvedLink } from "@/lib/api"
import { SearchContext, useAccentColour, useArtwork } from "@/lib/artwork"
import { plural } from "@/lib/format"
import { moonPhase } from "@/lib/moon-phase"
import { SORTS, type SortKey, tierOf, typicalTracks } from "@/lib/quality"
import { useRecentSearches } from "@/lib/recent"
import { matchLink, relevance, ResolvedContext } from "@/lib/tracklist"
import { type SearchState, useSearch } from "@/lib/use-search"
import { cn } from "@/lib/utils"

const route = getRouteApi("/")

export function SearchPage() {
  const { q } = route.useSearch()
  const navigate = route.useNavigate()
  const search = useSearch(q ?? null)
  const { recent, remember, forget } = useRecentSearches()

  const submit = useCallback(
    (value: string) => {
      remember(value)
      void navigate({ search: { q: value } })
    },
    [navigate, remember],
  )

  if (!q) return <Idle onSubmit={submit} recent={recent} onForget={forget} />
  // For links, artwork and library matching go by what the link points at, not the URL.
  const context = search.resolved
    ? [search.resolved.artist, search.resolved.album ?? search.resolved.title].filter(Boolean).join(" ")
    : q
  return (
    <SearchContext.Provider value={context}>
      <ResolvedContext.Provider value={search.resolved}>
        <Results key={q} query={q} search={search} onSubmit={submit} />
      </ResolvedContext.Provider>
    </SearchContext.Provider>
  )
}

function Idle({ onSubmit, recent, onForget }: { onSubmit: (q: string) => void; recent: string[]; onForget: (q: string) => void }) {
  const tonight = moonPhase()
  const status = useSoulseekStatus()
  const soulseek = describeSoulseek(status.data, status.isError)

  return (
    <div className="relative mx-auto flex min-h-[calc(100dvh-5rem)] max-w-[760px] flex-col items-center justify-center px-5 py-16 md:min-h-dvh">
      <div className="flex flex-col items-center text-center">
        <Moon illumination={tonight.illumination} waxing={tonight.waxing} size={188} glow label={`${tonight.name}, ${Math.round(tonight.illumination * 100)} percent lit`} />
        <p className="mt-6 text-sm text-muted-foreground">
          Tonight's moon is a {tonight.name.toLowerCase()}, {Math.round(tonight.illumination * 100)}% lit
        </p>
        <h1 className="type-display mt-8 text-[clamp(2.6rem,7.5vw,4.4rem)] text-balance">Find something to listen to</h1>
      </div>

      <div className="mt-10 w-full">
        <SearchField size="lg" onSubmit={onSubmit} />
      </div>

      {soulseek.tone === "bad" && (
        <p className="mt-2 w-full rounded-xl border border-destructive/25 bg-destructive/8 px-4 py-3 text-sm">
          {soulseek.text}.{" "}
          {status.data?.state === "not-configured"
            ? "Start the server with DELUNE_SLSK_USERNAME and DELUNE_SLSK_PASSWORD to search."
            : "Searches won't return anything until this is fixed."}
        </p>
      )}

      {recent.length > 0 && (
        <div className="mt-6 flex w-full flex-wrap items-center gap-2 px-1">
          <span className="mr-1 text-sm text-muted-foreground">Recent</span>
          {recent.map((query) => (
            <span key={query} className="group flex items-center rounded-full border bg-card/50 text-sm transition-colors hover:bg-accent">
              <button type="button" onClick={() => onSubmit(query)} className="rounded-l-full py-1.5 pr-1 pl-3.5 outline-none focus-visible:ring-2 focus-visible:ring-ring">
                {query}
              </button>
              <button
                type="button"
                onClick={() => onForget(query)}
                className="rounded-r-full py-1.5 pr-2.5 pl-1 text-muted-foreground/60 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                aria-label={`Remove ${query} from recent searches`}
              >
                <X className="size-3.5" />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

function useProgress(search: SearchState) {
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    if (search.status !== "running") return
    const id = window.setInterval(() => setNow(Date.now()), 250)
    return () => window.clearInterval(id)
  }, [search.status])
  if (search.status === "done") return 1
  if (search.status !== "running" || !search.startedAt) return 0
  return Math.min(0.97, (now - search.startedAt) / (search.timeoutSecs * 1000))
}

function Results({ query, search, onSubmit }: { query: string; search: SearchState; onSubmit: (q: string) => void }) {
  const [tier, setTier] = useState<TierFilter>("all")
  const [readyOnly, setReadyOnly] = useState(false)
  const [sort, setSort] = useState<SortKey>("quality")
  const [selected, setSelected] = useState(0)
  const [keyboard, setKeyboard] = useState(false)
  const [open, setOpen] = useState<Candidate | null>(null)
  const field = useRef<SearchFieldHandle>(null)
  const progress = useProgress(search)

  const counts = useMemo(() => {
    const pool = readyOnly ? search.candidates.filter((c) => c.free_slot) : search.candidates
    const result: Record<TierFilter, number> = { all: pool.length, hires: 0, lossless: 0, lossy: 0, unknown: 0 }
    for (const c of pool) result[tierOf(c.quality)]++
    return result
  }, [search.candidates, readyOnly])

  const visible = useMemo(() => {
    const filtered = search.candidates.filter(
      (c) => (tier === "all" || tierOf(c.quality) === tier) && (!readyOnly || c.free_slot),
    )
    const compare = SORTS[sort].compare(typicalTracks(search.candidates))
    // Folders that match the pasted link come first, whatever the sort.
    const link = search.resolved
    return filtered.sort((a, b) => relevance(matchLink(a, link)) - relevance(matchLink(b, link)) || compare(a, b))
  }, [search.candidates, search.resolved, tier, readyOnly, sort])

  useEffect(() => setSelected((s) => Math.min(s, Math.max(0, visible.length - 1))), [visible.length])

  const onOpen = useCallback((c: Candidate) => setOpen(c), [])
  const onLeaveTop = useCallback(() => {
    setKeyboard(false)
    field.current?.focus()
  }, [])

  const top = visible[0] ?? search.candidates[0]
  const resolved = search.resolved
  const heroArt = useArtwork(
    resolved ? resolved.artist : (top?.parent ?? null),
    resolved ? (resolved.album ?? resolved.title) : (top?.title ?? null),
  )
  const accent = useAccentColour(heroArt.data?.thumb)

  return (
    <div className="relative" style={accent ? ({ "--primary": accent, "--ring": accent } as React.CSSProperties) : undefined}>
      <div
        className="pointer-events-none absolute inset-x-0 top-0 h-[520px] transition-opacity duration-700"
        style={{
          background: `radial-gradient(60% 100% at 20% 0%, ${accent ?? "transparent"}, transparent 70%)`,
          opacity: accent ? 0.16 : 0,
        }}
        aria-hidden
      />

      <div className="sticky top-0 z-20 border-b border-transparent bg-background/75 backdrop-blur-xl supports-[backdrop-filter]:bg-background/55">
        <div className="mx-auto w-full max-w-[1200px] px-4 pt-4 pb-3 sm:px-8">
          <SearchField
            ref={field}
            initialValue={query}
            onSubmit={onSubmit}
            hideHint
            progress={search.status === "running" || search.status === "done" ? progress : null}
            onArrowDown={() => {
              if (!visible.length) return
              ;(document.activeElement as HTMLElement | null)?.blur()
              setKeyboard(true)
              setSelected(0)
            }}
          />
        </div>
      </div>

      <div className="relative mx-auto w-full max-w-[1200px] px-4 sm:px-8">
        <section className="flex items-center gap-6 pt-8 pb-8">
          {(top || resolved) && (
            <Cover
              src={heroArt.data?.cover}
              pending={heroArt.isPending}
              alt=""
              className="hidden size-28 rounded-2xl shadow-[0_24px_50px_-20px_rgb(0_0_0/0.9)] sm:block"
            />
          )}
          <div className="min-w-0">
            {resolved ? (
              <>
                <ResolvedHeading link={resolved} />
                <StatusLine search={search} visible={visible.length} query={query} />
              </>
            ) : (
              <>
                <h1 className="type-display text-[clamp(1.9rem,4vw,2.75rem)] text-balance break-words">
                  {looksLikeLink(query) ? "Reading that link" : query}
                </h1>
                <StatusLine search={search} visible={visible.length} query={query} />
              </>
            )}
          </div>
        </section>

        {search.status === "failed" ? (
          <EmptyState
            illumination={0}
            title="The search didn't go through"
            action={
              <Button variant="outline" onClick={() => onSubmit(query)}>
                <RotateCw /> Try again
              </Button>
            }
          >
            {search.error}
          </EmptyState>
        ) : search.status === "done" && search.candidates.length === 0 ? (
          <EmptyState illumination={0.08} title={`Nobody is sharing “${resolved?.title ?? query}” right now`}>
            Try fewer words, or just the album title. The Soulseek network sometimes drops longer exact phrases, and
            people come online throughout the day.
          </EmptyState>
        ) : (
          <>
            <ResultFilters
              tier={tier}
              onTier={setTier}
              counts={counts}
              readyOnly={readyOnly}
              onReadyOnly={setReadyOnly}
              sort={sort}
              onSort={setSort}
            />
            <div className="mt-6 pb-16">
              {search.candidates.length === 0 ? (
                <Waiting />
              ) : visible.length === 0 ? (
                <p className="px-5 py-10 text-muted-foreground">No results match these filters.</p>
              ) : (
                <>
                  <ResultHeader />
                  <ResultList
                    candidates={visible}
                    selected={selected}
                    onSelect={setSelected}
                    onOpen={onOpen}
                    keyboard={keyboard}
                    onKeyboard={setKeyboard}
                    onLeaveTop={onLeaveTop}
                    paused={open !== null}
                  />
                  <p className="px-5 pt-6 text-[13px] text-muted-foreground/70">
                    Use ↑ ↓ to move through results and Enter to open one.
                  </p>
                </>
              )}
            </div>
          </>
        )}
      </div>

      <ReleaseModal candidate={open} onClose={() => setOpen(null)} />
    </div>
  )
}

/** A pasted link, named: title first, then who made it and where the link came from. */
function ResolvedHeading({ link }: { link: ResolvedLink }) {
  const kind = link.kind === "track" ? "track" : link.kind === "artist" ? "artist" : "album"
  const facts = [link.artist, link.year, `${PROVIDER_NAMES[link.provider]} ${kind}`].filter(Boolean)
  return (
    <>
      <h1 className="type-display text-[clamp(1.9rem,4vw,2.75rem)] text-balance break-words">{link.title}</h1>
      <p className="mt-1 text-[17px] text-foreground/85">
        {facts.join(", ")}
        {link.kind === "track" && link.album ? <span className="text-muted-foreground"> from {link.album}</span> : null}
      </p>
    </>
  )
}

function looksLikeLink(query: string) {
  return /^(spotify:|https?:\/\/)/i.test(query) || (!/\s/.test(query) && /^[\w.-]+\.[a-z]{2,}\//i.test(query))
}

function StatusLine({ search, visible, query }: { search: SearchState; visible: number; query: string }) {
  const total = search.candidates.length
  let text: string
  if (search.status === "running" && !search.searchedFor && looksLikeLink(query)) {
    text = "Finding out what this link points to"
  } else if (search.status === "running") {
    text = total
      ? `Listening for answers. ${plural(total, "release")} from ${plural(search.peers, "person", "people")} so far`
      : "Asking the Soulseek network. The first answers usually arrive within a few seconds"
  } else if (search.status === "done") {
    text = `${plural(total, "release")} from ${plural(search.peers, "person", "people")}`
    if (visible !== total) text += `, ${visible.toLocaleString()} shown`
    if (search.resolved && search.searchedFor) text += `, searched for “${search.searchedFor}”`
  } else {
    text = ""
  }
  return (
    <p className={cn("mt-1.5 text-[15px] text-muted-foreground", search.status === "running" && "animate-pulse [animation-duration:2.4s]")}>
      {text}
    </p>
  )
}

function Waiting() {
  return (
    <div className="space-y-1" aria-hidden>
      {Array.from({ length: 6 }, (_, i) => (
        <div key={i} className="flex h-[70px] items-center gap-5 rounded-xl px-5" style={{ opacity: 1 - i * 0.14 }}>
          <div className="h-9 w-24 rounded-lg bg-muted/70" />
          <div className="flex-1 space-y-2">
            <div className="h-4 w-1/3 rounded bg-muted/70" />
            <div className="h-3 w-1/5 rounded bg-muted/50" />
          </div>
        </div>
      ))}
    </div>
  )
}
