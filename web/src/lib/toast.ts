import { useSyncExternalStore } from "react"

export type Toast = { id: number; message: string; tone: "error" | "info" }

let toasts: Toast[] = []
let next = 1
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((listener) => listener())

/** Show a short message at the bottom of the screen for a few seconds. */
export function toast(message: string, tone: Toast["tone"] = "info") {
  // The same failure twice in a row is one message, not a stack.
  if (toasts.some((t) => t.message === message)) return
  const id = next++
  toasts = [...toasts.slice(-2), { id, message, tone }]
  emit()
  window.setTimeout(() => dismiss(id), tone === "error" ? 6000 : 3500)
}

export function dismiss(id: number) {
  toasts = toasts.filter((t) => t.id !== id)
  emit()
}

export function useToasts() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    () => toasts,
  )
}
