import { Link, Outlet } from "@tanstack/react-router"
import { useEffect, useState } from "react"
import { ArrowDownToLine, Earth, Inbox, Search, SlidersHorizontal } from "lucide-react"

import { Moon } from "@/components/moon"
import { MusicViewsProvider } from "@/components/music-views"
import { NotificationsButton } from "@/components/notifications-button"
import { ProfileMenu } from "@/components/profile-menu"
import { Toaster } from "@/components/toaster"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useDownloads } from "@/lib/downloads"
import { useLiveUpdates } from "@/lib/live"
import { moonPhase } from "@/lib/moon-phase"
import { useMe, useSession } from "@/lib/session"
import { setupSkipped, useSetupStatus } from "@/lib/setup"
import { cn } from "@/lib/utils"
import { SetupPage } from "@/pages/setup-page"
import { SignInPage } from "@/pages/sign-in-page"

const NAV = [
  { to: "/", label: "Search", icon: Search },
  { to: "/downloads", label: "Downloads", icon: ArrowDownToLine },
  { to: "/review", label: "Review", icon: Inbox },
  { to: "/soulseek", label: "Soulseek", icon: Earth },
  { to: "/settings", label: "Settings", icon: SlidersHorizontal },
] as const

export function AppShell() {
  const session = useSession()
  if (session.isPending) return <div className="night-sky min-h-dvh" />
  if (session.isError) {
    return (
      <main className="night-sky flex min-h-dvh flex-col items-center justify-center gap-5 px-6 text-center">
        <Moon illumination={0} size={96} />
        <p className="max-w-[40ch] text-muted-foreground">Can't reach the delune server. Check that it's running.</p>
        <Button variant="outline" onClick={() => void session.refetch()}>
          Try again
        </Button>
      </main>
    )
  }
  if (!session.data) return <SignInPage />
  return <SignedIn />
}

/**
 * Whether the on-screen keyboard is up: a text field has focus on a touch screen.
 * The bottom bar steps aside meanwhile, as it does in native apps, instead of
 * riding on top of the keyboard.
 */
function useTyping() {
  const [typing, setTyping] = useState(false)
  useEffect(() => {
    if (!window.matchMedia("(pointer: coarse)").matches) return
    const isField = (el: EventTarget | null) =>
      el instanceof HTMLTextAreaElement ||
      (el instanceof HTMLElement && el.isContentEditable) ||
      (el instanceof HTMLInputElement && !["checkbox", "radio", "button", "submit", "range", "file"].includes(el.type))
    const onIn = (e: FocusEvent) => setTyping(isField(e.target))
    const onOut = (e: FocusEvent) => {
      if (!isField(e.relatedTarget)) setTyping(false)
    }
    document.addEventListener("focusin", onIn)
    document.addEventListener("focusout", onOut)
    return () => {
      document.removeEventListener("focusin", onIn)
      document.removeEventListener("focusout", onOut)
    }
  }, [])
  return typing
}

/** Downloads waiting for someone to review them, for a count on the Review tab. */
function useReviewCount() {
  const downloads = useDownloads()
  return (downloads.data ?? []).filter((job) => job.status === "ready").length
}

function Badge({ count, className }: { count: number; className?: string }) {
  if (!count) return null
  return (
    <span
      className={cn(
        "absolute flex h-[17px] min-w-[17px] items-center justify-center rounded-full bg-primary px-1 text-[10.5px] font-semibold text-primary-foreground",
        className,
      )}
      aria-hidden
    >
      {count > 99 ? "99+" : count}
    </span>
  )
}

function SignedIn() {
  const tonight = moonPhase()
  const reviews = useReviewCount()
  const typing = useTyping()
  useLiveUpdates(true)
  const me = useMe()
  const setup = useSetupStatus(me.permissions.manage)
  const [skipped, setSkipped] = useState(setupSkipped)
  if (setup.data?.needed && !skipped) return <SetupPage status={setup.data} onSkip={() => setSkipped(true)} />

  return (
    <MusicViewsProvider>
      <div
        className={cn(
          "night-sky min-h-dvh md:pl-[76px]",
          // How much of the screen the app's own bars take, so a page can fill exactly
          // what's left instead of guessing and ending up scrollable.
          "[--chrome-bottom:calc(4.25rem+env(safe-area-inset-bottom))] [--chrome-top:calc(3.5rem+env(safe-area-inset-top))]",
          "md:[--chrome-bottom:0px] md:[--chrome-top:0px]",
        )}
      >
        <nav
          aria-label="Main"
          className="fixed inset-y-0 left-0 z-30 hidden w-[76px] flex-col items-center border-r bg-card/30 py-5 backdrop-blur-md md:flex"
        >
          <Link
            to="/"
            search={{}}
            aria-label="delune home"
            className="mb-8 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Moon illumination={Math.max(tonight.illumination, 0.18)} waxing={tonight.waxing} size={30} />
          </Link>
          <ul className="flex flex-col gap-1.5">
            {NAV.map(({ to, label, icon: Icon }) => (
              <li key={to}>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <Link
                        to={to}
                        activeOptions={{ exact: to === "/", includeSearch: false }}
                        className="group relative flex size-11 items-center justify-center rounded-xl text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring data-[status=active]:bg-accent data-[status=active]:text-foreground"
                        aria-label={label}
                      />
                    }
                  >
                    <span className="absolute -left-[17px] h-5 w-[3px] rounded-r-full bg-primary opacity-0 transition-opacity group-data-[status=active]:opacity-100" />
                    <Icon className="size-[19px]" strokeWidth={1.8} />
                    {to === "/review" && <Badge count={reviews} className="-top-0.5 -right-0.5" />}
                  </TooltipTrigger>
                  <TooltipContent side="right">{label}</TooltipContent>
                </Tooltip>
              </li>
            ))}
          </ul>
          <div className="mt-auto flex flex-col items-center gap-3">
            <NotificationsButton />
            <ProfileMenu />
          </div>
        </nav>

        <header className="sticky top-0 z-30 flex h-[var(--chrome-top)] items-center gap-1 border-b bg-background/80 px-3 pt-[env(safe-area-inset-top)] backdrop-blur-xl md:hidden">
          <Link
            to="/"
            search={{}}
            aria-label="delune home"
            className="flex items-center gap-2 rounded-full px-1 outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Moon illumination={Math.max(tonight.illumination, 0.18)} waxing={tonight.waxing} size={22} />
            <span className="type-title text-[17px]">delune</span>
          </Link>
          <div className="ml-auto flex items-center gap-1">
            <NotificationsButton side="bottom" />
            <ProfileMenu side="bottom" />
          </div>
        </header>

        <main className="pb-[var(--chrome-bottom)]">
          <Outlet />
        </main>
        <Toaster />

        <nav
          aria-label="Main"
          className={cn(
            "fixed inset-x-0 bottom-0 z-30 flex h-[var(--chrome-bottom)] items-center justify-around border-t bg-card/85 px-2 pb-[env(safe-area-inset-bottom)] backdrop-blur-md md:hidden",
            typing && "hidden",
          )}
        >
          {NAV.map(({ to, label, icon: Icon }) => (
            <Link
              key={to}
              to={to}
              activeOptions={{ exact: to === "/", includeSearch: false }}
              className="relative flex min-w-16 flex-col items-center gap-1 rounded-lg px-3 py-1 text-[11.5px] text-muted-foreground data-[status=active]:text-foreground"
              aria-label={to === "/review" && reviews ? `${label}, ${reviews} waiting` : undefined}
            >
              <Icon className="size-5" strokeWidth={1.8} />
              {to === "/review" && <Badge count={reviews} className="top-0 right-3" />}
              {label}
            </Link>
          ))}
        </nav>
      </div>
    </MusicViewsProvider>
  )
}
