import { CircleAlert, X } from "lucide-react"

import { dismiss, useToasts } from "@/lib/toast"
import { cn } from "@/lib/utils"

/** Messages from anywhere in the app, just above the bottom bar on phones. */
export function Toaster() {
  const toasts = useToasts()
  if (!toasts.length) return null
  return (
    <div
      aria-live="polite"
      className="pointer-events-none fixed inset-x-0 bottom-[calc(var(--chrome-bottom)+0.75rem)] z-[60] flex flex-col items-center gap-2 px-4 md:bottom-6"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          role={t.tone === "error" ? "alert" : "status"}
          className={cn(
            "pointer-events-auto flex w-full max-w-md items-start gap-3 rounded-2xl border px-4 py-3 text-[14px] shadow-[0_20px_60px_-15px_rgb(0_0_0/0.7)] backdrop-blur-xl",
            "animate-in fade-in slide-in-from-bottom-2",
            t.tone === "error" ? "border-destructive/30 bg-card/95" : "bg-card/95",
          )}
        >
          {t.tone === "error" && <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />}
          <p className="min-w-0 flex-1">{t.message}</p>
          <button
            type="button"
            onClick={() => dismiss(t.id)}
            aria-label="Dismiss"
            className="-mr-1 rounded-md p-1 text-muted-foreground hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
      ))}
    </div>
  )
}
