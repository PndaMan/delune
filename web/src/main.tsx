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
