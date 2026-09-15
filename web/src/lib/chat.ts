import { useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

import { ApiError, toApiError } from "@/lib/api"
import { readEventStream } from "@/lib/sse"

export type ChatMessage = { id: number; at: number; from: string; text: string; outgoing: boolean }
export type ConversationSummary = { username: string; last: ChatMessage | null; unread: number }
export type RoomSummary = { name: string; members: number; joined: boolean; unread: number }
export type ChatOverview = { conversations: ConversationSummary[]; rooms: RoomSummary[] }
export type RoomPerson = {
  username: string
  presence: "online" | "away" | "offline"
  files: number
  avg_speed: number
  country: string | null
}
export type RoomView = { name: string; joined: boolean; members: RoomPerson[]; messages: ChatMessage[] }
type ChatUpdate =
  | { type: "conversation"; username: string; message: ChatMessage }
  | { type: "room"; room: string; message: ChatMessage | null }
  | { type: "rooms" }

const base = "/api/v1/soulseek/chat"
const enc = encodeURIComponent

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${base}${path}`, {
    method,
    headers: body === undefined ? { accept: "application/json" } : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 || res.status === 202 ? undefined : await res.json()) as T
}

export const chatApi = {
  overview: () => call<ChatOverview>("GET", ""),
  conversation: (username: string) => call<ChatMessage[]>("GET", `/users/${enc(username)}`),
  send: (username: string, text: string) => call<ChatMessage>("POST", `/users/${enc(username)}`, { text }),
  forget: (username: string) => call<void>("DELETE", `/users/${enc(username)}`),
  room: (room: string) => call<RoomView>("GET", `/rooms/${enc(room)}`),
  join: (room: string) => call<void>("PUT", `/rooms/${enc(room)}`),
  leave: (room: string) => call<void>("DELETE", `/rooms/${enc(room)}`),
  say: (room: string, text: string) => call<void>("POST", `/rooms/${enc(room)}/messages`, { text }),
  refreshRooms: () => call<void>("POST", "/rooms"),
}

export const chatKeys = {
  overview: ["chat"] as const,
  conversation: (username: string) => ["chat", "user", username] as const,
  room: (room: string) => ["chat", "room", room] as const,
}

export function useChatOverview() {
  return useQuery({ queryKey: chatKeys.overview, queryFn: chatApi.overview, refetchInterval: 60_000 })
}

/** Keep chat queries fresh while a chat page is open. Reconnects if the stream drops. */
export function useChatEvents() {
  const client = useQueryClient()
  useEffect(() => {
    const controller = new AbortController()
    let retry: number | undefined
    const connect = async () => {
      try {
        const res = await fetch(`${base}/events`, { signal: controller.signal, headers: { accept: "text/event-stream" } })
        if (!res.ok) throw await toApiError(res)
        await readEventStream(
          res,
          (data) => {
            const update = JSON.parse(data) as ChatUpdate
            void client.invalidateQueries({ queryKey: chatKeys.overview })
            if (update.type === "conversation") void client.invalidateQueries({ queryKey: chatKeys.conversation(update.username) })
            if (update.type === "room") void client.invalidateQueries({ queryKey: chatKeys.room(update.room) })
          },
          controller.signal,
        )
      } catch (error) {
        if (controller.signal.aborted || (error instanceof ApiError && error.status === 403)) return
      }
      if (!controller.signal.aborted) retry = window.setTimeout(() => void connect(), 3_000)
    }
    void connect()
    return () => {
      controller.abort()
      window.clearTimeout(retry)
    }
  }, [client])
}

/** "14:02", or "Tue 14:02" within a week, or "3 Sep" beyond. */
export function chatTime(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000)
  const now = new Date()
  const time = date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
  if (date.toDateString() === now.toDateString()) return time
  if (now.getTime() - date.getTime() < 6 * 86_400_000) return `${date.toLocaleDateString(undefined, { weekday: "short" })} ${time}`
  return date.toLocaleDateString(undefined, { day: "numeric", month: "short" })
}
