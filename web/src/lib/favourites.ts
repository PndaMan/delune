import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { toApiError } from "@/lib/api"

/** A Soulseek user someone starred, so delune keeps their shares to hand. */
export type FavouriteUser = {
  username: string
  since: number
  saved_at: number | null
  folders: number
  files: number
}

async function call(method: string, path: string): Promise<FavouriteUser[]> {
  const res = await fetch(`/api/v1/soulseek/favourites${path}`, { method })
  if (!res.ok) throw await toApiError(res)
  return (await res.json()) as FavouriteUser[]
}

export function useFavourites(enabled = true) {
  return useQuery({ queryKey: ["favourites"], queryFn: () => call("GET", ""), enabled, staleTime: 60_000 })
}

/** Star or unstar someone; the list updates straight away and settles when the server answers. */
export function useSetFavourite() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ username, on }: { username: string; on: boolean }) =>
      call(on ? "PUT" : "DELETE", `/${encodeURIComponent(username)}`),
    onMutate: async ({ username, on }) => {
      await client.cancelQueries({ queryKey: ["favourites"] })
      const before = client.getQueryData<FavouriteUser[]>(["favourites"])
      client.setQueryData<FavouriteUser[]>(["favourites"], (list = []) =>
        on
          ? [{ username, since: Date.now() / 1000, saved_at: null, folders: 0, files: 0 }, ...list]
          : list.filter((f) => f.username.toLowerCase() !== username.toLowerCase()),
      )
      return { before }
    },
    onError: (_error, _vars, context) => client.setQueryData(["favourites"], context?.before),
    onSuccess: (list) => client.setQueryData(["favourites"], list),
  })
}
