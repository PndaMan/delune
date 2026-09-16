import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { type DownloadJob, toApiError } from "@/lib/api"

/** A program delune may run to fetch a link, for sources it has no downloader for. */
export type ExternalSource = { enabled: boolean; program: string; arguments: string[] }

export type FetchRequest = { url: string; title: string; artist: string | null }

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return res.json() as Promise<T>
}

export function useExternalSource(enabled: boolean) {
  return useQuery({
    queryKey: ["external"],
    enabled,
    queryFn: () => call<ExternalSource>("GET", "/external"),
  })
}

export function useSaveExternalSource() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (settings: ExternalSource) => call<ExternalSource>("PUT", "/external", settings),
    onSuccess: (next) => client.setQueryData(["external"], next),
  })
}

export function useFetchWithCommand() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (request: FetchRequest) => call<DownloadJob>("POST", "/external/fetch", request),
    onSuccess: () => client.invalidateQueries({ queryKey: ["downloads"] }),
  })
}
