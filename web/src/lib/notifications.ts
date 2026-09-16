import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"

export type NotificationKind =
  | "request-new"
  | "request-approved"
  | "request-declined"
  | "review-ready"
  | "download-failed"
  | "imported"
  | "new-release"

export type Notification = {
  id: string
  kind: NotificationKind
  at: number
  title: string
  detail: string | null
  link: string | null
  read: boolean
}

export type Notifications = { unread: number; items: Notification[] }

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 ? undefined : await res.json()) as T
}

export function useNotifications() {
  return useQuery({
    queryKey: ["notifications"],
    queryFn: () => call<Notifications>("GET", "/notifications"),
    refetchInterval: 20_000,
  })
}

export function useMarkRead() {
  const client = useQueryClient()
  return useMutation({
    /** Marks everything read when `ids` is missing. */
    mutationFn: (ids?: string[]) => call<Notifications>("POST", "/notifications/read", { ids: ids ?? null }),
    onSuccess: (next) => client.setQueryData(["notifications"], next),
  })
}

export function useClearNotifications() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: () => call<void>("DELETE", "/notifications"),
    onSuccess: () => client.setQueryData(["notifications"], { unread: 0, items: [] }),
  })
}
