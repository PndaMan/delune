import { useNavigate } from "@tanstack/react-router"
import { History, LogOut, SlidersHorizontal, Users } from "lucide-react"

import { Moon } from "@/components/moon"
import { describeSoulseek, useSoulseekStatus } from "@/components/soulseek-indicator"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { avatarUrl } from "@/lib/appearance"
import { initial, useMe, useSignOut } from "@/lib/session"
import { cn } from "@/lib/utils"

const TONE_DOT = { good: "bg-q-lossless", wait: "bg-q-hires", bad: "bg-destructive" } as const

/** The signed-in person at the foot of the rail, with Soulseek's status on their avatar. */
export function ProfileMenu({ side = "right" }: { side?: "right" | "bottom" }) {
  const me = useMe()
  const navigate = useNavigate()
  const signOut = useSignOut()
  const status = useSoulseekStatus()
  const soulseek = describeSoulseek(status.data, status.isError)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={`${me.username}. ${soulseek.text}`}
        className="relative flex size-11 items-center justify-center rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Avatar name={me.username} src={avatarUrl(me.username, me.avatar)} />
        <span
          className={cn(
            "absolute right-0.5 bottom-0.5 size-3 rounded-full ring-[3px] ring-background",
            TONE_DOT[soulseek.tone],
          )}
          aria-hidden
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side={side}
        align="end"
        sideOffset={12}
        className="w-72 p-1.5 [&_[role=menuitem]]:gap-3 [&_[role=menuitem]]:px-2.5 [&_[role=menuitem]]:py-2 [&_[role=menuitem]]:text-[14px]"
      >
        <div className="flex items-center gap-3 px-2.5 pt-2 pb-3">
          <Avatar name={me.username} src={avatarUrl(me.username, me.avatar)} />
          <div className="min-w-0">
            <p className="truncate text-[15px] font-semibold">{me.username}</p>
            <p className="text-[13px] text-muted-foreground">
              {me.mode === "open" ? "Open mode, no sign-in" : me.admin ? "Admin" : "Member"}
            </p>
          </div>
        </div>
        <p className="flex items-center gap-2.5 border-t px-2.5 py-3 text-[13px] text-muted-foreground">
          <Moon
            illumination={soulseek.illumination}
            size={16}
            className={cn(soulseek.tone === "wait" && "animate-pulse")}
          />
          <span className={cn("min-w-0", soulseek.tone === "bad" && "text-destructive")}>{soulseek.text}</span>
        </p>
        <DropdownMenuSeparator />
        {me.permissions.manage && (
          <DropdownMenuItem onClick={() => void navigate({ to: "/settings", hash: "people" })}>
            <Users /> People and permissions
          </DropdownMenuItem>
        )}
        <DropdownMenuItem onClick={() => void navigate({ to: "/history" })}>
          <History /> History
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => void navigate({ to: "/settings" })}>
          <SlidersHorizontal /> Settings
        </DropdownMenuItem>
        {me.mode === "navidrome" && (
          <DropdownMenuItem onClick={() => signOut.mutate()}>
            <LogOut /> Sign out
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function Avatar({ name, src, className }: { name: string; src?: string | null; className?: string }) {
  if (src) {
    return <img src={src} alt="" className={cn("size-9 shrink-0 rounded-full object-cover", className)} />
  }
  return (
    <span
      className={cn(
        "flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/20 text-[15px] font-semibold text-primary",
        className,
      )}
      aria-hidden
    >
      {initial(name)}
    </span>
  )
}
