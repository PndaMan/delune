import { MutationCache, QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import { TooltipProvider } from "@/components/ui/tooltip"
import { applyAppearance } from "@/lib/appearance"
import { toast } from "@/lib/toast"
import { router } from "@/router"
import "@/index.css"

// Paint the last appearance straight away; the account's arrives with the session.
applyAppearance()

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
      timers.set(
        target,
        window.setTimeout(() => target.removeAttribute("data-scrolling"), 900),
      )
    },
    { capture: true, passive: true },
  )
}

// iOS leaves fixed bars (the bottom nav) floating above where they belong after the
// keyboard closes, until something makes it lay the page out again. Nudge it.
if (/iP(hone|ad|od)/.test(navigator.userAgent) || (navigator.maxTouchPoints > 1 && /Mac/.test(navigator.userAgent))) {
  // Scrolling to where the page already is does nothing, so move a pixel and back:
  // that makes Safari lay out fixed elements against the full screen again.
  const relayout = () =>
    window.requestAnimationFrame(() => {
      window.scrollBy(0, 1)
      window.scrollBy(0, -1)
    })
  document.addEventListener("focusout", (event) => {
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
      window.setTimeout(relayout, 50)
      window.setTimeout(relayout, 400)
    }
  })
  window.visualViewport?.addEventListener("resize", () => {
    if (window.visualViewport && window.visualViewport.height >= window.innerHeight - 1) relayout()
  })
}

// Installable app: the service worker only runs in built bundles, never under Vite's dev server.
if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => void navigator.serviceWorker.register("/sw.js"))
}

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false } },
  // Any action that fails says so, unless the screen already shows the error itself.
  mutationCache: new MutationCache({
    onError: (error, _variables, _context, mutation) => {
      if (mutation.meta?.quiet) return
      toast(error instanceof Error ? error.message : "That didn't work. Try again.", "error")
    },
  }),
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
