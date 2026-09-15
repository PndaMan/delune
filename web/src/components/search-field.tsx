import { useQuery } from "@tanstack/react-query"
import { Link2, Search, X } from "lucide-react"
import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react"

import { Moon } from "@/components/moon"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { api, type Classification, PROVIDER_NAMES } from "@/lib/api"
import { cn } from "@/lib/utils"

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform)

function useDebounced<T>(value: T, ms: number) {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = window.setTimeout(() => setDebounced(value), ms)
    return () => window.clearTimeout(id)
  }, [value, ms])
  return debounced
}

export type SearchFieldHandle = { focus: () => void }

type SearchFieldProps = {
  initialValue?: string
  onSubmit: (value: string) => void
  /** When set, a small moon in the field waxes with search progress. */
  progress?: number | null
  size?: "lg" | "md"
  /** Called when ↓ is pressed, to move focus into results. */
  onArrowDown?: () => void
  /** Hide the hint line under the field until the text is edited. */
  hideHint?: boolean
}

export const SearchField = forwardRef<SearchFieldHandle, SearchFieldProps>(function SearchField(
  { initialValue = "", onSubmit, progress = null, size = "md", onArrowDown, hideHint = false },
  ref,
) {
  const input = useRef<HTMLInputElement>(null)
  const [value, setValue] = useState(initialValue)
  const debounced = useDebounced(value.trim(), 160)

  useImperativeHandle(ref, () => ({ focus: () => input.current?.focus() }), [])
  useEffect(() => setValue(initialValue), [initialValue])

  const classification = useQuery({
    queryKey: ["classify", debounced],
    queryFn: ({ signal }) => api.classify(debounced, signal),
    enabled: debounced.length > 0,
    placeholderData: (previous) => previous,
    staleTime: Infinity,
  })

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLElement && e.target.closest("input, textarea, [contenteditable]")
      if ((e.key === "k" && (e.metaKey || e.ctrlKey)) || (e.key === "/" && !typing)) {
        e.preventDefault()
        input.current?.focus()
        input.current?.select()
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  const trimmed = value.trim()
  const kind = trimmed ? classification.data?.kind : undefined
  const isLink = kind === "link" || kind === "short-link"
  const large = size === "lg"
  const showHint = !hideHint || trimmed !== initialValue.trim()

  return (
    <form
      role="search"
      onSubmit={(e) => {
        e.preventDefault()
        if (trimmed) onSubmit(trimmed)
      }}
    >
      <label
        className={cn(
          "group relative flex items-center rounded-2xl border bg-card/80 shadow-[0_1px_0_0_rgb(255_255_255/0.04)_inset,0_20px_50px_-30px_rgb(0_0_0/0.6)] backdrop-blur-sm transition-[border-color,box-shadow]",
          "focus-within:border-primary/50 focus-within:shadow-[0_0_0_4px_color-mix(in_oklab,var(--primary)_16%,transparent),0_20px_50px_-30px_rgb(0_0_0/0.6)]",
          large ? "h-[68px] gap-4 px-5" : "h-14 gap-3 px-4",
        )}
      >
        <span className="flex size-6 items-center justify-center">
          {progress !== null ? (
            <Moon illumination={progress} size={22} label="Search progress" />
          ) : isLink ? (
            <Link2 className="size-5 text-primary" aria-hidden />
          ) : (
            <Search className="size-5 text-muted-foreground" aria-hidden />
          )}
        </span>
        <span className="sr-only">Search or paste a link</span>
        <input
          ref={input}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setValue("")
            if (e.key === "ArrowDown" && onArrowDown) {
              e.preventDefault()
              onArrowDown()
            }
          }}
          placeholder="Artist, album, or a link from any service"
          autoFocus={large}
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          className={cn(
            "h-full min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted-foreground/60",
            large ? "text-xl" : "text-[17px]",
          )}
        />
        {trimmed && (
          <button
            type="button"
            onClick={() => {
              setValue("")
              input.current?.focus()
            }}
            className="rounded-md p-1 text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            aria-label="Clear search"
          >
            <X className="size-4" />
          </button>
        )}
        <KbdGroup className="hidden shrink-0 sm:flex">
          <Kbd>{isMac ? "⌘" : "Ctrl"}</Kbd>
          <Kbd>K</Kbd>
        </KbdGroup>
      </label>
      {showHint && (
        <p className={cn("h-5 text-sm text-muted-foreground", large ? "mt-3 px-2" : "mt-2 px-1")} aria-live="polite">
          {trimmed ? hint(classification.data) : null}
        </p>
      )}
    </form>
  )
})

function hint(c: Classification | undefined): string | null {
  switch (c?.kind) {
    case "link":
      return `Looks like a ${PROVIDER_NAMES[c.provider]} ${c.entity}. Finding releases from links arrives soon; type the artist and album for now.`
    case "short-link":
      return `A ${PROVIDER_NAMES[c.provider]} short link. Finding releases from links arrives soon.`
    case "text":
      return "Press Enter to search Soulseek"
    default:
      return null
  }
}
