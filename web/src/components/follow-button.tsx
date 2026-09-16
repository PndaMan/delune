import { ListChecks, ListPlus, LoaderCircle, UserRoundCheck, UserRoundPlus } from "lucide-react"
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
import { toast } from "@/lib/toast"
import { sameTitle, titleKey } from "@/lib/track-name"
import { cn } from "@/lib/utils"

/**
 * Follow an artist, from anywhere they're named — or, given `album`, keep that album
 * complete.
 *
 * The two often sit side by side, so they read differently: a person for the artist
 * (their new releases go on the wishlist) and a checklist for the album (its missing
 * tracks, and any added later, do). Turning one on says what it does, since a phone
 * has no hover to explain it.
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
    toast(
      album
        ? `Keeping ${album} complete: missing tracks, and any added later, go on your wishlist`
        : `Following ${artist}: their new albums and EPs go on your wishlist`,
    )
    setJustFollowed(true)
    window.setTimeout(() => setJustFollowed(false), 900)
  }

  const Icon = busy
    ? LoaderCircle
    : album
      ? following
        ? ListChecks
        : ListPlus
      : following
        ? UserRoundCheck
        : UserRoundPlus
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
            // One nudge when it's switched on, rather than motion on every hover.
            justFollowed && "origin-center animate-[wiggle_0.7s_ease-in-out]",
          )}
          strokeWidth={1.9}
        />
        {following ? (
          confirming ? (
            album ? (
              "Tap to stop"
            ) : (
              "Tap to unfollow"
            )
          ) : (
            <>
              <span className="group-hover:hidden">{album ? "Keeping complete" : "Following artist"}</span>
              <span className="hidden group-hover:inline">{album ? "Stop keeping complete" : "Unfollow artist"}</span>
            </>
          )
        ) : album ? (
          "Keep album complete"
        ) : (
          "Follow artist"
        )}
      </TooltipTrigger>
      <TooltipContent>
        {album
          ? following
            ? "Missing tracks from this album, and any added later, go on your wishlist"
            : "Just this album: get the tracks it's missing, and any added later"
          : following
            ? `New albums and EPs from ${artist} go on your wishlist`
            : `Everything new from ${artist}: their next albums and EPs go on your wishlist`}
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
