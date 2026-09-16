import { useQuery } from "@tanstack/react-query"
import { createContext, useContext, useEffect, useState } from "react"

export type Artwork = { artist: string; album: string; thumb: string; cover: string }

/** The text the user searched for. Lets the server confirm an artist when a folder name doesn't say. */
export const SearchContext = createContext<string | null>(null)

/** Cover art for a release, or null when there's no confident match. Cached for the session. */
export function useArtwork(artist: string | null, album: string | null, where?: string) {
  const searched = useContext(SearchContext)
  const context = where ?? searched
  return useQuery({
    queryKey: ["artwork", artist ?? "", album ?? "", context ?? ""],
    enabled: !!album,
    staleTime: Infinity,
    gcTime: 30 * 60_000,
    retry: false,
    queryFn: async ({ signal }): Promise<Artwork | null> => {
      const params = new URLSearchParams({ album: album ?? "" })
      if (artist) params.set("artist", artist)
      if (context) params.set("context", context)
      const res = await fetch(`/api/v1/artwork?${params}`, { signal })
      if (!res.ok) throw new Error(`artwork lookup failed: ${res.status}`)
      return res.json() as Promise<Artwork | null>
    },
  })
}

/**
 * The most characterful colour in an image, as an `rgb()` string. Samples a tiny
 * copy and favours saturated, mid-bright pixels so dark or washed-out covers still
 * give a usable accent. Images come through the same-origin proxy, so the canvas
 * isn't tainted.
 */
export function useAccentColour(src: string | undefined) {
  const [colour, setColour] = useState<string | null>(null)

  useEffect(() => {
    if (!src) return
    let cancelled = false
    const img = new Image()
    img.decoding = "async"
    img.onload = () => {
      if (cancelled) return
      const size = 24
      const canvas = document.createElement("canvas")
      canvas.width = canvas.height = size
      const ctx = canvas.getContext("2d", { willReadFrequently: true })
      if (!ctx) return
      ctx.drawImage(img, 0, 0, size, size)
      const { data } = ctx.getImageData(0, 0, size, size)
      let best = { score: -1, r: 171, g: 157, b: 255 }
      for (let i = 0; i < data.length; i += 4) {
        const [r, g, b] = [data[i], data[i + 1], data[i + 2]]
        const max = Math.max(r, g, b)
        const min = Math.min(r, g, b)
        const saturation = max === 0 ? 0 : (max - min) / max
        const brightness = max / 255
        const score = saturation * (1 - Math.abs(brightness - 0.72))
        if (score > best.score) best = { score, r, g, b }
      }
      // Lift very dark picks so the accent stays readable on the night background.
      const lift = Math.max(0, 170 - Math.max(best.r, best.g, best.b))
      setColour(`rgb(${Math.min(255, best.r + lift)} ${Math.min(255, best.g + lift)} ${Math.min(255, best.b + lift)})`)
    }
    img.src = src
    return () => {
      cancelled = true
    }
  }, [src])

  return src ? colour : null
}
