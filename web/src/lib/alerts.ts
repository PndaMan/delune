import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"
import type {
  AlertSettings,
  AlertSettingsUpdate,
  AlertTestResult,
  NotificationKind,
  RadarRelease,
} from "@/lib/api.generated"

export type { AlertSettings, AlertTestResult, NotificationKind, RadarRelease }

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/v1${path}`, {
    method,
    headers: body === undefined ? { accept: "application/json" } : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) throw await toApiError(res)
  return (await res.json()) as T
}

const KEY = ["notifications", "settings"]

export function useAlertSettings() {
  return useQuery({ queryKey: KEY, queryFn: () => call<AlertSettings>("GET", "/notifications/settings") })
}

export function useSaveAlertSettings() {
  const client = useQueryClient()
  return useMutation({
    meta: { quiet: true },
    mutationFn: (update: AlertSettingsUpdate) => call<AlertSettings>("PUT", "/notifications/settings", update),
    onSuccess: (settings) => client.setQueryData(KEY, settings),
  })
}

export function useTestAlerts() {
  return useMutation({
    meta: { quiet: true },
    mutationFn: () => call<AlertTestResult[]>("POST", "/notifications/test"),
  })
}

export function useRadar() {
  return useQuery({
    queryKey: ["radar"],
    queryFn: () => call<RadarRelease[]>("GET", "/radar"),
    staleTime: 30 * 60_000,
    retry: false,
  })
}

/** Readable names for each kind of notification, in the order settings lists them. */
export const KIND_LABELS: [NotificationKind, string][] = [
  ["review-ready", "Downloads ready for review"],
  ["download-failed", "Downloads that failed"],
  ["imported", "Albums added to the library"],
  ["new-release", "New releases from artists you follow"],
  ["request-approved", "Your requests approved"],
  ["request-declined", "Your requests declined"],
  ["request-new", "New requests to decide on"],
]

const DEVICE_KEY = "delune.push-device"

export function pushSupported() {
  return "serviceWorker" in navigator && "PushManager" in window && "Notification" in window
}

/** Installed to the home screen: iPhones only allow push there. */
export function isStandalone() {
  return window.matchMedia("(display-mode: standalone)").matches
}

export function thisDeviceId(): string | null {
  try {
    return localStorage.getItem(DEVICE_KEY)
  } catch {
    return null
  }
}

function keyBytes(base64url: string): Uint8Array<ArrayBuffer> {
  const padded = base64url.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((base64url.length + 3) % 4)
  const raw = atob(padded)
  const bytes = new Uint8Array(new ArrayBuffer(raw.length))
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i)
  return bytes
}

function toBase64url(buffer: ArrayBuffer | null): string {
  if (!buffer) return ""
  let text = ""
  for (const byte of new Uint8Array(buffer)) text += String.fromCharCode(byte)
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "")
}

/** "Firefox on Linux", "Safari on iPhone". */
function deviceLabel() {
  const ua = navigator.userAgent
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /Firefox\//.test(ua)
      ? "Firefox"
      : /Chrome\//.test(ua)
        ? "Chrome"
        : /Safari\//.test(ua)
          ? "Safari"
          : "Browser"
  const system = /iPhone/.test(ua)
    ? "iPhone"
    : /iPad/.test(ua)
      ? "iPad"
      : /Android/.test(ua)
        ? "Android"
        : /Mac OS X/.test(ua)
          ? "Mac"
          : /Windows/.test(ua)
            ? "Windows"
            : /Linux/.test(ua)
              ? "Linux"
              : "this device"
  return `${browser} on ${system}`
}

async function registration() {
  return (await navigator.serviceWorker.getRegistration()) ?? (await navigator.serviceWorker.register("/sw.js"))
}

/** Ask this browser for push notifications and tell delune where to send them. */
export async function subscribeThisDevice(pushKey: string): Promise<AlertSettings> {
  const permission = await Notification.requestPermission()
  if (permission !== "granted") {
    throw new Error("Notifications are blocked for delune in this browser's settings.")
  }
  const reg = await registration()
  await navigator.serviceWorker.ready
  const existing = await reg.pushManager.getSubscription()
  const subscription =
    existing ??
    (await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: keyBytes(pushKey) }))
  const settings = await call<AlertSettings>("POST", "/notifications/devices", {
    endpoint: subscription.endpoint,
    p256dh: toBase64url(subscription.getKey("p256dh")),
    auth: toBase64url(subscription.getKey("auth")),
    label: deviceLabel(),
  })
  const mine = settings.devices.at(-1)
  try {
    if (mine) localStorage.setItem(DEVICE_KEY, mine.id)
  } catch {
    // Remembering which device this is only helps the switch show its state.
  }
  return settings
}

export async function unsubscribeDevice(id: string): Promise<AlertSettings> {
  if (id === thisDeviceId()) {
    const reg = await navigator.serviceWorker.getRegistration()
    await (await reg?.pushManager.getSubscription())?.unsubscribe()
    try {
      localStorage.removeItem(DEVICE_KEY)
    } catch {
      // Nothing to forget.
    }
  }
  return call<AlertSettings>("DELETE", `/notifications/devices/${encodeURIComponent(id)}`)
}
