import { useCallback, useState } from "react"

const KEY = "delune.recent-searches"
const LIMIT = 6

function load(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "[]")
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string").slice(0, LIMIT) : []
  } catch {
    return []
  }
}

/** Recent searches, remembered in this browser only. */
export function useRecentSearches() {
  const [recent, setRecent] = useState(load)

  const remember = useCallback((query: string) => {
    setRecent((current) => {
      const next = [query, ...current.filter((q) => q.toLowerCase() !== query.toLowerCase())].slice(0, LIMIT)
      try {
        localStorage.setItem(KEY, JSON.stringify(next))
      } catch {
        // Storage can be unavailable (private windows); recent searches are a nicety.
      }
      return next
    })
  }, [])

  const forget = useCallback((query: string) => {
    setRecent((current) => {
      const next = current.filter((q) => q !== query)
      try {
        localStorage.setItem(KEY, JSON.stringify(next))
      } catch {
        // See above.
      }
      return next
    })
  }, [])

  return { recent, remember, forget }
}
