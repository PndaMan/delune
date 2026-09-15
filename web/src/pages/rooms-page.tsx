import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { ChevronLeft, LogOut, RefreshCw, Users } from "lucide-react"
import { useEffect, useMemo, useState } from "react"

import { ChatThread } from "@/components/chat-thread"
import { useSoulseekStatus } from "@/components/soulseek-indicator"
import { Button } from "@/components/ui/button"
import { chatApi, chatKeys, type RoomSummary, useChatOverview } from "@/lib/chat"
import { cn } from "@/lib/utils"

/** Chat rooms: the ones delune is in, and the busiest public ones to join. */
export function RoomsPage() {
  const params = useParams({ strict: false }) as { room?: string }
  const room = params.room

  return (
    <div className="mb-4 flex min-h-0 flex-1 overflow-hidden rounded-3xl border bg-card/40 md:mb-8">
      <aside className={cn("flex min-h-0 w-full flex-col border-r lg:w-[300px] lg:shrink-0", room && "hidden lg:flex")}>
        <RoomList active={room} />
      </aside>
      <section className={cn("min-h-0 flex-1", room ? "flex" : "hidden lg:flex")}>
        {room ? (
          <Room key={room} name={room} />
        ) : (
          <div className="flex flex-1 items-center justify-center p-8 text-center text-muted-foreground">
            Pick a room to read along, or join one to talk.
          </div>
        )}
      </section>
    </div>
  )
}

function RoomList({ active }: { active?: string }) {
  const overview = useChatOverview()
  const navigate = useNavigate()
  const client = useQueryClient()
  const [filter, setFilter] = useState("")
  const refresh = useMutation({
    mutationFn: chatApi.refreshRooms,
    onSuccess: () => window.setTimeout(() => void client.invalidateQueries({ queryKey: chatKeys.overview }), 1500),
  })
  const rooms = overview.data?.rooms ?? []
  const needle = filter.trim().toLowerCase()
  const joined = rooms.filter((r) => r.joined)
  const available = rooms.filter((r) => !r.joined && (!needle || r.name.toLowerCase().includes(needle)))

  return (
    <>
      <form
        className="flex shrink-0 gap-2 border-b p-3"
        onSubmit={(e) => {
          e.preventDefault()
          const name = filter.trim()
          if (name) void navigate({ to: "/soulseek/rooms/$room", params: { room: name } })
        }}
      >
        <input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Find or create a room"
          aria-label="Room name"
          className="h-10 min-w-0 flex-1 rounded-xl border bg-background/50 px-3 text-[15px] outline-none focus:border-primary/50"
        />
        <Button type="button" variant="outline" className="h-10 rounded-xl" onClick={() => refresh.mutate()} aria-label="Refresh room list">
          <RefreshCw className={cn(refresh.isPending && "animate-spin")} />
        </Button>
      </form>
      <div className="scrollbar-themed min-h-0 flex-1 overflow-y-auto p-2">
        {joined.length > 0 && <RoomGroup title="Joined" rooms={joined} active={active} />}
        <RoomGroup title="Public rooms" rooms={available} active={active} />
        {available.length === 0 && (
          <p className="px-3 py-6 text-center text-sm text-muted-foreground">
            {needle ? `Press Enter to open “${filter.trim()}”.` : "The room list arrives shortly after connecting."}
          </p>
        )}
      </div>
    </>
  )
}

function RoomGroup({ title, rooms, active }: { title: string; rooms: RoomSummary[]; active?: string }) {
  if (!rooms.length) return null
  return (
    <section className="mb-3">
      <h2 className="px-3 pt-2 pb-1.5 text-[12.5px] text-muted-foreground">{title}</h2>
      <ul>
        {rooms.map((r) => (
          <li key={r.name}>
            <Link
              to="/soulseek/rooms/$room"
              params={{ room: r.name }}
              className={cn(
                "flex items-center gap-3 rounded-xl px-3 py-2 transition-colors hover:bg-accent/60",
                active === r.name && "bg-accent",
              )}
            >
              <span className={cn("min-w-0 flex-1 truncate text-[15px]", r.unread > 0 && "font-semibold")}>{r.name}</span>
              {r.unread > 0 ? (
                <span className="rounded-full bg-primary px-1.5 text-[11.5px] leading-5 font-semibold text-primary-foreground">
                  {r.unread > 99 ? "99+" : r.unread}
                </span>
              ) : (
                <span className="text-[12.5px] text-muted-foreground">{r.members.toLocaleString()}</span>
              )}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  )
}

function Room({ name }: { name: string }) {
  const client = useQueryClient()
  const status = useSoulseekStatus()
  const [showMembers, setShowMembers] = useState(false)
  const room = useQuery({ queryKey: chatKeys.room(name), queryFn: () => chatApi.room(name) })
  const refresh = () => {
    void client.invalidateQueries({ queryKey: chatKeys.overview })
    void client.invalidateQueries({ queryKey: chatKeys.room(name) })
  }
  useEffect(() => {
    if (room.dataUpdatedAt) void client.invalidateQueries({ queryKey: chatKeys.overview })
  }, [room.dataUpdatedAt, client])
  const join = useMutation({ mutationFn: () => chatApi.join(name), onSuccess: refresh })
  const leave = useMutation({ mutationFn: () => chatApi.leave(name), onSuccess: refresh })
  const members = useMemo(
    () => [...(room.data?.members ?? [])].sort((a, b) => a.username.localeCompare(b.username, undefined, { sensitivity: "base" })),
    [room.data?.members],
  )
  const joined = room.data?.joined ?? false

  return (
    <div className="flex min-h-0 flex-1">
      <div className="flex min-h-0 flex-1 flex-col">
        <header className="flex shrink-0 items-center gap-2 border-b px-3 py-2.5 sm:px-5">
          <Link to="/soulseek/rooms" className="rounded-lg p-1.5 text-muted-foreground hover:text-foreground lg:hidden" aria-label="All rooms">
            <ChevronLeft className="size-5" />
          </Link>
          <div className="min-w-0 flex-1">
            <p className="truncate text-[16px] font-semibold">{name}</p>
            {joined && <p className="text-[12.5px] text-muted-foreground">{members.length.toLocaleString()} here</p>}
          </div>
          {joined ? (
            <>
              <Button variant="ghost" size="sm" onClick={() => setShowMembers((v) => !v)} className="xl:hidden" aria-pressed={showMembers}>
                <Users /> <span className="hidden sm:inline">People</span>
              </Button>
              <Button variant="ghost" size="sm" onClick={() => leave.mutate()} disabled={leave.isPending} className="text-muted-foreground">
                <LogOut /> <span className="hidden sm:inline">Leave</span>
              </Button>
            </>
          ) : (
            <Button size="sm" onClick={() => join.mutate()} disabled={join.isPending || status.data?.state !== "online"}>
              Join room
            </Button>
          )}
        </header>
        {join.isError && <p className="border-b px-5 py-2 text-sm text-destructive">{join.error.message}</p>}
        {joined ? (
          <ChatThread
            messages={room.data?.messages ?? []}
            self={status.data?.username ?? null}
            placeholder={`Say something in ${name}`}
            disabled={status.data?.state !== "online"}
            empty={<>You're in. New messages show up here.</>}
            onSend={(text) => chatApi.say(name, text)}
          />
        ) : (
          <div className="flex flex-1 items-center justify-center p-8 text-center text-muted-foreground">
            Join {name} to see what people are saying. delune rejoins it whenever it reconnects.
          </div>
        )}
      </div>
      {joined && (
        <aside className={cn("scrollbar-themed min-h-0 w-60 shrink-0 overflow-y-auto border-l p-2", showMembers ? "block" : "hidden xl:block")}>
          <h2 className="px-2 pt-1 pb-2 text-[12.5px] text-muted-foreground">People here</h2>
          <ul>
            {members.map((m) => (
              <li key={m.username}>
                <Link
                  to="/soulseek/users/$username"
                  params={{ username: m.username }}
                  className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-[14px] hover:bg-accent/60"
                >
                  <span
                    className={cn("size-2 shrink-0 rounded-full", m.presence === "online" ? "bg-q-lossless" : m.presence === "away" ? "bg-q-hires" : "bg-muted-foreground/40")}
                    aria-hidden
                  />
                  <span className="truncate">{m.username}</span>
                </Link>
              </li>
            ))}
          </ul>
        </aside>
      )}
    </div>
  )
}
