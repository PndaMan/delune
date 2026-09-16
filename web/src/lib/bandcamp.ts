import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { ApiError, type DownloadJob, toApiError } from "@/lib/api"

/** An album on Bandcamp, to buy there or fetch again. */
export type BandcampOffer = {
  url: string
  title: string
  artist: string
  price: number | null
  currency: string | null
  name_your_price: boolean
  owned: boolean
  purchase: string | null
}

/** Your linked Bandcamp account; the login itself never comes back. */
export type BandcampAccount = {
  linked: boolean
  username: string | null
  name: string | null
  purchases: number
  synced_at: number | null
  syncing: boolean
  problem: string | null
}

export type BandcampPurchase = {
  id: string
  title: string
  artist: string
  purchased_at: number | null
  art: string | null
  url: string | null
  downloadable: boolean
  job: string | null
}

export type LinkBandcamp = { cookie: string }
export type BandcampDownload = { format: string | null }

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1/bandcamp${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (await res.json()) as T
}

/** Whether an album is on Bandcamp. A miss is an answer, not an error. */
export function useBandcampOffer(artist: string | null | undefined, album: string | null | undefined) {
  return useQuery({
    queryKey: ["bandcamp", "offer", artist, album],
    queryFn: async () => {
      try {
        return await call<BandcampOffer>(
          "GET",
          `/release?artist=${encodeURIComponent(artist ?? "")}&album=${encodeURIComponent(album ?? "")}`,
        )
      } catch (e) {
        if (e instanceof ApiError && e.status === 404) return null
        throw e
      }
    },
    enabled: !!artist && !!album,
    staleTime: 60 * 60_000,
    retry: false,
  })
}

export function useBandcampAccount() {
  return useQuery({
    queryKey: ["bandcamp", "account"],
    queryFn: () => call<BandcampAccount>("GET", "/account"),
    staleTime: 60_000,
    // While purchases are being fetched, check back until they're in.
    refetchInterval: (query) => (query.state.data?.syncing ? 2000 : false),
  })
}

export function useBandcampPurchases(enabled: boolean) {
  return useQuery({
    queryKey: ["bandcamp", "purchases"],
    queryFn: () => call<BandcampPurchase[]>("GET", "/purchases"),
    enabled,
    staleTime: 60_000,
  })
}

function useAccountMutation<V>(run: (vars: V) => Promise<BandcampAccount>) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: run,
    onSuccess: (account) => {
      client.setQueryData(["bandcamp", "account"], account)
      void client.invalidateQueries({ queryKey: ["bandcamp"] })
    },
  })
}

export const useLinkBandcamp = () =>
  useAccountMutation((cookie: string) => call<BandcampAccount>("PUT", "/account", { cookie } satisfies LinkBandcamp))
export const useUnlinkBandcamp = () => useAccountMutation(() => call<BandcampAccount>("DELETE", "/account"))
export const useSyncBandcamp = () => useAccountMutation(() => call<BandcampAccount>("POST", "/purchases/sync"))

export function useDownloadPurchase() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ id, format }: { id: string; format?: string }) =>
      call<DownloadJob>("POST", `/purchases/${encodeURIComponent(id)}/download`, {
        format: format ?? null,
      } satisfies BandcampDownload),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["bandcamp", "purchases"] })
      void client.invalidateQueries({ queryKey: ["downloads"] })
    },
  })
}

/** "£7" or "£7.50", in the album's own currency. */
export function formatPrice(price: number, currency: string | null) {
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: currency ?? "USD",
      minimumFractionDigits: Number.isInteger(price) ? 0 : 2,
    }).format(price)
  } catch {
    return `${price} ${currency ?? ""}`.trim()
  }
}
