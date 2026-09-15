import { useWindowVirtualizer } from "@tanstack/react-virtual"
import { useEffect, useLayoutEffect, useRef, useState } from "react"

import { ResultRow } from "@/components/results/result-row"
import type { Candidate } from "@/lib/api"

const ROW_HEIGHT = 76

type Props = {
  candidates: Candidate[]
  selected: number
  onSelect: (index: number) => void
  onOpen: (candidate: Candidate) => void
  /** Rows highlight the selection only once someone starts using the keyboard. */
  keyboard: boolean
  onKeyboard: (active: boolean) => void
  onLeaveTop: () => void
  /** Pause keyboard handling, e.g. while a modal is open. */
  paused: boolean
}

/** A virtualised list: 800 results scroll as smoothly as 8. */
export function ResultList({ candidates, selected, onSelect, onOpen, keyboard, onKeyboard, onLeaveTop, paused }: Props) {
  const listRef = useRef<HTMLDivElement>(null)
  const [scrollMargin, setScrollMargin] = useState(0)

  useLayoutEffect(() => {
    const measure = () => {
      const el = listRef.current
      if (el) setScrollMargin(el.getBoundingClientRect().top + window.scrollY)
    }
    measure()
    window.addEventListener("resize", measure)
    return () => window.removeEventListener("resize", measure)
  }, [])

  const virtualizer = useWindowVirtualizer({
    count: candidates.length,
    estimateSize: () => ROW_HEIGHT,
    overscan: 6,
    scrollMargin,
  })

  useEffect(() => {
    if (paused) return
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLElement && e.target.closest("input, textarea, [role=dialog]")) return
      const move = (index: number) => {
        onSelect(index)
        virtualizer.scrollToIndex(index, { align: "auto" })
      }
      if (e.key === "ArrowDown" || e.key === "j") {
        e.preventDefault()
        if (!keyboard) {
          onKeyboard(true)
          return move(Math.min(selected, candidates.length - 1))
        }
        move(Math.min(selected + 1, candidates.length - 1))
      } else if (e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault()
        if (!keyboard) return onKeyboard(true)
        if (selected === 0) return onLeaveTop()
        move(selected - 1)
      } else if (e.key === "Enter" && keyboard && candidates[selected]) {
        e.preventDefault()
        onOpen(candidates[selected])
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [paused, keyboard, selected, candidates, onSelect, onOpen, onKeyboard, onLeaveTop, virtualizer])

  return (
    <div ref={listRef} role="list" aria-label="Search results" className="relative" style={{ height: virtualizer.getTotalSize() }}>
      {virtualizer.getVirtualItems().map((item) => {
        const candidate = candidates[item.index]
        return (
          <div
            key={candidate.id}
            role="listitem"
            className="absolute inset-x-0 top-0 py-0.5"
            style={{ height: ROW_HEIGHT, transform: `translateY(${item.start - scrollMargin}px)` }}
          >
            <ResultRow
              candidate={candidate}
              selected={keyboard && item.index === selected}
              onOpen={() => onOpen(candidate)}
              onHover={() => keyboard && item.index !== selected && onSelect(item.index)}
            />
          </div>
        )
      })}
    </div>
  )
}

export function ResultHeader() {
  return (
    <div className="hidden grid-cols-[52px_112px_minmax(0,1fr)_84px_80px_84px_118px] gap-x-4 pr-5 pb-2 pl-3 text-[12.5px] text-muted-foreground/70 md:grid">
      <span />
      <span>Quality</span>
      <span>Release</span>
      <span>Tracks</span>
      <span>Length</span>
      <span>Size</span>
      <span>Availability</span>
    </div>
  )
}
