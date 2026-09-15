import { Link, Outlet } from "@tanstack/react-router"
import { ArrowDownToLine, Inbox, Search, SlidersHorizontal } from "lucide-react"

import { Moon } from "@/components/moon"
import { SoulseekIndicator } from "@/components/soulseek-indicator"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { moonPhase } from "@/lib/moon-phase"

const NAV = [
  { to: "/", label: "Search", icon: Search },
  { to: "/downloads", label: "Downloads", icon: ArrowDownToLine },
  { to: "/review", label: "Review", icon: Inbox },
  { to: "/settings", label: "Settings", icon: SlidersHorizontal },
] as const

export function AppShell() {
  const tonight = moonPhase()

  return (
    <div className="night-sky min-h-dvh md:pl-[76px]">
      <nav
        aria-label="Main"
        className="fixed inset-y-0 left-0 z-30 hidden w-[76px] flex-col items-center border-r bg-card/30 py-5 backdrop-blur-md md:flex"
      >
        <Link to="/" search={{}} aria-label="delune home" className="mb-8 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
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
                      activeOptions={{ exact: true, includeSearch: false }}
                      className="group relative flex size-11 items-center justify-center rounded-xl text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring data-[status=active]:bg-accent data-[status=active]:text-foreground"
                      aria-label={label}
                    />
                  }
                >
                  <span className="absolute -left-[17px] h-5 w-[3px] rounded-r-full bg-primary opacity-0 transition-opacity group-data-[status=active]:opacity-100" />
                  <Icon className="size-[19px]" strokeWidth={1.8} />
                </TooltipTrigger>
                <TooltipContent side="right">{label}</TooltipContent>
              </Tooltip>
            </li>
          ))}
        </ul>
        <div className="mt-auto">
          <SoulseekIndicator />
        </div>
      </nav>

      <main className="pt-[env(safe-area-inset-top)] pb-24 md:pb-0">
        <Outlet />
      </main>

      <nav
        aria-label="Main"
        className="fixed inset-x-0 bottom-0 z-30 flex items-center justify-around border-t bg-card/85 px-2 pt-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] backdrop-blur-md md:hidden"
      >
        {NAV.map(({ to, label, icon: Icon }) => (
          <Link
            key={to}
            to={to}
            activeOptions={{ exact: true, includeSearch: false }}
            className="flex min-w-16 flex-col items-center gap-1 rounded-lg px-3 py-1.5 text-[11.5px] text-muted-foreground data-[status=active]:text-foreground"
          >
            <Icon className="size-5" strokeWidth={1.8} />
            {label}
          </Link>
        ))}
      </nav>
    </div>
  )
}
