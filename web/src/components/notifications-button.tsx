import { useNavigate } from "@tanstack/react-router"
import { Bell, CheckCheck, CircleAlert, CircleCheck, Inbox, MessageSquarePlus, X } from "lucide-react"

import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { type NotificationKind, useClearNotifications, useMarkRead, useNotifications } from "@/lib/notifications"
import { cn } from "@/lib/utils"

const ICONS: Record<NotificationKind, typeof Bell> = {
  "request-new": MessageSquarePlus,
  "request-approved": CircleCheck,
  "request-declined": X,
  "review-ready": Inbox,
  "download-failed": CircleAlert,
  imported: CircleCheck,
}

function ago(at: number) {
  const minutes = Math.round((Date.now() / 1000 - at) / 60)
  if (minutes < 1) return "just now"
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours} h ago`
  return new Date(at * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" })
}

/** A bell with the unread count; opening it lists what happened. */
export function NotificationsButton({ side = "right", className }: { side?: "right" | "bottom"; className?: string }) {
  const navigate = useNavigate()
  const notifications = useNotifications()
  const markRead = useMarkRead()
  const clear = useClearNotifications()
  const unread = notifications.data?.unread ?? 0
  const items = notifications.data?.items ?? []

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={unread ? `Notifications, ${unread} unread` : "Notifications"}
        className={cn(
          "relative flex size-11 items-center justify-center rounded-xl text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
          className,
        )}
      >
        <Bell className="size-[19px]" strokeWidth={1.8} />
        {unread > 0 && (
          <span className="absolute top-1.5 right-1.5 flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-primary px-1 text-[11px] font-semibold text-primary-foreground">
            {unread > 99 ? "99+" : unread}
          </span>
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent side={side} align="end" sideOffset={12} className="w-[min(22rem,calc(100vw-2rem))] p-0">
        <div className="flex items-center gap-2 border-b px-4 py-3">
          <p className="flex-1 text-[15px] font-semibold">Notifications</p>
          {unread > 0 && (
            <button
              type="button"
              onClick={() => markRead.mutate(undefined)}
              className="flex items-center gap-1 text-[12.5px] text-muted-foreground hover:text-foreground"
            >
              <CheckCheck className="size-3.5" /> Mark all read
            </button>
          )}
          {items.length > 0 && unread === 0 && (
            <button
              type="button"
              onClick={() => clear.mutate()}
              className="text-[12.5px] text-muted-foreground hover:text-foreground"
            >
              Clear
            </button>
          )}
        </div>
        <div className="max-h-[min(28rem,70vh)] overflow-y-auto p-1.5">
          {items.length === 0 ? (
            <p className="px-3 py-6 text-center text-sm text-muted-foreground">
              Nothing yet. You'll hear here when downloads are ready and requests are answered.
            </p>
          ) : (
            items.map((n) => {
              const Icon = ICONS[n.kind]
              return (
                <DropdownMenuItem
                  key={n.id}
                  onClick={() => {
                    if (!n.read) markRead.mutate([n.id])
                    if (n.link) void navigate({ to: n.link })
                  }}
                  className="items-start gap-3 rounded-lg px-2.5 py-2.5"
                >
                  <Icon
                    className={cn(
                      "mt-0.5 size-4 shrink-0",
                      n.kind === "download-failed" || n.kind === "request-declined"
                        ? "text-destructive"
                        : "text-primary",
                    )}
                  />
                  <span className="min-w-0 flex-1">
                    <span className={cn("block text-[14px] leading-snug", n.read && "text-muted-foreground")}>
                      {n.title}
                    </span>
                    {n.detail && <span className="mt-0.5 block text-[12.5px] text-muted-foreground">{n.detail}</span>}
                    <span className="mt-0.5 block text-[12px] text-muted-foreground/80">{ago(n.at)}</span>
                  </span>
                  {!n.read && <span className="mt-1.5 size-2 shrink-0 rounded-full bg-primary" aria-label="Unread" />}
                </DropdownMenuItem>
              )
            })
          )}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
