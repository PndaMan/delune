import { useMutation, useQueryClient } from "@tanstack/react-query"

import { type Me, toApiError } from "@/lib/api"

export type Theme = "system" | "night" | "blue-hour" | "midnight" | "forest"
export type Accent = "moon" | "aurora" | "dusk" | "ember" | "tide" | "fern"
export type Appearance = { theme: Theme; accent: Accent }

export const THEMES: { id: Theme; label: string; description: string; swatch: [string, string] }[] = [
  { id: "system", label: "System", description: "Night or blue hour, like your device", swatch: ["#0f1226", "#e9ebf4"] },
  { id: "night", label: "Night", description: "The default dark sky", swatch: ["#0f1226", "#161a33"] },
  { id: "blue-hour", label: "Blue hour", description: "Light, just after sunset", swatch: ["#e9ebf4", "#f5f6fa"] },
  { id: "midnight", label: "Midnight", description: "True black for OLED screens", swatch: ["#000000", "#0b0c16"] },
  { id: "forest", label: "Forest", description: "A deep green night", swatch: ["#0b1712", "#11211a"] },
]

export const ACCENTS: { id: Accent; label: string; color: string }[] = [
  { id: "moon", label: "Moon", color: "#ab9dff" },
  { id: "aurora", label: "Aurora", color: "#6fe0d6" },
  { id: "dusk", label: "Dusk", color: "#ff9ec7" },
  { id: "ember", label: "Ember", color: "#ffbe73" },
  { id: "tide", label: "Tide", color: "#8cbcff" },
  { id: "fern", label: "Fern", color: "#9ee29a" },
]

const KEY = "delune.appearance"
const prefersLight = window.matchMedia("(prefers-color-scheme: light)")

function stored(): Appearance {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "null") as Appearance | null
    if (value?.theme && value.accent) return value
  } catch {
    // Fall through to the default.
  }
  return { theme: "system", accent: "moon" }
}

let current = stored()

/** Paint the page for an appearance, and remember it so the next load starts right. */
export function applyAppearance(appearance: Appearance = current) {
  current = appearance
  const root = document.documentElement
  const dark = appearance.theme === "system" ? !prefersLight.matches : appearance.theme !== "blue-hour"
  root.classList.toggle("dark", dark)
  root.classList.toggle("midnight", appearance.theme === "midnight")
  root.classList.toggle("forest", appearance.theme === "forest")
  root.dataset.accent = appearance.accent
  const background = getComputedStyle(root).getPropertyValue("--background").trim()
  document.querySelectorAll('meta[name="theme-color"]').forEach((meta) => meta.setAttribute("content", background))
  try {
    localStorage.setItem(KEY, JSON.stringify(appearance))
  } catch {
    // Storage unavailable: the account still has it.
  }
}

prefersLight.addEventListener("change", () => applyAppearance())

export function useSetAppearance() {
  const client = useQueryClient()
  return useMutation({
    meta: { quiet: true },
    mutationFn: async (appearance: Appearance) => {
      applyAppearance(appearance)
      const res = await fetch("/api/v1/session/appearance", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(appearance),
      })
      if (!res.ok) throw await toApiError(res)
      return appearance
    },
    onSuccess: (appearance) => client.setQueryData<Me | null>(["session"], (me) => (me ? { ...me, appearance } : me)),
  })
}

export function useSetAvatar() {
  const client = useQueryClient()
  return useMutation({
    meta: { quiet: true },
    mutationFn: async (file: File | null) => {
      const res = await fetch("/api/v1/session/avatar", { method: file ? "PUT" : "DELETE", body: file ?? undefined })
      if (!res.ok) throw await toApiError(res)
      return file ? ((await res.json()) as { avatar: number }).avatar : null
    },
    onSuccess: (avatar) => client.setQueryData<Me | null>(["session"], (me) => (me ? { ...me, avatar } : me)),
  })
}

export const avatarUrl = (username: string, version: number | null | undefined) =>
  version ? `/api/v1/avatars/${encodeURIComponent(username)}?v=${version}` : null
