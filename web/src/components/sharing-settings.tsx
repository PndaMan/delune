import { useMutation, useQueryClient } from "@tanstack/react-query"
import { LoaderCircle, RefreshCw } from "lucide-react"
import { useEffect, useState } from "react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { plural } from "@/lib/format"
import { type SharingSettings, sharingApi, useSharingStatus } from "@/lib/sharing"
import { cn } from "@/lib/utils"

/** Turn sharing on, choose how generous to be, and see what's shared. */
export function SharingSettingsPanel() {
  const client = useQueryClient()
  const status = useSharingStatus()
  const [draft, setDraft] = useState<SharingSettings | null>(null)
  const save = useMutation({
    mutationFn: sharingApi.update,
    onSuccess: (next) => {
      client.setQueryData(["sharing"], next)
      setDraft(null)
    },
  })
  const rescan = useMutation({
    mutationFn: sharingApi.rescan,
    onSuccess: () => void client.invalidateQueries({ queryKey: ["sharing"] }),
  })
  useEffect(() => {
    if (save.isSuccess) save.reset()
  }, [status.data, save])

  if (!status.data) return <div className="h-40 animate-pulse rounded-xl bg-muted/50" />
  const s = status.data
  const settings = draft ?? s.settings
  const edit = (patch: Partial<SharingSettings>) => setDraft({ ...settings, ...patch })
  const dirty = draft !== null

  return (
    <div className="space-y-4">
      <label className={cn("flex items-start gap-4 rounded-2xl border bg-card/50 px-5 py-4", s.library_dir && "cursor-pointer")}>
        <div className="min-w-0 flex-1">
          <p className="text-[15px]">Share my library on Soulseek</p>
          <p className="mt-1 text-sm text-muted-foreground">
            {s.library_dir
              ? `Other Soulseek users can browse and download what's in ${s.library_dir}. They see it as “${settings.share_name}”, never the real path. Many people only share with those who share back.`
              : "Set a library folder when starting the server to share it."}
          </p>
        </div>
        <Switch
          checked={settings.enabled}
          disabled={!s.library_dir || save.isPending}
          onCheckedChange={(enabled) => save.mutate({ ...settings, enabled })}
          className="mt-1"
        />
      </label>

      {s.settings.enabled && (
        <div className="flex flex-wrap items-center gap-3 rounded-2xl border bg-card/50 px-5 py-4 text-[14.5px]">
          {s.scanning ? (
            <span className="flex items-center gap-2 text-muted-foreground">
              <LoaderCircle className="size-4 animate-spin" /> Indexing your library
            </span>
          ) : s.error ? (
            <span className="text-destructive">{s.error}</span>
          ) : (
            <span>
              Sharing {plural(s.files, "file")} in {plural(s.folders, "folder")}
              {s.last_scan && <span className="text-muted-foreground">, indexed {new Date(s.last_scan * 1000).toLocaleString()}</span>}
            </span>
          )}
          <Button variant="outline" size="sm" className="ml-auto" disabled={s.scanning || rescan.isPending} onClick={() => rescan.mutate()}>
            <RefreshCw /> Rescan
          </Button>
        </div>
      )}

      <div className="grid gap-4 rounded-2xl border bg-card/50 px-5 py-5 sm:grid-cols-2">
        <Field label="Shared folder name" hint="The top folder people see when they browse you.">
          <input value={settings.share_name} onChange={(e) => edit({ share_name: e.target.value })} className={input} />
        </Field>
        <Field label="Upload slots" hint="How many people download from you at once.">
          <input type="number" min={1} max={20} value={settings.slots} onChange={(e) => edit({ slots: Number(e.target.value) })} className={input} />
        </Field>
        <Field label="Speed limit (KiB/s)" hint="Leave empty for no limit.">
          <input
            type="number"
            min={0}
            value={settings.speed_limit_kib ?? ""}
            placeholder="No limit"
            onChange={(e) => edit({ speed_limit_kib: e.target.value ? Number(e.target.value) : null })}
            className={input}
          />
        </Field>
        <Field label="Files per person" hint="How many files one person can have waiting.">
          <input
            type="number"
            min={1}
            max={10000}
            value={settings.queue_per_user}
            onChange={(e) => edit({ queue_per_user: Number(e.target.value) })}
            className={input}
          />
        </Field>
        <Field label="Blocked people" hint="One Soulseek username per line. They can't download from you." wide>
          <textarea
            rows={3}
            value={settings.banned.join("\n")}
            onChange={(e) => edit({ banned: e.target.value.split("\n") })}
            className={cn(input, "h-auto py-2")}
          />
        </Field>
        {(dirty || save.isError) && (
          <div className="flex items-center gap-3 sm:col-span-2">
            {save.isError && <p className="text-sm text-destructive">{save.error.message}</p>}
            <Button variant="ghost" className="ml-auto" onClick={() => setDraft(null)} disabled={!dirty}>
              Discard
            </Button>
            <Button onClick={() => draft && save.mutate(draft)} disabled={!dirty || save.isPending}>
              {save.isPending && <LoaderCircle className="animate-spin" />} Save
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}

const input = "h-10 w-full rounded-xl border bg-background/50 px-3 text-[15px] outline-none focus:border-primary/50"

function Field({ label, hint, wide, children }: { label: string; hint: string; wide?: boolean; children: React.ReactNode }) {
  return (
    <label className={cn("block", wide && "sm:col-span-2")}>
      <span className="text-[14px]">{label}</span>
      <span className="mt-0.5 mb-2 block text-[12.5px] text-muted-foreground">{hint}</span>
      {children}
    </label>
  )
}
