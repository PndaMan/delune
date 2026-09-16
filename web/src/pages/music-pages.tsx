import { useNavigate } from "@tanstack/react-router"

import { AlbumBody, SongBody, useMusicViews } from "@/components/music-views"
import { PageFrame } from "@/pages/placeholder-pages"

/**
 * The album and song views as pages, for links opened from outside delune. In the
 * app the same content appears in a dialog; here it's a page you can share. Both
 * bodies carry their own heading, so the page doesn't add a second one.
 */
function Sheet({ children }: { children: React.ReactNode }) {
  return (
    <div className="mx-auto w-full max-w-[860px] px-4 pt-6 pb-10 sm:px-8 sm:pt-12">
      <div className="overflow-hidden rounded-3xl border bg-card/60">{children}</div>
    </div>
  )
}

export function AlbumPage({ artist, title }: { artist: string | null; title: string }) {
  const views = useMusicViews()
  return (
    <Sheet>
      <AlbumBody album={{ artist, title }} onTrack={views.openTrack} />
    </Sheet>
  )
}

export function SongPage({ artist, title, album }: { artist: string | null; title: string; album: string | null }) {
  return (
    <Sheet>
      <SongBody track={{ artist, title, album }} />
    </Sheet>
  )
}

/**
 * Where Android sends a link shared into delune: hand it to search, which
 * recognises Bandcamp, Spotify, Deezer and the rest.
 */
export function SharePage() {
  const navigate = useNavigate()
  const params = new URLSearchParams(window.location.search)
  const shared = [params.get("url"), params.get("text"), params.get("title")].find((value) => value?.trim())
  const link = shared?.match(/https?:\/\/\S+/)?.[0] ?? shared?.trim() ?? ""
  void navigate({ to: "/", search: link ? { q: link } : {}, replace: true })
  return <PageFrame title="Opening">{null}</PageFrame>
}
