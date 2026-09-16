import { LoaderCircle } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { type ExternalSource, useExternalSource, useSaveExternalSource } from "@/lib/external"

const SUGGESTED: ExternalSource = {
  enabled: true,
  program: "yt-dlp",
  arguments: [
    "--extract-audio",
    "--audio-format",
    "flac",
    "--embed-metadata",
    "-o",
    "{output}/%(title)s.%(ext)s",
    "{url}",
  ],
}

/** Point delune at a downloader you trust, for links it can't fetch itself. */
export function ExternalSourcePanel() {
  const saved = useExternalSource(true)
  const save = useSaveExternalSource()
  const [draft, setDraft] = useState<ExternalSource | null>(null)
  const current = draft ?? saved.data ?? { enabled: false, program: "", arguments: [] }
  const dirty = draft !== null
  const edit = (patch: Partial<ExternalSource>) => setDraft({ ...current, ...patch })

  if (!saved.data && saved.isPending) return <div className="h-40 animate-pulse rounded-2xl bg-muted/40" />

  return (
    <div className="space-y-4">
      <label className="flex cursor-pointer items-start gap-4 rounded-2xl border bg-card/50 px-5 py-4">
        <div className="min-w-0 flex-1">
          <p className="text-[15px]">Fetch links with a program of my own</p>
          <p className="mt-1 text-sm text-muted-foreground">
            delune doesn't get around any service's copy protection or terms, so it has no built-in downloader for
            streaming services. If you have one you trust, delune can run it for a link and review whatever it fetches.
            It runs as the delune server, so only turn this on for a program you'd run yourself.
          </p>
        </div>
        <Switch checked={current.enabled} onCheckedChange={(enabled) => edit({ enabled })} className="mt-1" />
      </label>

      {current.enabled && (
        <div className="grid gap-4 rounded-2xl border bg-card/50 px-5 py-5">
          <label className="block">
            <span className="text-[14px]">Program</span>
            <span className="mt-0.5 mb-2 block text-[12.5px] text-muted-foreground">
              A command on the server's PATH, or a full path to it.
            </span>
            <input
              value={current.program}
              onChange={(e) => edit({ program: e.target.value })}
              placeholder="yt-dlp"
              spellCheck={false}
              className="h-10 w-full rounded-xl border bg-background/50 px-3 text-[15px] outline-none focus:border-primary/50"
            />
          </label>
          <label className="block">
            <span className="text-[14px]">Arguments</span>
            <span className="mt-0.5 mb-2 block text-[12.5px] text-muted-foreground">
              One per line. {"{url}"} is the link and {"{output}"} the folder to write to; nothing else is substituted,
              and no shell is involved.
            </span>
            <textarea
              rows={6}
              value={current.arguments.join("\n")}
              onChange={(e) => edit({ arguments: e.target.value.split("\n") })}
              spellCheck={false}
              className="w-full rounded-xl border bg-background/50 px-3 py-2 font-mono text-[13.5px] outline-none focus:border-primary/50"
            />
          </label>
          <Button
            variant="ghost"
            size="sm"
            className="justify-self-start"
            onClick={() => setDraft({ ...SUGGESTED, enabled: current.enabled })}
          >
            Use yt-dlp settings
          </Button>
        </div>
      )}

      {(dirty || save.isError) && (
        <div className="flex flex-wrap items-center gap-3">
          <p className="min-w-0 flex-1 text-sm text-destructive">{save.isError ? save.error.message : ""}</p>
          <Button variant="ghost" onClick={() => setDraft(null)} disabled={save.isPending}>
            Discard
          </Button>
          <Button onClick={() => save.mutate(current, { onSuccess: () => setDraft(null) })} disabled={save.isPending}>
            {save.isPending && <LoaderCircle className="animate-spin" />} Save
          </Button>
        </div>
      )}
    </div>
  )
}
