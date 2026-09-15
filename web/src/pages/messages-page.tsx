import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { ChevronLeft, FolderSearch, PenSquare, Trash2 } from "lucide-react"
import { useEffect, useState } from "react"

import { ChatThread } from "@/components/chat-thread"
import { Avatar } from "@/components/profile-menu"
import { useSoulseekStatus } from "@/components/soulseek-indicator"
import { Button } from "@/components/ui/button"
import { chatApi, chatKeys, chatTime, useChatOverview } from "@/lib/chat"
import { cn } from "@/lib/utils"

/** Private conversations on delune's Soulseek account. */
export function MessagesPage() {
  const params = useParams({ strict: false }) as { username?: string }
  const username = params.username

  return (
    <div className="mb-4 flex min-h-0 flex-1 overflow-hidden rounded-3xl border bg-card/40 md:mb-8">
      <aside className={cn("flex min-h-0 w-full flex-col border-r lg:w-[320px] lg:shrink-0", username && "hidden lg:flex")}>
        <Conversations active={username} />
      </aside>
      <section className={cn("min-h-0 flex-1 flex-col", username ? "flex" : "hidden lg:flex")}>
        {username ? (
          <Conversation key={username} username={username} />
        ) : (
          <div className="flex flex-1 items-center justify-center p-8 text-center text-muted-foreground">
            Pick a conversation, or start one with a Soulseek username.
          </div>
        )}
      </section>
    </div>
  )
}

function Conversations({ active }: { active?: string }) {
  const overview = useChatOverview()
  const navigate = useNavigate()
  const [name, setName] = useState("")

  return (
    <>
      <form
        className="flex shrink-0 gap-2 border-b p-3"
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) void navigate({ to: "/soulseek/messages/$username", params: { username: name.trim() } })
          setName("")
        }}
      >
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Message someone"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          aria-label="Soulseek username to message"
          className="h-10 min-w-0 flex-1 rounded-xl border bg-background/50 px-3 text-[15px] outline-none focus:border-primary/50"
        />
        <Button type="submit" variant="outline" className="h-10 rounded-xl" disabled={!name.trim()} aria-label="Start conversation">
          <PenSquare />
        </Button>
      </form>
      <ul className="scrollbar-themed min-h-0 flex-1 overflow-y-auto p-2">
        {overview.data?.conversations.length === 0 && (
          <li className="px-3 py-8 text-center text-sm text-muted-foreground">No messages yet.</li>
        )}
        {overview.data?.conversations.map((c) => (
          <li key={c.username}>
            <Link
              to="/soulseek/messages/$username"
              params={{ username: c.username }}
              className={cn(
                "flex items-center gap-3 rounded-xl px-3 py-2.5 transition-colors hover:bg-accent/60",
                active === c.username && "bg-accent",
              )}
            >
              <Avatar name={c.username} />
              <span className="min-w-0 flex-1">
                <span className="flex items-baseline gap-2">
                  <span className={cn("truncate text-[15px]", c.unread > 0 && "font-semibold")}>{c.username}</span>
                  {c.last && <span className="ml-auto shrink-0 text-[12px] text-muted-foreground">{chatTime(c.last.at)}</span>}
                </span>
                <span className={cn("block truncate text-[13.5px]", c.unread > 0 ? "text-foreground/85" : "text-muted-foreground")}>
                  {c.last ? `${c.last.outgoing ? "You: " : ""}${c.last.text}` : "No messages yet"}
                </span>
              </span>
              {c.unread > 0 && <span className="size-2.5 shrink-0 rounded-full bg-primary" aria-label={`${c.unread} unread`} />}
            </Link>
          </li>
        ))}
      </ul>
    </>
  )
}

function Conversation({ username }: { username: string }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const status = useSoulseekStatus()
  const messages = useQuery({
    queryKey: chatKeys.conversation(username),
    queryFn: () => chatApi.conversation(username),
  })
  // Opening a conversation marks it read on the server; refresh the unread counts.
  useEffect(() => {
    if (messages.dataUpdatedAt) void client.invalidateQueries({ queryKey: chatKeys.overview })
  }, [messages.dataUpdatedAt, client])
  const forget = useMutation({
    mutationFn: () => chatApi.forget(username),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: chatKeys.overview })
      void navigate({ to: "/soulseek/messages" })
    },
  })

  return (
    <>
      <header className="flex shrink-0 items-center gap-3 border-b px-3 py-2.5 sm:px-5">
        <Link to="/soulseek/messages" className="rounded-lg p-1.5 text-muted-foreground hover:text-foreground lg:hidden" aria-label="All conversations">
          <ChevronLeft className="size-5" />
        </Link>
        <Avatar name={username} />
        <p className="min-w-0 flex-1 truncate text-[16px] font-semibold">{username}</p>
        <Button variant="ghost" size="sm" nativeButton={false} render={<Link to="/soulseek/users/$username" params={{ username }} />}>
          <FolderSearch /> <span className="hidden sm:inline">Shares</span>
        </Button>
        <Button variant="ghost" size="sm" onClick={() => forget.mutate()} aria-label="Delete conversation" className="text-muted-foreground">
          <Trash2 />
        </Button>
      </header>
      <ChatThread
        messages={messages.data ?? []}
        self={status.data?.username ?? null}
        placeholder={`Message ${username}`}
        disabled={status.data?.state !== "online"}
        empty={<>Say hello to {username}. They'll see it next time they're online.</>}
        onSend={async (text) => {
          await chatApi.send(username, text)
          await client.invalidateQueries({ queryKey: chatKeys.conversation(username) })
        }}
      />
    </>
  )
}
