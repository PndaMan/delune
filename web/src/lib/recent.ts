import { useCallback, useState } from "react"

import { useMe } from "@/lib/session"

const LIMIT = 6

function load(key: string): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "[]")
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string").slice(0, LIMIT) : []
  } catch {
    return []
  }
}

/** Recent searches, remembered in this browser only. */
export const useRecentSearches = () => useRecentList("delune.recent-searches")

/** A short most-recent-first list kept in this browser's storage under `key`. */
export function useRecentList(name: string) {
  // Per person: a shared browser (a family computer) shouldn't show one person's searches to another.
  const key = `${name}:${useMe().username}`
  const [recent, setRecent] = useState(() => load(key))

  const remember = useCallback((query: string) => {
    setRecent((current) => {
      const next = [query, ...current.filter((q) => q.toLowerCase() !== query.toLowerCase())].slice(0, LIMIT)
      try {
        localStorage.setItem(key, JSON.stringify(next))
      } catch {
        // Storage can be unavailable (private windows); recent searches are a nicety.
      }
      return next
    })
  }, [key])

  const forget = useCallback((query: string) => {
    setRecent((current) => {
      const next = current.filter((q) => q !== query)
      try {
        localStorage.setItem(key, JSON.stringify(next))
      } catch {
        // See above.
      }
      return next
    })
  }, [key])

  return { recent, remember, forget }
}

const LABELS = "delune.recent-labels"

/** Readable names for searches that were links: "No Surprises, Radiohead" instead of a URL. */
export function recentLabel(query: string): string | undefined {
  try {
    const labels = JSON.parse(localStorage.getItem(LABELS) ?? "{}") as Record<string, string>
    return typeof labels[query] === "string" ? labels[query] : undefined
  } catch {
    return undefined
  }
}

export function rememberLabel(query: string, label: string) {
  try {
    const labels = JSON.parse(localStorage.getItem(LABELS) ?? "{}") as Record<string, string>
    if (labels[query] === label) return
    labels[query] = label
    // Keep the map small: drop the oldest entries beyond 50.
    const entries = Object.entries(labels).slice(-50)
    localStorage.setItem(LABELS, JSON.stringify(Object.fromEntries(entries)))
  } catch {
    // Storage unavailable: the URL shows instead.
  }
}
