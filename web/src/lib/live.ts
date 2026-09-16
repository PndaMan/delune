import { useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

/**
 * Keep the screen up to date without polling: the server names what changed on
 * `/api/v1/events` and we refresh just that. The browser reconnects the stream on
 * its own, and a reconnect refreshes everything in case something was missed.
 */
export function useLiveUpdates(enabled: boolean) {
  const client = useQueryClient()
  useEffect(() => {
    if (!enabled) return
    const source = new EventSource("/api/v1/events")
    const refresh = (topic: string) => {
      if (topic === "all") {
        void client.invalidateQueries()
        return
      }
      void client.invalidateQueries({ queryKey: [topic] })
      // A favourite's fresh share list replaces the saved copy on screen.
      if (topic === "favourites") void client.invalidateQueries({ queryKey: ["share-tree"] })
      // A finished download changes what Review and the library show too.
      if (topic === "downloads") {
        void client.invalidateQueries({ queryKey: ["library"] })
        void client.invalidateQueries({ queryKey: ["requests"] })
      }
    }
    source.onmessage = (event) => refresh(event.data)
    source.onopen = () => void client.invalidateQueries()
    return () => source.close()
  }, [client, enabled])
}
