import { Dialog } from "@base-ui/react/dialog"
import { FileImage, File as FileIcon, TriangleAlert, X } from "lucide-react"

import { Cover } from "@/components/cover"
import type { Candidate } from "@/lib/api"
import { useAccentColour, useArtwork } from "@/lib/artwork"
import { formatBytes, formatRuntime, formatSpeed, formatTrackTime, plural } from "@/lib/format"
import { describeQuality, TIER_BG, TIER_TEXT, tierOf } from "@/lib/quality"
import { parseTrackName } from "@/lib/track-name"
import { cn } from "@/lib/utils"

type Props = {
  candidate: Candidate | null
  onClose: () => void
}

/** A release, large: artwork and facts on the left, every file on the right. */
export function ReleaseModal({ candidate, onClose }: Props) {
  return (
    <Dialog.Root open={candidate !== null} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup
          className={cn(
            "fixed inset-0 z-50 m-auto flex h-[100dvh] w-full flex-col overflow-hidden bg-card outline-none",
            "sm:h-[min(92dvh,900px)] sm:w-[min(94vw,1240px)] sm:rounded-3xl sm:border sm:shadow-[0_40px_120px_-20px_rgb(0_0_0/0.8)]",
            "transition-[opacity,scale] duration-200 ease-out data-ending-style:scale-[0.97] data-ending-style:opacity-0 data-starting-style:scale-[0.97] data-starting-style:opacity-0",
          )}
        >
          {candidate && <ReleaseDetail candidate={candidate} />}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function ReleaseDetail({ candidate: c }: { candidate: Candidate }) {
  const tier = tierOf(c.quality)
  const artwork = useArtwork(c.parent, c.title)
  const accent = useAccentColour(artwork.data?.thumb)
  const audio = c.files.filter((f) => f.audio)
  const other = c.files.filter((f) => !f.audio)
  const tracks = audio.map((f) => ({ file: f, ...parseTrackName(f.name) }))

  return (
    <div className="relative grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)] lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] lg:grid-rows-1">
      {/* Artwork-tinted light behind the left column. */}
      <div
        className="pointer-events-none absolute inset-y-0 left-0 w-full opacity-40 lg:w-[42%]"
        style={{
          background: `radial-gradient(90% 60% at 30% 15%, ${accent ?? "var(--primary)"} 0%, transparent 70%)`,
          opacity: accent ? 0.28 : 0.12,
        }}
        aria-hidden
      />

      <Dialog.Close
        className="absolute top-4 right-4 z-10 flex size-10 items-center justify-center rounded-full bg-background/60 text-muted-foreground backdrop-blur transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        aria-label="Close"
      >
        <X className="size-5" />
      </Dialog.Close>

      <aside className="relative overflow-y-auto border-b px-6 pt-8 pb-6 sm:px-10 sm:pt-10 lg:border-r lg:border-b-0 lg:pb-10">
        <div className="flex gap-6 lg:block">
          <Cover
            src={artwork.data?.cover}
            pending={artwork.isPending}
            alt={artwork.data ? `${artwork.data.album} cover` : ""}
            className="aspect-square w-28 rounded-xl shadow-[0_24px_60px_-24px_rgb(0_0_0/0.9)] sm:w-36 lg:w-full lg:max-w-[min(400px,38vh)] lg:rounded-2xl"
          />
          <div className="min-w-0 lg:mt-8">
            <p className={cn("flex flex-wrap items-center gap-x-2 gap-y-0.5 pr-12 text-[14px] font-semibold lg:pr-0", TIER_TEXT[tier])}>
              <span className={cn("size-2 rounded-full", TIER_BG[tier])} aria-hidden />
              {c.quality_label ?? "Unknown quality"}
              <span className="font-normal text-muted-foreground">{describeQuality(c.quality)}</span>
            </p>
            <Dialog.Title className="type-display mt-3 text-[28px] text-balance break-words sm:text-[36px] lg:text-[40px]">
              {artwork.data?.album ?? c.title}
            </Dialog.Title>
            <Dialog.Description className="mt-2 text-lg text-muted-foreground">
              {artwork.data?.artist ?? c.parent ?? "Unknown artist"}
            </Dialog.Description>
          </div>
        </div>

        <dl className="mt-8 grid grid-cols-2 gap-x-6 gap-y-5 text-sm sm:grid-cols-3 lg:grid-cols-2">
          <Stat label="Tracks" value={plural(c.audio_files, "track")} />
          <Stat label="Length" value={formatRuntime(c.duration_secs) ?? "Not reported"} />
          <Stat label="Size" value={formatBytes(c.total_bytes)} />
          <Stat
            label="Availability"
            value={c.free_slot ? "Ready to send" : `${plural(c.queue_length, "person", "people")} ahead`}
            tone={c.free_slot ? "good" : undefined}
          />
          <Stat label="Upload speed" value={formatSpeed(c.avg_speed) ?? "Not reported"} />
          <Stat label="Shared by" value={c.username} />
        </dl>

        {c.mixed_quality && (
          <div className="mt-6 flex gap-3 rounded-xl border border-q-hires/25 bg-q-hires/8 px-4 py-3 text-sm">
            <TriangleAlert className="mt-0.5 size-4 shrink-0 text-q-hires" />
            <p>Not every file here is the same quality. The label shows the lowest; check the tracklist before choosing it.</p>
          </div>
        )}

      </aside>

      <section className="relative min-h-0 overflow-y-auto px-3 pt-4 pb-8 sm:px-6 lg:pt-10">
        <h3 className="px-4 pb-3 text-sm text-muted-foreground">
          {plural(audio.length, "track")}
          {other.length > 0 && `, ${plural(other.length, "other file")}`}
        </h3>
        <ol>
          {tracks.map(({ file, number, disc, title, extension }, i) => (
            <li
              key={file.name}
              className="grid grid-cols-[32px_minmax(0,1fr)_auto] items-center gap-x-4 rounded-xl px-4 py-2.5 transition-colors hover:bg-accent/50 sm:grid-cols-[32px_minmax(0,1fr)_auto_auto_auto]"
              title={file.name}
            >
              <span className="text-right text-[14px] text-muted-foreground/70">
                {disc ? `${disc}-` : ""}
                {number ?? i + 1}
              </span>
              <span className="min-w-0">
                <span className="block truncate text-[15px]">{title}</span>
                <span className="block truncate text-[12.5px] text-muted-foreground/70 sm:hidden">
                  {file.quality_label} {formatBytes(file.size)}
                </span>
              </span>
              <span className={cn("hidden text-[13px] sm:block", TIER_TEXT[tierOf(file.quality)])}>
                {file.quality_label ?? extension.toUpperCase()}
              </span>
              <span className="hidden w-12 text-right text-[13.5px] text-muted-foreground sm:block">
                {formatTrackTime(file.duration_secs)}
              </span>
              <span className="w-16 text-right text-[13.5px] text-muted-foreground">{formatBytes(file.size)}</span>
            </li>
          ))}
        </ol>
        {other.length > 0 && (
          <ul className="mt-4 border-t pt-4">
            {other.map((f) => (
              <li key={f.name} className="grid grid-cols-[32px_minmax(0,1fr)_auto] items-center gap-x-4 px-4 py-2 text-muted-foreground">
                {/\.(jpe?g|png|webp|gif)$/i.test(f.name) ? (
                  <FileImage className="ml-auto size-4 opacity-60" />
                ) : (
                  <FileIcon className="ml-auto size-4 opacity-60" />
                )}
                <span className="truncate text-[14px]">{f.name}</span>
                <span className="w-16 text-right text-[13.5px]">{formatBytes(f.size)}</span>
              </li>
            ))}
          </ul>
        )}
        <div className="mx-4 mt-8 rounded-xl border bg-background/40 px-4 py-3.5">
          <p className="text-[13.5px] leading-relaxed text-muted-foreground">
            Downloading is delune's next milestone. When it lands, you'll send a release from here to be checked, tagged and
            held for your review.
          </p>
          <p className="mt-2 truncate text-[12.5px] text-muted-foreground/60" title={c.folder}>
            {c.folder}
          </p>
        </div>
      </section>
    </div>
  )
}

function Stat({ label, value, tone }: { label: string; value: string; tone?: "good" }) {
  return (
    <div className="min-w-0">
      <dt className="text-[12.5px] text-muted-foreground">{label}</dt>
      <dd className={cn("mt-0.5 text-[15px] break-words", tone === "good" && "text-q-lossless")}>{value}</dd>
    </div>
  )
}
