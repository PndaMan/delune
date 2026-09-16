import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

import { api, type Me, sessionEvents } from "@/lib/api"
import { applyAppearance } from "@/lib/appearance"

const KEY = ["session"]

/** When someone last signed in, so a stale 401 can't sign them straight back out. */
let signedInAt = 0

/** Who is signed in: `null` when nobody is, `undefined` while finding out. */
export function useSession() {
  const client = useQueryClient()
  const query = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.session(signal),
    staleTime: 5 * 60_000,
    retry: 1,
  })

  // The account's look wins over whatever this browser last used.
  const appearance = query.data?.appearance
  useEffect(() => {
    if (appearance) applyAppearance(appearance)
  }, [appearance])

  // Any request answered with 401 means the session ended (expired, or signed out elsewhere).
  useEffect(() => {
    const onSignedOut = () => {
      if (Date.now() - signedInAt < 3_000) return
      if (client.getQueryData(KEY)) client.setQueryData(KEY, null)
    }
    sessionEvents.addEventListener("signed-out", onSignedOut)
    return () => sessionEvents.removeEventListener("signed-out", onSignedOut)
  }, [client])

  return query
}

/** The signed-in person. Only for components rendered inside the signed-in app. */
export function useMe(): Me {
  const { data } = useQuery({ queryKey: KEY, queryFn: ({ signal }) => api.session(signal), staleTime: 5 * 60_000 })
  if (!data) throw new Error("useMe used outside the signed-in app")
  return data
}

export function useSignIn() {
  const client = useQueryClient()
  return useMutation({
    meta: { quiet: true },
    mutationFn: ({ username, password }: { username: string; password: string }) => api.signIn(username, password),
    onSuccess: (me) => {
      // Start fresh: nothing cached for someone else should linger, but keep the
      // session itself, and ignore 401s from requests that were already in flight.
      signedInAt = Date.now()
      client.removeQueries({ predicate: (query) => query.queryKey[0] !== "session" })
      client.setQueryData(KEY, me)
      void client.invalidateQueries()
    },
  })
}

export function useSignOut() {
  const client = useQueryClient()
  return useMutation({
    meta: { quiet: true },
    mutationFn: () => api.signOut(),
    onSettled: () => {
      // Show the sign-in page first, so nothing still on screen reads a session that
      // has gone; then drop what was cached for this person.
      client.setQueryData(KEY, null)
      window.setTimeout(() => {
        void client.cancelQueries({ predicate: (query) => query.queryKey[0] !== "session" })
        client.removeQueries({ predicate: (query) => query.queryKey[0] !== "session" })
      }, 0)
    },
  })
}

/** Someone else's name when a manager is looking at another person's download. */
export function requesterLabel(me: Me, requestedBy: string | null): string | null {
  return me.permissions.manage && requestedBy && requestedBy !== me.username ? requestedBy : null
}

/** "sam" → "S", for avatars. */
export const initial = (name: string) => (name.trim()[0] ?? "?").toUpperCase()
