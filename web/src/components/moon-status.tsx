import { useQuery } from "@tanstack/react-query"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api } from "@/lib/api"

type Phase = "full" | "crescent" | "new"

/**
 * Server connection shown as a moon phase: full when connected, a crescent while
 * connecting, a new moon when the server can't be reached.
 */
export function MoonStatus() {
  const health = useQuery({
    queryKey: ["health"],
    queryFn: ({ signal }) => api.health(signal),
    refetchInterval: 5_000,
    retry: false,
  })

  const phase: Phase = health.isSuccess ? "full" : health.isPending ? "crescent" : "new"
  const label = health.isSuccess
    ? `Connected to delune ${health.data.version}`
    : health.isPending
      ? "Connecting to server"
      : "Can't reach the delune server. Check that it's running."

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            className="flex items-center gap-2 rounded-md px-2 py-1 text-sm text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            aria-label={label}
          />
        }
      >
        <Moon phase={phase} />
        <span className="hidden sm:inline">
          {health.isSuccess ? `v${health.data.version}` : health.isPending ? "Connecting" : "Offline"}
        </span>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

function Moon({ phase }: { phase: Phase }) {
  // The shadow disc slides across the moon; a small offset reads as a crescent.
  const shadowX = { full: 24, crescent: 4.5, new: 0 }[phase]
  return (
    <svg viewBox="0 0 16 16" className="size-3.5" aria-hidden="true">
      <defs>
        <mask id={`moon-${phase}`}>
          <rect width="16" height="16" fill="white" />
          <circle cx={8 + shadowX} cy="7" r="7" fill="black" className="transition-[cx] duration-500" />
        </mask>
      </defs>
      <circle cx="8" cy="8" r="6.5" className="fill-none stroke-moon/30" strokeWidth="1" />
      <circle cx="8" cy="8" r="6.5" className="fill-moon" mask={`url(#moon-${phase})`} />
    </svg>
  )
}
