import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { type DownloadJobRequest, toApiError } from "@/lib/api"

export type RequestStatus = "pending" | "declined" | "searching" | "downloading" | "review" | "available" | "failed"

export type MusicRequest = {
  id: string
  requested_by: string
  requested_at: number
  title: string
  artist: string | null
  query: string
  link: string | null
  download: DownloadJobRequest | null
  quality_label: string | null
  note: string | null
  status: RequestStatus
  decided_by: string | null
  decided_at: number | null
  reason: string | null
  download_id: string | null
  wishlist_id: string | null
}

export type NewRequest = {
  title: string
  artist: string | null
  query: string
  link?: string | null
  download?: DownloadJobRequest | null
  quality_label?: string | null
  note?: string | null
}

export const REQUEST_STATUS: Record<RequestStatus, string> = {
  pending: "Waiting for approval",
  declined: "Declined",
  searching: "Approved, looking for a copy",
  downloading: "Downloading",
  review: "Waiting in review",
  available: "In the library",
  failed: "Download failed",
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 ? undefined : await res.json()) as T
}

export function useRequests() {
  return useQuery({
    queryKey: ["requests"],
    queryFn: () => call<MusicRequest[]>("GET", "/requests"),
    refetchInterval: 10_000,
  })
}

export function useCreateRequest() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (request: NewRequest) => call<MusicRequest>("POST", "/requests", request),
    onSuccess: () => client.invalidateQueries({ queryKey: ["requests"] }),
  })
}

export function useDecideRequest() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, approve, reason }: { id: string; approve: boolean; reason?: string }) =>
      call<MusicRequest>("POST", `/requests/${encodeURIComponent(id)}/decision`, { approve, reason: reason || null }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["requests"] })
      void client.invalidateQueries({ queryKey: ["downloads"] })
      void client.invalidateQueries({ queryKey: ["wishlist"] })
    },
  })
}

export function useRemoveRequest() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => call<void>("DELETE", `/requests/${encodeURIComponent(id)}`),
    onSuccess: () => client.invalidateQueries({ queryKey: ["requests"] }),
  })
}
