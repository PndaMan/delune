import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { type Candidate, toApiError } from "@/lib/api"

export type MinQuality = "any" | "lossless" | "hi-res"

export type WishlistItem = {
  id: string
  query: string
  added_by: string
  added_at: number
  auto_download: boolean
  min_quality: MinQuality
  paused: boolean
  last_searched: number | null
  last_matches: number
  best: Candidate | null
  download_id: string | null
}

export const MIN_QUALITY_LABELS: Record<MinQuality, string> = {
  any: "Any quality",
  lossless: "Lossless",
  "hi-res": "Hi-res only",
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1/wishlist${path}`, {
    method,
    headers: body === undefined ? { accept: "application/json" } : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (res.status === 204 ? undefined : await res.json()) as T
}

export function useWishlist() {
  return useQuery({ queryKey: ["wishlist"], queryFn: () => call<WishlistItem[]>("GET", ""), refetchInterval: 30_000 })
}

export function useAddToWishlist() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (item: { query: string; auto_download?: boolean; min_quality?: MinQuality }) => call<WishlistItem>("POST", "", item),
    onSuccess: () => void client.invalidateQueries({ queryKey: ["wishlist"] }),
  })
}

export function useUpdateWishlist() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...change }: { id: string; auto_download?: boolean; min_quality?: MinQuality; paused?: boolean }) =>
      call<WishlistItem>("PATCH", `/${encodeURIComponent(id)}`, change),
    onSuccess: () => void client.invalidateQueries({ queryKey: ["wishlist"] }),
  })
}

export function useRemoveFromWishlist() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => call<void>("DELETE", `/${encodeURIComponent(id)}`),
    onSuccess: () => void client.invalidateQueries({ queryKey: ["wishlist"] }),
  })
}
