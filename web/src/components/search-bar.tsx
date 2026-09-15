import { useQuery } from "@tanstack/react-query"
import { Link2, Search } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { api, type Classification, PROVIDER_NAMES } from "@/lib/api"
import { cn } from "@/lib/utils"

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform)

/** Debounce a fast-changing value so we don't classify on every keystroke. */
function useDebounced<T>(value: T, ms: number) {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), ms)
    return () => clearTimeout(id)
  }, [value, ms])
  return debounced
}

export function SearchBar({ onSubmit }: { onSubmit: (input: string) => void }) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [value, setValue] = useState("")
  const debounced = useDebounced(value.trim(), 150)

  const classification = useQuery({
    queryKey: ["classify", debounced],
    queryFn: ({ signal }) => api.classify(debounced, signal),
    enabled: debounced.length > 0,
    placeholderData: (prev) => prev,
    staleTime: Infinity,
  })

  // ⌘K / Ctrl+K from anywhere, and "/" when not already typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLElement && e.target.closest("input, textarea, [contenteditable]")
      if ((e.key === "k" && (e.metaKey || e.ctrlKey)) || (e.key === "/" && !typing)) {
        e.preventDefault()
        inputRef.current?.focus()
        inputRef.current?.select()
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  const hint = value.trim() ? describe(classification.data) : null
  const isLink = classification.data?.kind === "link" || classification.data?.kind === "short-link"

  return (
    <form
      role="search"
      onSubmit={(e) => {
        e.preventDefault()
        if (value.trim()) onSubmit(value.trim())
      }}
    >
      <label
        className={cn(
          "group flex h-14 items-center gap-3 rounded-lg border bg-card px-4 transition-colors",
          "focus-within:border-primary/60 focus-within:ring-3 focus-within:ring-primary/15",
        )}
      >
        {isLink && value.trim() ? (
          <Link2 className="size-5 shrink-0 text-primary" aria-hidden="true" />
        ) : (
          <Search className="size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
        )}
        <span className="sr-only">Search or paste a link</span>
        <input
          ref={inputRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setValue("")}
          placeholder="Search or paste a link"
          autoFocus
          autoComplete="off"
          spellCheck={false}
          className="h-full min-w-0 flex-1 bg-transparent text-lg outline-none placeholder:text-muted-foreground/70"
        />
        <KbdGroup className="hidden shrink-0 sm:flex">
          <Kbd>{isMac ? "⌘" : "Ctrl"}</Kbd>
          <Kbd>K</Kbd>
        </KbdGroup>
      </label>
      <p className="mt-2 h-5 px-1 text-sm text-muted-foreground" aria-live="polite">
        {hint}
      </p>
    </form>
  )
}

function describe(c: Classification | undefined): string | null {
  switch (c?.kind) {
    case "link":
      return `${PROVIDER_NAMES[c.provider]} ${c.entity}. Press Enter to find it on Soulseek.`
    case "short-link":
      return `${PROVIDER_NAMES[c.provider]} short link. Press Enter to open it.`
    case "text":
      return "Press Enter to search Soulseek."
    default:
      return null
  }
}
