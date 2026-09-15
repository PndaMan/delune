import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"
import type { MinQuality } from "@/lib/wishlist"

export type AutomationSettings = {
  follow_artists: boolean
  quality_upgrades: boolean
  upgrade_to: MinQuality
  auto_download: boolean
}

export type Follow = {
  artist: string
  deezer_id: number
  picture: string | null
  added_by: string
  since: number
  last_checked: number | null
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? { accept: "application/json" } : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 ? undefined : await res.json()) as T
}

export function useAutomation() {
  const client = useQueryClient()
  const settings = useQuery({ queryKey: ["automation"], queryFn: () => call<AutomationSettings>("GET", "/automation") })
  const save = useMutation({
    mutationFn: (next: AutomationSettings) => call<AutomationSettings>("PUT", "/automation", next),
    onSuccess: (next) => client.setQueryData(["automation"], next),
  })
  return { settings, save }
}

export function useFollows() {
  return useQuery({ queryKey: ["follows"], queryFn: () => call<Follow[]>("GET", "/follows") })
}

export function useFollow() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (artist: string) => call<Follow>("POST", "/follows", { artist }),
    onSuccess: () => void client.invalidateQueries({ queryKey: ["follows"] }),
  })
}

export function useUnfollow() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => call<void>("DELETE", `/follows/${id}`),
    onSuccess: () => void client.invalidateQueries({ queryKey: ["follows"] }),
  })
}
