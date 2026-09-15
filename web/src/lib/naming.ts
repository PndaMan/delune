import { useQuery } from "@tanstack/react-query"

import { type NamingOptions, toApiError } from "@/lib/api"

export type NamingSettings = { template: string; options: NamingOptions }

/** The layout an existing library follows, as a template. */
export type DetectedLayout = NamingSettings & {
  matching: number
  sampled: number
  examples: string[]
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return res.json() as Promise<T>
}

export const namingApi = {
  update: (settings: NamingSettings) => call<NamingSettings>("PUT", "/naming", settings),
  detect: () => call<DetectedLayout>("POST", "/naming/detect"),
}

export function useNamingSettings() {
  return useQuery({ queryKey: ["naming"], queryFn: () => call<NamingSettings>("GET", "/naming") })
}
