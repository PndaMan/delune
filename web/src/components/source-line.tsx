import { useQuery } from "@tanstack/react-query"
import { Lock } from "lucide-react"

import { api } from "@/lib/api"
import { cn } from "@/lib/utils"

/**
 * The sources a search will use, in order. Soulseek is always first; streaming
 * services appear dimmed until switched on in settings.
 */
export function SourceLine() {
  const sources = useQuery({ queryKey: ["sources"], queryFn: ({ signal }) => api.sources(signal) })
  if (!sources.data) return <div className="h-7" />

  const enabled = sources.data.filter((s) => s.enabled).sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
  const off = sources.data.filter((s) => !s.enabled)

  return (
    <div className="flex flex-wrap items-center gap-x-1.5 gap-y-2 text-sm">
      <span className="mr-1 text-muted-foreground">Searches</span>
      {enabled.map((s) => (
        <span
          key={s.provider}
          className="inline-flex h-7 items-center gap-1.5 rounded-md bg-primary/12 px-2.5 font-medium text-foreground"
          title={s.role === "peer-to-peer" ? "Always searched first" : undefined}
        >
          {s.name}
          {s.role === "peer-to-peer" && <Lock className="size-3 text-primary" aria-label="always on" />}
        </span>
      ))}
      {off.length > 0 && <span className="mx-1 text-muted-foreground">then, if you turn them on</span>}
      {off.map((s) => (
        <span
          key={s.provider}
          className={cn(
            "inline-flex h-7 items-center rounded-md border border-dashed px-2.5 text-muted-foreground",
          )}
        >
          {s.name}
        </span>
      ))}
    </div>
  )
}
