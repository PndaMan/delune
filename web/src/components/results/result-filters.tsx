import { Search, X } from "lucide-react"

import { Switch } from "@/components/ui/switch"
import type { Codec } from "@/lib/api"
import { SORTS, type SortKey, TIER_BG, TIERS, type Tier } from "@/lib/quality"
import { cn } from "@/lib/utils"

export type TierFilter = Tier | "all"

type Props = {
  tier: TierFilter
  onTier: (tier: TierFilter) => void
  counts: Record<TierFilter, number>
  readyOnly: boolean
  onReadyOnly: (value: boolean) => void
  sort: SortKey
  onSort: (sort: SortKey) => void
  text: string
  onText: (text: string) => void
  /** Formats in these results, most common first, with how many releases use each. */
  formats: [Codec, number][]
  selectedFormats: Set<Codec>
  onToggleFormat: (codec: Codec) => void
  hiddenPeople: string[]
  onUnhide: (username: string) => void
}

const CODEC_LABELS: Record<Codec, string> = {
  flac: "FLAC",
  alac: "ALAC",
  wav: "WAV",
  aiff: "AIFF",
  mp3: "MP3",
  aac: "AAC",
  opus: "Opus",
  vorbis: "Vorbis",
}

/** Narrow results by words in their names (a leading - excludes). */
function TextFilter({ text, onText, className }: { text: string; onText: (t: string) => void; className?: string }) {
  return (
    <label
      className={cn(
        "flex h-10 items-center gap-2 rounded-xl border bg-card/60 px-3 focus-within:border-primary/50",
        className,
      )}
    >
      <Search className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      <span className="sr-only">Filter results</span>
      <input
        value={text}
        onChange={(e) => onText(e.target.value)}
        placeholder="Filter, -word to exclude"
        className="h-full min-w-0 flex-1 bg-transparent text-[14px] outline-none placeholder:text-muted-foreground/60"
      />
      {text && (
        <button
          type="button"
          onClick={() => onText("")}
          aria-label="Clear filter"
          className="text-muted-foreground hover:text-foreground"
        >
          <X className="size-4" />
        </button>
      )}
    </label>
  )
}

function HiddenPeople({ people, onUnhide }: { people: string[]; onUnhide: (u: string) => void }) {
  if (!people.length) return null
  return (
    <p className="flex flex-wrap items-center gap-2 text-[13px] text-muted-foreground">
      Hiding results from
      {people.map((u) => (
        <button
          key={u}
          type="button"
          onClick={() => onUnhide(u)}
          title={`Show ${u}'s results again`}
          className="flex items-center gap-1 rounded-full border px-2 py-0.5 hover:text-foreground"
        >
          {u} <X className="size-3" />
        </button>
      ))}
    </p>
  )
}

export function ResultFilters(props: Props) {
  return (
    <>
      <MobileFilters {...props} />
      <DesktopFilters {...props} />
    </>
  )
}

/** Phones: one row of chips you swipe through, instead of controls stacked three deep. */
function MobileFilters(props: Props) {
  const { tier, onTier, counts, readyOnly, onReadyOnly, sort, onSort, formats, selectedFormats, onToggleFormat } = props
  return (
    <div className="space-y-2.5 md:hidden">
      <TextFilter text={props.text} onText={props.onText} />
      <div className="-mx-4 flex gap-2 overflow-x-auto px-4 pb-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <Chip active={tier === "all"} onClick={() => onTier("all")}>
          All <span className="opacity-60">{counts.all}</span>
        </Chip>
        {TIERS.filter(({ id }) => counts[id] > 0).map(({ id, label }) => (
          <Chip key={id} active={tier === id} onClick={() => onTier(id)}>
            <span className={cn("size-2 rounded-full", TIER_BG[id])} aria-hidden />
            {label} <span className="opacity-60">{counts[id]}</span>
          </Chip>
        ))}
        <span className="mx-1 w-px shrink-0 bg-border" aria-hidden />
        <Chip active={readyOnly} onClick={() => onReadyOnly(!readyOnly)}>
          Ready now
        </Chip>
        {(Object.keys(SORTS) as SortKey[]).map((key) => (
          <Chip key={key} active={sort === key} onClick={() => onSort(key)}>
            {SORTS[key].label}
          </Chip>
        ))}
        {formats.length > 1 && <span className="mx-1 w-px shrink-0 bg-border" aria-hidden />}
        {formats.length > 1 &&
          formats.map(([codec, n]) => (
            <Chip key={codec} active={selectedFormats.has(codec)} onClick={() => onToggleFormat(codec)}>
              {CODEC_LABELS[codec]} <span className="opacity-60">{n}</span>
            </Chip>
          ))}
      </div>
      <HiddenPeople people={props.hiddenPeople} onUnhide={props.onUnhide} />
    </div>
  )
}

function Chip({ active, children, onClick }: { active: boolean; children: React.ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "flex h-9 shrink-0 items-center gap-1.5 rounded-full border px-3.5 text-[14px] whitespace-nowrap transition-colors",
        active ? "border-transparent bg-foreground text-background" : "bg-card/60 text-muted-foreground",
      )}
    >
      {children}
    </button>
  )
}

function DesktopFilters(props: Props) {
  const { tier, onTier, counts, readyOnly, onReadyOnly, sort, onSort, formats, selectedFormats, onToggleFormat } = props
  return (
    <div className="hidden space-y-3 md:block">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
        <Segmented label="Quality">
          <SegmentButton active={tier === "all"} onClick={() => onTier("all")}>
            All <Count n={counts.all} />
          </SegmentButton>
          {TIERS.map(({ id, label }) => (
            <SegmentButton key={id} active={tier === id} onClick={() => onTier(id)} disabled={counts[id] === 0}>
              <span className={cn("size-2 rounded-full", TIER_BG[id])} aria-hidden />
              {label} <Count n={counts[id]} />
            </SegmentButton>
          ))}
        </Segmented>

        <label className="flex cursor-pointer items-center gap-2.5 text-sm text-muted-foreground select-none">
          <Switch checked={readyOnly} onCheckedChange={onReadyOnly} />
          Ready to send now
        </label>

        <div className="ml-auto">
          <Segmented label="Sort">
            {(Object.keys(SORTS) as SortKey[]).map((key) => (
              <SegmentButton key={key} active={sort === key} onClick={() => onSort(key)}>
                {SORTS[key].label}
              </SegmentButton>
            ))}
          </Segmented>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
        <TextFilter text={props.text} onText={props.onText} className="w-72" />
        {formats.length > 1 && (
          <Segmented label="Formats">
            {formats.map(([codec, n]) => (
              <SegmentButton key={codec} active={selectedFormats.has(codec)} onClick={() => onToggleFormat(codec)}>
                {CODEC_LABELS[codec]} <Count n={n} />
              </SegmentButton>
            ))}
          </Segmented>
        )}
        <HiddenPeople people={props.hiddenPeople} onUnhide={props.onUnhide} />
      </div>
    </div>
  )
}

function Segmented({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div
      role="group"
      aria-label={label}
      className="flex flex-wrap items-center gap-0.5 rounded-xl border bg-card/60 p-1"
    >
      {children}
    </div>
  )
}

function SegmentButton({
  active,
  children,
  ...props
}: { active: boolean } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      aria-pressed={active}
      className={cn(
        "flex h-8 items-center gap-2 rounded-lg px-3 text-[13.5px] whitespace-nowrap transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40",
        active
          ? "bg-accent text-foreground shadow-[0_1px_0_0_rgb(255_255_255/0.05)_inset]"
          : "text-muted-foreground hover:text-foreground",
      )}
      {...props}
    >
      {children}
    </button>
  )
}

function Count({ n }: { n: number }) {
  return <span className="text-muted-foreground/70">{n.toLocaleString()}</span>
}
