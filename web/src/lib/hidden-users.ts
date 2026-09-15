import { useCallback, useSyncExternalStore } from "react"

const KEY = "delune.hidden-users"
const listeners = new Set<() => void>()

function read(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "[]")
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : []
  } catch {
    return []
  }
}

let snapshot = read()

function write(next: string[]) {
  snapshot = next
  try {
    localStorage.setItem(KEY, JSON.stringify(next))
  } catch {
    // Storage can be unavailable; hiding still works until the page reloads.
  }
  for (const listener of listeners) listener()
}

/** People whose search results this browser hides. */
export function useHiddenUsers() {
  const hidden = useSyncExternalStore(
    (notify) => {
      listeners.add(notify)
      return () => listeners.delete(notify)
    },
    () => snapshot,
  )
  const hide = useCallback((username: string) => write([...new Set([...snapshot, username])]), [])
  const unhide = useCallback((username: string) => write(snapshot.filter((u) => u !== username)), [])
  return { hidden, hide, unhide }
}
