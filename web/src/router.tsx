import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router"

import { AppShell } from "@/components/app-shell"
import { DownloadsPage } from "@/pages/downloads-page"
import { ReviewPage } from "@/pages/review-page"
import { SearchPage } from "@/pages/search-page"
import { SettingsPage } from "@/pages/settings-page"
import { SoulseekPage } from "@/pages/soulseek-page"
import { UserPage } from "@/pages/user-page"

const rootRoute = createRootRoute({ component: AppShell })

const searchRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  // `?q=` makes every search a link you can share, bookmark or reload.
  validateSearch: (search: Record<string, unknown>): { q?: string } => {
    const q = typeof search.q === "string" ? search.q.trim() : ""
    return q ? { q } : {}
  },
  component: SearchPage,
})

const downloadsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/downloads", component: DownloadsPage })
const reviewRoute = createRoute({ getParentRoute: () => rootRoute, path: "/review", component: ReviewPage })
const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: SettingsPage })

const soulseekRoute = createRoute({ getParentRoute: () => rootRoute, path: "/soulseek", component: SoulseekPage })
const userRoute = createRoute({ getParentRoute: () => rootRoute, path: "/soulseek/users/$username", component: UserPage })

const routeTree = rootRoute.addChildren([searchRoute, downloadsRoute, reviewRoute, soulseekRoute, userRoute, settingsRoute])

export const router = createRouter({ routeTree, defaultPreload: "intent", scrollRestoration: true })

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}
