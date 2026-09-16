import { Bell, BellRing, LoaderCircle } from "lucide-react"
import { useState } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import {
  useAlbumFollows,
  useFollow,
  useFollowAlbum,
  useFollows,
  useUnfollow,
  useUnfollowAlbum,
} from "@/lib/automation"
import { useMe } from "@/lib/session"
import { sameTitle, titleKey } from "@/lib/track-name"
import { cn } from "@/lib/utils"

/**
 * Follow an artist, from anywhere they're named — or, given `album`, that album.
 *
 * Following is the whole switch: new albums and EPs from a followed artist, and
 * tracks missing from a followed album (including ones added after release), land on
 * the wishlist by themselves. So the button carries that meaning — a bell that rings
 * once when it's turned on, and says what it does the moment you hover it.
 */
export function FollowButton({
  artist,
  album,
  size = "default",
}: {
  artist: string
  album?: string | null
  size?: "default" | "small"
}) {
  const me = useMe()
  const artistFollow = useArtistFollow(artist)
  const albumFollow = useAlbumFollow(artist, album ?? null)
  const { following, follow, unfollow } = album ? albumFollow : artistFollow
  const [justFollowed, setJustFollowed] = useState(false)
  // Touch screens have no hover to warn with, so unfollowing takes a second tap.
  const [confirming, setConfirming] = useState(false)
  const busy = follow.isPending || unfollow.isPending

  if (!me.permissions.download || !me.permissions.search) return null

  const toggle = () => {
    if (following) {
      if (!confirming && window.matchMedia("(hover: none)").matches) {
        setConfirming(true)
        window.setTimeout(() => setConfirming(false), 3000)
        return
      }
      unfollow.mutate(following)
      setConfirming(false)
      setJustFollowed(false)
      return
    }
    follow.mutate()
    setJustFollowed(true)
    window.setTimeout(() => setJustFollowed(false), 900)
  }

  const Icon = busy ? LoaderCircle : following ? BellRing : Bell
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            onClick={toggle}
            disabled={busy}
            aria-pressed={!!following}
            className={cn(
              "group inline-flex shrink-0 items-center gap-2 rounded-full border font-medium outline-none transition-colors",
              "focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-70",
              size === "small" ? "h-8 px-3 text-[13px]" : "h-9 px-3.5 text-[14px]",
              following
                ? "border-primary/40 bg-primary/15 text-primary hover:border-destructive/40 hover:bg-destructive/10 hover:text-destructive"
                : "border-border bg-card/60 text-foreground hover:border-primary/50 hover:bg-primary/10 hover:text-primary",
            )}
          />
        }
      >
        <Icon
          className={cn(
            size === "small" ? "size-3.5" : "size-4",
            busy && "animate-spin",
            // One ring when it's switched on, rather than motion on every hover.
            justFollowed && "origin-top animate-[wiggle_0.7s_ease-in-out]",
          )}
          strokeWidth={1.9}
        />
        {following ? (
          confirming ? (
            "Tap to unfollow"
          ) : (
            <>
              <span className="group-hover:hidden">{album ? "Following album" : "Following"}</span>
              <span className="hidden group-hover:inline">Unfollow</span>
            </>
          )
        ) : album ? (
          "Follow album"
        ) : (
          "Follow"
        )}
      </TooltipTrigger>
      <TooltipContent>
        {album
          ? following
            ? "Missing tracks, and any the artist adds, go on the wishlist"
            : "Keep this album complete, even when tracks are added later"
          : following
            ? `New releases from ${artist} go on the wishlist`
            : "Put their new releases on the wishlist"}
      </TooltipContent>
    </Tooltip>
  )
}

function useArtistFollow(artist: string) {
  const follows = useFollows()
  const follow = useFollow()
  const unfollow = useUnfollow()
  const found = (follows.data ?? []).find((f) => f.artist.toLowerCase() === artist.toLowerCase())
  return {
    following: found?.deezer_id,
    follow: { isPending: follow.isPending, mutate: () => follow.mutate(artist) },
    unfollow: { isPending: unfollow.isPending, mutate: (id: number) => unfollow.mutate(id) },
  }
}

function useAlbumFollow(artist: string, album: string | null) {
  const follows = useAlbumFollows()
  const follow = useFollowAlbum()
  const unfollow = useUnfollowAlbum()
  const found = album
    ? (follows.data ?? []).find(
        (f) =>
          sameTitle(titleKey(f.title), titleKey(album)) && sameTitle(titleKey(f.artist), titleKey(artist)),
      )
    : undefined
  return {
    following: found?.id,
    follow: { isPending: follow.isPending, mutate: () => album && follow.mutate({ artist, album }) },
    unfollow: { isPending: unfollow.isPending, mutate: (id: number) => unfollow.mutate(id) },
  }
}
