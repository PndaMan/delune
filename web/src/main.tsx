import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import { TooltipProvider } from "@/components/ui/tooltip"
import { router } from "@/router"
import "@/index.css"

// Night is the default; the light "blue hour" theme follows an explicit OS preference.
const light = window.matchMedia("(prefers-color-scheme: light)")
const applyTheme = () => document.documentElement.classList.toggle("dark", !light.matches)
applyTheme()
light.addEventListener("change", applyTheme)

// Show scrollbars only while something is being scrolled.
{
  const timers = new WeakMap<Element, number>()
  document.addEventListener(
    "scroll",
    (event) => {
      const target = event.target === document ? document.documentElement : (event.target as Element)
      if (!(target instanceof Element)) return
      target.setAttribute("data-scrolling", "")
      window.clearTimeout(timers.get(target))
      timers.set(target, window.setTimeout(() => target.removeAttribute("data-scrolling"), 900))
    },
    { capture: true, passive: true },
  )
}

// Installable app: the service worker only runs in built bundles, never under Vite's dev server.
if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => void navigator.serviceWorker.register("/sw.js"))
}

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false } },
})

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delay={250}>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>
  </StrictMode>,
)
