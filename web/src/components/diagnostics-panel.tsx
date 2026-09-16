import { useQuery } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { ArrowRight, CircleAlert, CircleCheck, Info, LoaderCircle, RefreshCw, TriangleAlert } from "lucide-react"

import { Button } from "@/components/ui/button"
import { toApiError } from "@/lib/api"
import type { CheckState, DiagnosticCheck, Diagnostics } from "@/lib/api.generated"
import { formatAgo } from "@/lib/format"
import { cn } from "@/lib/utils"

const AREAS: { id: string; title: string }[] = [
  { id: "soulseek", title: "Soulseek" },
  { id: "sharing", title: "Sharing" },
  { id: "library", title: "Library" },
  { id: "downloads", title: "Downloads" },
  { id: "services", title: "Services" },
]

const LOOK: Record<CheckState, { icon: typeof Info; tone: string }> = {
  problem: { icon: CircleAlert, tone: "text-destructive" },
  warning: { icon: TriangleAlert, tone: "text-q-hires" },
  ok: { icon: CircleCheck, tone: "text-q-lossless" },
  info: { icon: Info, tone: "text-muted-foreground" },
}

function useDiagnostics() {
  return useQuery({
    queryKey: ["diagnostics"],
    queryFn: async ({ signal }) => {
      const res = await fetch("/api/v1/diagnostics", { signal })
      if (!res.ok) throw await toApiError(res)
      return (await res.json()) as Diagnostics
    },
    refetchInterval: 60_000,
  })
}

/** Everything delune depends on, checked at once, with what to do about anything wrong. */
export function DiagnosticsPanel() {
  const diagnostics = useDiagnostics()

  if (diagnostics.isPending) {
    return (
      <p className="flex items-center gap-2 px-5 py-8 text-muted-foreground">
        <LoaderCircle className="size-4 animate-spin" /> Checking everything
      </p>
    )
  }
  if (diagnostics.isError) {
    return (
      <div className="flex items-center gap-3 px-5 py-6">
        <p className="min-w-0 flex-1 text-destructive">{diagnostics.error.message}</p>
        <Button variant="outline" onClick={() => void diagnostics.refetch()}>
          Try again
        </Button>
      </div>
    )
  }

  const data = diagnostics.data
  const problems = data.checks.filter((c) => c.state === "problem").length
  const warnings = data.checks.filter((c) => c.state === "warning").length
  const overall: CheckState = problems ? "problem" : warnings ? "warning" : "ok"
  const Overall = LOOK[overall].icon
  const headline = problems
    ? `${problems} ${problems === 1 ? "thing needs" : "things need"} fixing`
    : warnings
      ? `${warnings} ${warnings === 1 ? "thing" : "things"} to look at`
      : "Everything's working"

  return (
    <div>
      <div className="flex items-center gap-4 border-b px-5 py-5">
        <span
          className={cn(
            "flex size-12 items-center justify-center rounded-full",
            overall === "problem" ? "bg-destructive/15" : overall === "warning" ? "bg-q-hires/15" : "bg-q-lossless/15",
          )}
        >
          <Overall className={cn("size-6", LOOK[overall].tone)} />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[18px] font-semibold">{headline}</p>
          <p className="text-[13px] text-muted-foreground">
            delune {data.version} · checked {formatAgo(data.checked_at)}
          </p>
        </div>
        <Button
          variant="outline"
          onClick={() => void diagnostics.refetch()}
          disabled={diagnostics.isFetching}
          aria-label="Check again"
          className="max-sm:size-10 max-sm:px-0"
        >
          <RefreshCw className={cn(diagnostics.isFetching && "animate-spin")} />
          <span className="max-sm:hidden">Check again</span>
        </Button>
      </div>

      {AREAS.map((area) => {
        const checks = data.checks.filter((c) => c.area === area.id)
        if (!checks.length) return null
        return (
          <section key={area.id} className="border-b px-5 py-4 last:border-b-0">
            <h3 className="text-[12.5px] font-semibold tracking-wide text-muted-foreground uppercase">{area.title}</h3>
            <ul className="mt-2 space-y-1">
              {checks.map((check) => (
                <CheckRow key={check.id} check={check} />
              ))}
            </ul>
          </section>
        )
      })}
    </div>
  )
}

function CheckRow({ check }: { check: DiagnosticCheck }) {
  const { icon: Icon, tone } = LOOK[check.state]
  const attention = check.state === "problem" || check.state === "warning"
  return (
    <li className={cn("flex gap-3 rounded-xl px-2 py-2.5", attention && "bg-accent/40")}>
      <Icon className={cn("mt-0.5 size-[18px] shrink-0", tone)} />
      <div className="min-w-0 flex-1">
        <p className="text-[15px] font-medium">{check.title}</p>
        <p className="text-[14px] text-pretty text-muted-foreground">{check.summary}</p>
        {check.fix && attention && <p className="mt-1 text-[14px] text-pretty">{check.fix}</p>}
      </div>
      {check.link && attention && (
        <Button
          variant="ghost"
          size="sm"
          nativeButton={false}
          render={<Link to={check.link} />}
          className="shrink-0 self-center"
        >
          Fix <ArrowRight />
        </Button>
      )}
    </li>
  )
}
