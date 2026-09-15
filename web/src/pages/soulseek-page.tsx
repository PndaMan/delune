import { Link, Outlet, useNavigate, useRouterState } from "@tanstack/react-router"
import { ArrowRight, X } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { useChatEvents, useChatOverview } from "@/lib/chat"
import { useRecentList } from "@/lib/recent"
import { initial, useMe } from "@/lib/session"
import { cn } from "@/lib/utils"

/**
 * The Soulseek network beyond search: people and their shares, and, for the people
 * who manage delune, messages and rooms on its account.
 */
export function SoulseekLayout() {
  const me = useMe()
  const path = useRouterState({ select: (s) => s.location.pathname })
  // Chat fills the window and scrolls inside; everything else scrolls the page.
  const fill = /^\/soulseek\/(messages|rooms)/.test(path)

  return (
    <div className={cn("mx-auto flex w-full max-w-[1200px] flex-col px-4 sm:px-10", fill && "h-[calc(100dvh-6rem)] md:h-dvh")}>
      <header className="flex shrink-0 flex-wrap items-end justify-between gap-x-8 gap-y-3 pt-8 pb-5 sm:pt-12">
        <h1 className="type-display text-[40px] sm:text-[44px]">Soulseek</h1>
        {me.permissions.manage && <Tabs />}
      </header>
      <div className={cn("flex min-h-0 flex-col", fill && "flex-1")}>
        <Outlet />
      </div>
    </div>
  )
}

function Tabs() {
  useChatEvents()
  const overview = useChatOverview()
  const path = useRouterState({ select: (s) => s.location.pathname })
  const unreadMessages = overview.data?.conversations.reduce((n, c) => n + c.unread, 0) ?? 0
  const unreadRooms = overview.data?.rooms.reduce((n, r) => n + r.unread, 0) ?? 0
  const section = path.startsWith("/soulseek/messages")
    ? "messages"
    : path.startsWith("/soulseek/rooms")
      ? "rooms"
      : path.startsWith("/soulseek/uploads")
        ? "uploads"
        : "people"
  const tab = (active: boolean) =>
    cn(
      "relative flex h-10 items-center gap-2 rounded-xl px-4 text-[14.5px] transition-colors",
      active ? "bg-accent text-foreground" : "text-muted-foreground hover:text-foreground",
    )
  return (
    <nav aria-label="Soulseek" className="-mx-1 flex gap-1 overflow-x-auto rounded-2xl border bg-card/50 p-1 [scrollbar-width:none]">
      <Link to="/soulseek" className={tab(section === "people")} aria-current={section === "people" ? "page" : undefined}>
        People
      </Link>
      <Link to="/soulseek/messages" className={tab(section === "messages")} aria-current={section === "messages" ? "page" : undefined}>
        Messages {unreadMessages > 0 && <Count n={unreadMessages} />}
      </Link>
      <Link to="/soulseek/rooms" className={tab(section === "rooms")} aria-current={section === "rooms" ? "page" : undefined}>
        Rooms {unreadRooms > 0 && <Count n={unreadRooms} />}
      </Link>
      <Link to="/soulseek/uploads" className={tab(section === "uploads")} aria-current={section === "uploads" ? "page" : undefined}>
        Uploads
      </Link>
    </nav>
  )
}

function Count({ n }: { n: number }) {
  return (
    <span className="min-w-5 rounded-full bg-primary px-1.5 text-center text-[11.5px] leading-5 font-semibold text-primary-foreground">
      {n > 99 ? "99+" : n}
    </span>
  )
}

/** Look someone up to browse their shares. */
export function PeopleTab() {
  const navigate = useNavigate()
  const [name, setName] = useState("")
  const { recent, forget } = useRecentList("delune.recent-users")
  const browse = (username: string) => void navigate({ to: "/soulseek/users/$username", params: { username } })

  return (
    <div className="pb-24">
      <p className="max-w-[60ch] text-[15px] text-muted-foreground">
        Look someone up to see their profile and browse everything they share. Any folder opens like a search result, ready
        to download.
      </p>

      <form
        className="mt-8 flex max-w-xl gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) browse(name.trim())
        }}
      >
        <label className="min-w-0 flex-1">
          <span className="sr-only">Soulseek username</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Soulseek username"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            className="h-12 w-full rounded-xl border bg-card/70 px-4 text-[16px] outline-none focus:border-primary/50"
          />
        </label>
        <Button type="submit" size="lg" className="h-12 rounded-xl px-5" disabled={!name.trim()}>
          Browse <ArrowRight />
        </Button>
      </form>

      {recent.length > 0 && (
        <section className="mt-12">
          <h2 className="type-title text-[19px]">Recently browsed</h2>
          <ul className="mt-4 grid gap-2 sm:grid-cols-2">
            {recent.map((username) => (
              <li key={username} className="flex items-center rounded-xl border bg-card/50 transition-colors hover:bg-accent/60">
                <button type="button" onClick={() => browse(username)} className="flex min-w-0 flex-1 items-center gap-3 px-4 py-3 text-left">
                  <span className="flex size-9 items-center justify-center rounded-full bg-primary/15 font-semibold text-primary">
                    {initial(username)}
                  </span>
                  <span className="truncate text-[15px]">{username}</span>
                </button>
                <button
                  type="button"
                  onClick={() => forget(username)}
                  aria-label={`Forget ${username}`}
                  className="mr-2 rounded-md p-2 text-muted-foreground/60 hover:text-foreground"
                >
                  <X className="size-4" />
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}
