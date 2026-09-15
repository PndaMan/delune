import { useQuery } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"

export type SetupStatus = {
  needed: boolean
  library_dir: string | null
  soulseek_username: string | null
  soulseek_port: number | null
  navidrome_url: string | null
  navidrome_username: string | null
  locked: { library: boolean; soulseek: boolean; navidrome: boolean }
  config_path: string
  can_restart: boolean
}

export type SetupRequest = {
  library_dir?: string
  soulseek?: { username: string; password: string; port: number | null }
  navidrome?: { url: string; username: string; password: string }
}

export type CheckResult = { ok: boolean; message: string }
export type SetupCheck = { library: CheckResult | null; soulseek: CheckResult | null; navidrome: CheckResult | null }

async function send(method: string, path: string, body: SetupRequest): Promise<{ ok: boolean; check: SetupCheck }> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  })
  // A 422 carries the checks that failed.
  if (res.ok || res.status === 422) return { ok: res.ok, check: (await res.json()) as SetupCheck }
  throw await toApiError(res)
}

export const setupApi = {
  check: (body: SetupRequest) => send("POST", "/setup/check", body),
  save: (body: SetupRequest) => send("PUT", "/setup", body),
}

export function useSetupStatus(enabled: boolean) {
  return useQuery({
    queryKey: ["setup"],
    enabled,
    queryFn: async () => {
      const res = await fetch("/api/v1/setup")
      if (!res.ok) throw await toApiError(res)
      return (await res.json()) as SetupStatus
    },
  })
}

/** Wait for the server to go away and come back after a restart, then reload. */
export async function reloadAfterRestart() {
  await new Promise((resolve) => setTimeout(resolve, 1500))
  for (let attempt = 0; attempt < 90; attempt++) {
    try {
      const res = await fetch("/api/v1/health", { cache: "no-store" })
      if (res.ok) break
    } catch {
      // Still restarting.
    }
    await new Promise((resolve) => setTimeout(resolve, 1000))
  }
  window.location.reload()
}

const SKIPPED = "delune-setup-skipped"

export function setupSkipped() {
  try {
    return localStorage.getItem(SKIPPED) === "1"
  } catch {
    return false
  }
}

export function skipSetup() {
  try {
    localStorage.setItem(SKIPPED, "1")
  } catch {
    // Skipping just won't be remembered.
  }
}
