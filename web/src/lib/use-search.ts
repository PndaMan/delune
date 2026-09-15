import { useEffect, useReducer, useRef } from "react"

import { type Candidate, type ResolvedLink, type SearchEvent, toApiError } from "@/lib/api"
import { readEventStream } from "@/lib/sse"

export type SearchState = {
  status: "idle" | "running" | "done" | "failed"
  query: string | null
  startedAt: number | null
  timeoutSecs: number
  peers: number
  candidates: Candidate[]
  /** Set when the query was a link: what it points at. */
  resolved: ResolvedLink | null
  /** The words actually sent to Soulseek, which differ from the query for links. */
  searchedFor: string | null
  error: string | null
  errorCode: string | null
}

const initial: SearchState = {
  status: "idle",
  query: null,
  startedAt: null,
  timeoutSecs: 20,
  peers: 0,
  candidates: [],
  resolved: null,
  searchedFor: null,
  error: null,
  errorCode: null,
}

type Action =
  | { type: "reset"; query: string | null }
  | { type: "events"; events: SearchEvent[] }
  | { type: "error"; message: string; code: string }

function reducer(state: SearchState, action: Action): SearchState {
  switch (action.type) {
    case "reset":
      return action.query
        ? { ...initial, status: "running", query: action.query, startedAt: Date.now() }
        : initial
    case "error":
      return { ...state, status: "failed", error: action.message, errorCode: action.code }
    case "events": {
      let next = state
      const added: Candidate[] = []
      for (const event of action.events) {
        switch (event.type) {
          case "resolved":
            next = { ...next, resolved: event.link }
            break
          case "started":
            next = { ...next, timeoutSecs: event.timeout_secs, startedAt: Date.now(), searchedFor: event.query }
            break
          case "candidates":
            added.push(...event.items)
            next = { ...next, peers: next.peers + 1 }
            break
          case "finished":
            next = { ...next, status: "done", peers: event.peers }
            break
          case "failed":
            next = { ...next, status: "failed", error: event.error.message, errorCode: event.error.code }
            break
        }
      }
      return added.length ? { ...next, candidates: next.candidates.concat(added) } : next
    }
  }
}

/**
 * Run a Soulseek search for `query` and stream results into state. Changing or
 * clearing the query cancels the previous search. Updates are batched so a burst
 * of peer responses re-renders a few times per second, not once per peer.
 */
export function useSearch(query: string | null): SearchState {
  const [state, dispatch] = useReducer(reducer, initial)
  const pending = useRef<SearchEvent[]>([])

  useEffect(() => {
    dispatch({ type: "reset", query })
    if (!query) return

    const controller = new AbortController()
    const flush = () => {
      if (pending.current.length) {
        dispatch({ type: "events", events: pending.current })
        pending.current = []
      }
    }
    const timer = window.setInterval(flush, 180)

    ;(async () => {
      try {
        const res = await fetch(`/api/v1/search?q=${encodeURIComponent(query)}`, {
          signal: controller.signal,
          headers: { accept: "text/event-stream" },
        })
        if (!res.ok) {
          const error = await toApiError(res)
          dispatch({ type: "error", message: error.message, code: error.code })
          return
        }
        await readEventStream(res, (data) => pending.current.push(JSON.parse(data) as SearchEvent), controller.signal)
        flush()
      } catch (err) {
        if (controller.signal.aborted) return
        flush()
        dispatch({
          type: "error",
          code: "network",
          message: "Lost the connection to the delune server. Check that it's still running.",
        })
        console.error(err)
      }
    })()

    return () => {
      controller.abort()
      window.clearInterval(timer)
      pending.current = []
    }
  }, [query])

  return state
}
