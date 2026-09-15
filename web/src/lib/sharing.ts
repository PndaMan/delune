import { useQuery } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"

export type SharingSettings = {
  enabled: boolean
  share_name: string
  slots: number
  queue_per_user: number
  speed_limit_kib: number | null
  download_limit_kib: number | null
  refuse_leechers: boolean
  downloads_at_once: number | null
  upnp: boolean
  distributed_children: number
  banned: string[]
  schedule: SpeedSchedule | null
}

/** Speed limits that replace the usual ones between two times of day. */
export type SpeedSchedule = {
  start_minute: number
  end_minute: number
  upload_limit_kib: number | null
  download_limit_kib: number | null
  time_zone: string
}

export type SoulseekStats = {
  shared_files: number
  shared_folders: number
  uploads_running: number
  uploads_waiting: number
  downloads_running: number
  downloaded_bytes: number
  uploaded_bytes: number
  uploads_completed: number
  distributed_children: number
}

export function useSoulseekStats() {
  return useQuery({
    queryKey: ["soulseek-stats"],
    queryFn: () => call<SoulseekStats>("GET", "/soulseek/stats"),
    refetchInterval: 5_000,
  })
}

export type SharingStatus = {
  settings: SharingSettings
  scheduled: boolean
  library_dir: string | null
  scanning: boolean
  files: number
  folders: number
  last_scan: number | null
  error: string | null
}

export type Upload = {
  id: number
  username: string
  filename: string
  size: number
  bytes: number
  status: "queued" | "connecting" | "transferring" | "completed" | "failed" | "cancelled"
  reason: string | null
  queued_at: number
  speed: number
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? { accept: "application/json" } : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 || res.status === 202 ? undefined : await res.json()) as T
}

export const sharingApi = {
  status: () => call<SharingStatus>("GET", "/sharing"),
  update: (settings: SharingSettings) => call<SharingStatus>("PUT", "/sharing", settings),
  rescan: () => call<void>("POST", "/sharing/rescan"),
  uploads: () => call<Upload[]>("GET", "/soulseek/uploads"),
  cancel: (id: number) => call<void>("DELETE", `/soulseek/uploads/${id}`),
  clear: () => call<void>("POST", "/soulseek/uploads/clear"),
}

export function useSharingStatus() {
  return useQuery({
    queryKey: ["sharing"],
    queryFn: sharingApi.status,
    refetchInterval: (query) => (query.state.data?.scanning ? 1_500 : 30_000),
  })
}

export function useUploads() {
  return useQuery({
    queryKey: ["uploads"],
    queryFn: sharingApi.uploads,
    refetchInterval: (query) =>
      query.state.data?.some((u) => u.status === "transferring" || u.status === "connecting") ? 1_000 : 5_000,
  })
}
