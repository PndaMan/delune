import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router"

import { AppShell } from "@/components/app-shell"
import { ArtistPage } from "@/pages/artist-page"
import { AlbumPage, SharePage, SongPage } from "@/pages/music-pages"
import { DownloadsPage } from "@/pages/downloads-page"
import { HistoryPage } from "@/pages/history-page"
import { ReviewPage } from "@/pages/review-page"
import { SearchPage } from "@/pages/search-page"
import { SettingsPage } from "@/pages/settings-page"
import { MessagesPage } from "@/pages/messages-page"
import { RoomsPage } from "@/pages/rooms-page"
import { UploadsPage } from "@/pages/uploads-page"
import { PeopleTab, SoulseekLayout } from "@/pages/soulseek-page"
import { UserPage } from "@/pages/user-page"

const rootRoute = createRootRoute({ component: AppShell })

const searchRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  // `?q=` makes every search a link you can share, bookmark or reload.
  // `?q=` makes a search shareable; `?wishlist` opens the wishlist over it.
  validateSearch: (search: Record<string, unknown>): { q?: string; wishlist?: boolean } => {
    const q = typeof search.q === "string" ? search.q.trim() : ""
    const wishlist = search.wishlist === true || search.wishlist === "true"
    return { ...(q ? { q } : {}), ...(wishlist ? { wishlist: true } : {}) }
  },
  component: SearchPage,
})

const downloadsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/downloads", component: DownloadsPage })
const reviewRoute = createRoute({ getParentRoute: () => rootRoute, path: "/review", component: ReviewPage })
const artistRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/artist/$name",
  component: Artist,
})

function Artist() {
  const { name } = artistRoute.useParams()
  return <ArtistPage name={name} />
}

const albumRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/album/$artist/$title",
  component: Album,
})

function Album() {
  const { artist, title } = albumRoute.useParams()
  return <AlbumPage artist={artist === "-" ? null : artist} title={title} />
}

const songRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/song/$artist/$title",
  validateSearch: (search: Record<string, unknown>): { album?: string } =>
    typeof search.album === "string" && search.album ? { album: search.album } : {},
  component: Song,
})

function Song() {
  const { artist, title } = songRoute.useParams()
  const { album } = songRoute.useSearch()
  return <SongPage artist={artist === "-" ? null : artist} title={title} album={album ?? null} />
}

const shareRoute = createRoute({ getParentRoute: () => rootRoute, path: "/share", component: SharePage })

const historyRoute = createRoute({ getParentRoute: () => rootRoute, path: "/history", component: HistoryPage })
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: () => <SettingsPage />,
})
const settingsSectionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings/$section",
  component: SettingsSection,
})

function SettingsSection() {
  const { section } = settingsSectionRoute.useParams()
  return <SettingsPage section={section} />
}

const soulseekRoute = createRoute({ getParentRoute: () => rootRoute, path: "/soulseek", component: SoulseekLayout })
const peopleRoute = createRoute({ getParentRoute: () => soulseekRoute, path: "/", component: PeopleTab })
const userRoute = createRoute({ getParentRoute: () => soulseekRoute, path: "/users/$username", component: UserPage })
const messagesRoute = createRoute({ getParentRoute: () => soulseekRoute, path: "/messages", component: MessagesPage })
const conversationRoute = createRoute({
  getParentRoute: () => soulseekRoute,
  path: "/messages/$username",
  component: MessagesPage,
})
const roomsRoute = createRoute({ getParentRoute: () => soulseekRoute, path: "/rooms", component: RoomsPage })
const roomRoute = createRoute({ getParentRoute: () => soulseekRoute, path: "/rooms/$room", component: RoomsPage })
const uploadsRoute = createRoute({ getParentRoute: () => soulseekRoute, path: "/uploads", component: UploadsPage })

const routeTree = rootRoute.addChildren([
  searchRoute,
  downloadsRoute,
  reviewRoute,
  soulseekRoute.addChildren([
    peopleRoute,
    userRoute,
    messagesRoute,
    conversationRoute,
    roomsRoute,
    roomRoute,
    uploadsRoute,
  ]),
  artistRoute,
  albumRoute,
  songRoute,
  shareRoute,
  historyRoute,
  settingsRoute,
  settingsSectionRoute,
])

export const router = createRouter({ routeTree, defaultPreload: "intent", scrollRestoration: true })

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}
