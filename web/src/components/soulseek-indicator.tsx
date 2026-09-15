import { useQuery } from "@tanstack/react-query"

import { Moon } from "@/components/moon"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, type SoulseekStatus } from "@/lib/api"
import { cn } from "@/lib/utils"

export function useSoulseekStatus() {
  return useQuery({
    queryKey: ["soulseek"],
    queryFn: ({ signal }) => api.soulseek(signal),
    refetchInterval: 4_000,
    retry: false,
  })
}

export function describeSoulseek(status: SoulseekStatus | undefined, unreachable: boolean) {
  if (unreachable) return { illumination: 0, text: "Can't reach the delune server", tone: "bad" as const }
  switch (status?.state) {
    case "online":
      return { illumination: 1, text: `Connected to Soulseek as ${status.username}`, tone: "good" as const }
    case "connecting":
      return { illumination: 0.35, text: "Connecting to Soulseek", tone: "wait" as const }
    case "reconnecting":
      return { illumination: 0.35, text: status.message ?? "Reconnecting to Soulseek", tone: "wait" as const }
    case "stopped":
      return { illumination: 0, text: status.message ?? "Soulseek stopped", tone: "bad" as const }
    case "not-configured":
      return { illumination: 0, text: "Soulseek isn't set up", tone: "bad" as const }
    default:
      return { illumination: 0.35, text: "Checking the server", tone: "wait" as const }
  }
}

/** Connection state as a moon: full when online, a crescent while connecting, new when down. */
export function SoulseekIndicator({ showLabel = false, className }: { showLabel?: boolean; className?: string }) {
  const status = useSoulseekStatus()
  const { illumination, text, tone } = describeSoulseek(status.data, status.isError)

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            tabIndex={0}
            className={cn(
              "flex items-center gap-2.5 rounded-lg p-2 text-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring",
              className,
            )}
            aria-label={text}
          />
        }
      >
        <Moon illumination={illumination} size={22} className={cn(tone === "wait" && "animate-pulse")} />
        {showLabel && <span className={cn(tone === "bad" && "text-destructive")}>{text}</span>}
      </TooltipTrigger>
      <TooltipContent side="right">{text}</TooltipContent>
    </Tooltip>
  )
}
