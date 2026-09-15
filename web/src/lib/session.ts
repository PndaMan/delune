import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

import { api, type Me, sessionEvents } from "@/lib/api"
import { applyAppearance } from "@/lib/appearance"

const KEY = ["session"]

/** Who is signed in: `null` when nobody is, `undefined` while finding out. */
export function useSession() {
  const client = useQueryClient()
  const query = useQuery({ queryKey: KEY, queryFn: ({ signal }) => api.session(signal), staleTime: 5 * 60_000, retry: 1 })

  // The account's look wins over whatever this browser last used.
  const appearance = query.data?.appearance
  useEffect(() => {
    if (appearance) applyAppearance(appearance)
  }, [appearance])

  // Any request answered with 401 means the session ended (expired, or signed out elsewhere).
  useEffect(() => {
    const onSignedOut = () => {
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
    mutationFn: ({ username, password }: { username: string; password: string }) => api.signIn(username, password),
    onSuccess: (me) => {
      // Start fresh: nothing cached for someone else should linger.
      client.clear()
      client.setQueryData(KEY, me)
    },
  })
}

export function useSignOut() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: () => api.signOut(),
    onSettled: () => {
      client.clear()
      client.setQueryData(KEY, null)
    },
  })
}

/** Someone else's name when a manager is looking at another person's download. */
export function requesterLabel(me: Me, requestedBy: string | null): string | null {
  return me.permissions.manage && requestedBy && requestedBy !== me.username ? requestedBy : null
}

/** "sam" → "S", for avatars. */
export const initial = (name: string) => (name.trim()[0] ?? "?").toUpperCase()
