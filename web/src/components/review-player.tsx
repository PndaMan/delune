import { Dialog } from "@base-ui/react/dialog"
import { AudioLines, LoaderCircle, Pause, Play, X } from "lucide-react"
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import { formatTrackTime } from "@/lib/format"
import { cn } from "@/lib/utils"

/** A downloaded file to listen to before it's imported. */
export type Playable = {
  jobId: string
  /** The file's name in the download. */
  file: string
  title: string
  subtitle?: string
  /** Highest frequency with real content, when the check found one. */
  cutoffHz?: number | null
  sampleRate?: number | null
}

const fileUrl = (p: Pick<Playable, "jobId" | "file">) =>
  `/api/v1/downloads/${encodeURIComponent(p.jobId)}/files/${encodeURIComponent(p.file)}`

type Player = {
  current: Playable | null
  playing: boolean
  loading: boolean
  toggle: (track: Playable) => void
  showSpectrogram: (track: Playable) => void
}

const PlayerContext = createContext<Player | null>(null)

export function usePlayer() {
  return useContext(PlayerContext)
}

/**
 * One audio element for a whole page of downloads: a play button on each track, and a
 * bar at the bottom while something plays. Files stream from the server, so playback
 * starts at once and seeking works.
 */
export function ReviewPlayerProvider({ children }: { children: React.ReactNode }) {
  const audio = useRef<HTMLAudioElement>(null)
  const [current, setCurrent] = useState<Playable | null>(null)
  const [playing, setPlaying] = useState(false)
  const [loading, setLoading] = useState(false)
  const [time, setTime] = useState({ at: 0, duration: 0 })
  const [error, setError] = useState<string | null>(null)
  const [spectrogram, setSpectrogram] = useState<Playable | null>(null)

  const toggle = useCallback(
    (track: Playable) => {
      const el = audio.current
      if (!el) return
      if (current && current.jobId === track.jobId && current.file === track.file) {
        if (el.paused) void el.play().catch(() => setError("This browser can't play that file."))
        else el.pause()
        return
      }
      setCurrent(track)
      setError(null)
      setTime({ at: 0, duration: 0 })
      el.src = fileUrl(track)
      setLoading(true)
      void el.play().catch(() => {
        setLoading(false)
        setError("This browser can't play that file.")
      })
    },
    [current],
  )

  const stop = () => {
    audio.current?.pause()
    audio.current?.removeAttribute("src")
    audio.current?.load()
    setCurrent(null)
    setPlaying(false)
  }

  // Leaving the page stops the music.
  useEffect(() => () => audio.current?.pause(), [])
  // Sticky bars move up while the player is showing, rather than sit under it.
  useEffect(() => {
    const root = document.documentElement
    if (current) root.style.setProperty("--player-height", "5.75rem")
    else root.style.removeProperty("--player-height")
    return () => {
      root.style.removeProperty("--player-height")
    }
  }, [current])

  const value = useMemo<Player>(
    () => ({ current, playing, loading, toggle, showSpectrogram: setSpectrogram }),
    [current, playing, loading, toggle],
  )

  return (
    <PlayerContext.Provider value={value}>
      {children}
      <audio
        ref={audio}
        preload="none"
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onPlaying={() => setLoading(false)}
        onWaiting={() => setLoading(true)}
        onEnded={() => setPlaying(false)}
        onError={() => {
          if (!current) return
          setLoading(false)
          setError("This browser can't play that file.")
        }}
        onTimeUpdate={(e) => setTime({ at: e.currentTarget.currentTime, duration: e.currentTarget.duration || 0 })}
        onLoadedMetadata={(e) => setTime({ at: 0, duration: e.currentTarget.duration || 0 })}
      />
      {current && (
        <div className="fixed inset-x-0 bottom-[var(--chrome-bottom)] z-40 px-3 pb-3 md:left-[76px] md:px-8 md:pb-5">
          <div className="mx-auto flex max-w-3xl items-center gap-3 rounded-2xl border bg-card/95 p-2.5 pr-3 shadow-2xl backdrop-blur-xl">
            <Button
              size="icon"
              className="size-11 shrink-0 rounded-full"
              onClick={() => toggle(current)}
              aria-label={playing ? "Pause" : "Play"}
            >
              {loading ? <LoaderCircle className="animate-spin" /> : playing ? <Pause /> : <Play />}
            </Button>
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline gap-2">
                <p className="min-w-0 flex-1 truncate text-[14.5px] font-medium">{current.title}</p>
                <span className="shrink-0 text-[12px] text-muted-foreground tabular-nums">
                  {formatTrackTime(Math.floor(time.at)) || "0:00"} / {formatTrackTime(Math.floor(time.duration)) || "–"}
                </span>
              </div>
              {error ? (
                <p className="truncate text-[12.5px] text-destructive">{error}</p>
              ) : (
                <input
                  type="range"
                  min={0}
                  max={time.duration || 1}
                  step={0.1}
                  value={time.at}
                  aria-label="Position"
                  onChange={(e) => {
                    if (audio.current) audio.current.currentTime = Number(e.target.value)
                  }}
                  className="mt-1 h-4 w-full cursor-pointer accent-primary"
                />
              )}
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setSpectrogram(current)}
              aria-label="Show the spectrogram"
              title="Spectrogram"
              className="text-muted-foreground"
            >
              <AudioLines />
            </Button>
            <Button variant="ghost" size="icon-sm" onClick={stop} aria-label="Stop" className="text-muted-foreground">
              <X />
            </Button>
          </div>
        </div>
      )}
      <SpectrogramDialog track={spectrogram} onClose={() => setSpectrogram(null)} />
    </PlayerContext.Provider>
  )
}

/** A play button for a track row; shows the track number when idle. */
export function PlayButton({ track, label }: { track: Playable; label: React.ReactNode }) {
  const player = usePlayer()
  if (!player) return <span>{label}</span>
  const active = player.current?.jobId === track.jobId && player.current.file === track.file
  const Icon = active && player.loading ? LoaderCircle : active && player.playing ? Pause : Play
  return (
    <button
      type="button"
      onClick={() => player.toggle(track)}
      aria-label={active && player.playing ? `Pause ${track.title}` : `Play ${track.title}`}
      className={cn(
        "group/play relative flex size-7 items-center justify-center rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring",
        active ? "bg-primary text-primary-foreground" : "hover:bg-accent",
      )}
    >
      <span className={cn("text-[13px] text-muted-foreground/70", active ? "hidden" : "group-hover/play:hidden [@media(hover:none)]:hidden")}>
        {label}
      </span>
      <Icon
        className={cn(
          "size-3.5",
          active ? "" : "hidden group-hover/play:block [@media(hover:none)]:block",
          active && player.loading && "animate-spin",
        )}
        fill={active && player.playing ? undefined : "currentColor"}
      />
    </button>
  )
}

/** The frequencies of a file over time, with the scale marked, as a lossy file gives itself away. */
function SpectrogramDialog({ track, onClose }: { track: Playable | null; onClose: () => void }) {
  const [loaded, setLoaded] = useState<string | null>(null)
  const [failed, setFailed] = useState<string | null>(null)
  const src = track ? `${fileUrl(track)}/spectrogram` : null
  const ready = src !== null && loaded === src
  const nyquist = (track?.sampleRate ?? 44_100) / 2
  const cutoffShare = track?.cutoffHz ? Math.min(1, track.cutoffHz / nyquist) : null
  const khz = (share: number) => {
    const value = (nyquist * share) / 1000
    return value >= 10 ? Math.round(value).toString() : value.toFixed(1).replace(/\.0$/, "")
  }

  return (
    <Dialog.Root open={track !== null} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup className="fixed top-1/2 left-1/2 z-50 w-[min(960px,calc(100vw-24px))] -translate-x-1/2 -translate-y-1/2 rounded-3xl border bg-card p-5 shadow-2xl outline-none sm:p-6">
          <div className="flex items-start gap-3">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="type-title truncate text-[20px]">{track?.title}</Dialog.Title>
              <Dialog.Description className="text-[13.5px] text-muted-foreground">
                Frequencies over time. A lossy file shows a flat ceiling well below the top.
              </Dialog.Description>
            </div>
            <Dialog.Close
              className="flex size-9 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
              aria-label="Close"
            >
              <X className="size-5" />
            </Dialog.Close>
          </div>
          <div className="mt-4 flex gap-2">
            <div className="flex flex-col justify-between py-0.5 text-right text-[11px] text-muted-foreground tabular-nums">
              <span>{khz(1)} kHz</span>
              <span>{khz(0.75)}</span>
              <span>{khz(0.5)}</span>
              <span>{khz(0.25)}</span>
              <span>0</span>
            </div>
            <div className="relative aspect-[900/320] min-w-0 flex-1 overflow-hidden rounded-xl bg-black">
              {src && failed !== src && (
                <img
                  key={src}
                  src={src}
                  alt={`Spectrogram of ${track?.title ?? "the track"}`}
                  onLoad={() => setLoaded(src)}
                  onError={() => setFailed(src)}
                  className={cn("size-full object-fill transition-opacity", ready ? "opacity-100" : "opacity-0")}
                />
              )}
              {!ready && failed !== src && (
                <p className="absolute inset-0 flex items-center justify-center gap-2 text-[13.5px] text-white/70">
                  <LoaderCircle className="size-4 animate-spin" /> Drawing it; long tracks take a few seconds
                </p>
              )}
              {failed === src && (
                <p className="absolute inset-0 flex items-center justify-center text-[13.5px] text-white/70">
                  This file couldn't be drawn.
                </p>
              )}
              {ready && cutoffShare !== null && cutoffShare < 0.97 && (
                <div
                  className="absolute inset-x-0 border-t border-dashed border-white/70"
                  style={{ bottom: `${cutoffShare * 100}%` }}
                >
                  <span className="absolute right-2 bottom-1 rounded bg-black/60 px-1.5 text-[11px] text-white">
                    content ends at {Math.round((track?.cutoffHz ?? 0) / 100) / 10} kHz
                  </span>
                </div>
              )}
            </div>
          </div>
          {track?.subtitle && <p className="mt-3 text-[13px] text-muted-foreground">{track.subtitle}</p>}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
