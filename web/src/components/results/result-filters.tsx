import { Switch } from "@/components/ui/switch"
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
function MobileFilters({ tier, onTier, counts, readyOnly, onReadyOnly, sort, onSort }: Props) {
  return (
    <div className="-mx-4 flex gap-2 overflow-x-auto px-4 pb-1 [scrollbar-width:none] md:hidden [&::-webkit-scrollbar]:hidden">
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

function DesktopFilters({ tier, onTier, counts, readyOnly, onReadyOnly, sort, onSort }: Props) {
  return (
    <div className="hidden flex-wrap items-center gap-x-5 gap-y-3 md:flex">
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
  )
}

function Segmented({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div role="group" aria-label={label} className="flex flex-wrap items-center gap-0.5 rounded-xl border bg-card/60 p-1">
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
        active ? "bg-accent text-foreground shadow-[0_1px_0_0_rgb(255_255_255/0.05)_inset]" : "text-muted-foreground hover:text-foreground",
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
