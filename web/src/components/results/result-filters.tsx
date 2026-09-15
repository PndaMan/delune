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

export function ResultFilters({ tier, onTier, counts, readyOnly, onReadyOnly, sort, onSort }: Props) {
  return (
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
