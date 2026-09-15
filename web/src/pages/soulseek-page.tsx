import { useNavigate } from "@tanstack/react-router"
import { ArrowRight, X } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { useRecentList } from "@/lib/recent"
import { initial } from "@/lib/session"
import { PageFrame } from "@/pages/placeholder-pages"

/** The Soulseek network beyond search: people and their shares. Messages, rooms and uploads join here. */
export function SoulseekPage() {
  const navigate = useNavigate()
  const [name, setName] = useState("")
  const { recent, forget } = useRecentList("delune.recent-users")
  const browse = (username: string) => void navigate({ to: "/soulseek/users/$username", params: { username } })

  return (
    <PageFrame title="Soulseek">
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
    </PageFrame>
  )
}
