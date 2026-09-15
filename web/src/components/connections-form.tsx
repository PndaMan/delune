import { useMutation } from "@tanstack/react-query"
import { CircleAlert, CircleCheck, LoaderCircle, Lock } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import {
  type CheckResult,
  reloadAfterRestart,
  type SetupCheck,
  type SetupRequest,
  type SetupStatus,
  setupApi,
} from "@/lib/setup"
import { cn } from "@/lib/utils"

/** The music folder, Navidrome and Soulseek: check them, then save and restart. */
export function ConnectionsForm({ status }: { status: SetupStatus }) {
  const [libraryDir, setLibraryDir] = useState(status.library_dir ?? "")
  const [slsk, setSlsk] = useState({
    username: status.soulseek_username ?? "",
    password: "",
    port: status.soulseek_port,
  })
  const [nd, setNd] = useState({
    url: status.navidrome_url ?? "",
    username: status.navidrome_username ?? "",
    password: "",
  })
  const [results, setResults] = useState<SetupCheck | null>(null)
  const [restarting, setRestarting] = useState(false)

  const request: SetupRequest = {}
  if (!status.locked.library && libraryDir.trim() !== (status.library_dir ?? ""))
    request.library_dir = libraryDir.trim()
  const slskChanged =
    slsk.username.trim() !== (status.soulseek_username ?? "") ||
    slsk.password !== "" ||
    slsk.port !== status.soulseek_port
  if (!status.locked.soulseek && slskChanged) {
    request.soulseek = { username: slsk.username.trim(), password: slsk.password, port: slsk.port }
  }
  const ndChanged =
    nd.url.trim() !== (status.navidrome_url ?? "") ||
    nd.username.trim() !== (status.navidrome_username ?? "") ||
    nd.password !== ""
  if (!status.locked.navidrome && ndChanged) {
    request.navidrome = { url: nd.url.trim(), username: nd.username.trim(), password: nd.password }
  }
  const dirty = Object.keys(request).length > 0

  const check = useMutation({ mutationFn: setupApi.check, onSuccess: ({ check }) => setResults(check) })
  const save = useMutation({
    mutationFn: setupApi.save,
    onSuccess: ({ ok, check }) => {
      setResults(check)
      if (ok) {
        setRestarting(true)
        void reloadAfterRestart()
      }
    },
  })
  const busy = check.isPending || save.isPending || restarting

  return (
    <form
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault()
        if (dirty) save.mutate(request)
      }}
    >
      <Part
        title="Music folder"
        hint="The folder Navidrome scans. Approved albums are moved here."
        locked={status.locked.library && "DELUNE_LIBRARY_DIR"}
        result={results?.library}
      >
        <Field label="Folder">
          <input
            value={libraryDir}
            onChange={(e) => setLibraryDir(e.target.value)}
            placeholder="/srv/music"
            disabled={status.locked.library || busy}
            spellCheck={false}
            autoCapitalize="none"
            className={input}
          />
        </Field>
      </Part>

      <Part
        title="Navidrome"
        hint="People sign in to delune with their Navidrome accounts, and imports trigger a rescan. Use an admin account."
        locked={status.locked.navidrome && "DELUNE_NAVIDROME_*"}
        result={results?.navidrome}
      >
        <div className="grid gap-3 sm:grid-cols-3">
          <Field label="Address" wide>
            <input
              value={nd.url}
              onChange={(e) => setNd({ ...nd, url: e.target.value })}
              placeholder="http://localhost:4533"
              inputMode="url"
              disabled={status.locked.navidrome || busy}
              spellCheck={false}
              autoCapitalize="none"
              className={input}
            />
          </Field>
          <Field label="Admin username">
            <input
              value={nd.username}
              onChange={(e) => setNd({ ...nd, username: e.target.value })}
              autoComplete="off"
              disabled={status.locked.navidrome || busy}
              spellCheck={false}
              autoCapitalize="none"
              className={input}
            />
          </Field>
          <Field label="Password">
            <input
              type="password"
              value={nd.password}
              onChange={(e) => setNd({ ...nd, password: e.target.value })}
              placeholder={status.navidrome_username ? "Unchanged" : ""}
              autoComplete="new-password"
              disabled={status.locked.navidrome || busy}
              className={input}
            />
          </Field>
        </div>
      </Part>

      <Part
        title="Soulseek"
        hint="delune signs in to Soulseek itself. A new username is registered the first time it signs in."
        locked={status.locked.soulseek && "DELUNE_SLSK_*"}
        result={results?.soulseek}
      >
        <div className="grid gap-3 sm:grid-cols-3">
          <Field label="Username">
            <input
              value={slsk.username}
              onChange={(e) => setSlsk({ ...slsk, username: e.target.value })}
              autoComplete="off"
              disabled={status.locked.soulseek || busy}
              spellCheck={false}
              autoCapitalize="none"
              className={input}
            />
          </Field>
          <Field label="Password">
            <input
              type="password"
              value={slsk.password}
              onChange={(e) => setSlsk({ ...slsk, password: e.target.value })}
              placeholder={status.soulseek_username ? "Unchanged" : ""}
              autoComplete="new-password"
              disabled={status.locked.soulseek || busy}
              className={input}
            />
          </Field>
          <Field label="Listening port">
            <input
              type="number"
              min={1024}
              max={65535}
              value={slsk.port ?? ""}
              placeholder="2234"
              onChange={(e) => setSlsk({ ...slsk, port: e.target.value ? Number(e.target.value) : null })}
              disabled={status.locked.soulseek || busy}
              className={input}
            />
          </Field>
        </div>
      </Part>

      <div className="flex flex-wrap items-center gap-3 pt-1">
        <p className="min-w-0 flex-1 text-[13px] text-muted-foreground">
          {restarting
            ? "Restarting delune with the new connections"
            : save.isError
              ? save.error.message
              : check.isError
                ? check.error.message
                : `Saved to ${status.config_path}. Saving restarts delune; downloads carry on afterwards.`}
        </p>
        <Button type="button" variant="outline" disabled={!dirty || busy} onClick={() => check.mutate(request)}>
          {check.isPending && <LoaderCircle className="animate-spin" />} Check
        </Button>
        <Button type="submit" disabled={!dirty || busy || !status.can_restart}>
          {(save.isPending || restarting) && <LoaderCircle className="animate-spin" />} Save and restart
        </Button>
      </div>
    </form>
  )
}

const input =
  "h-10 w-full rounded-xl border bg-background/50 px-3 text-[15px] outline-none focus:border-primary/50 disabled:opacity-60"

function Part({
  title,
  hint,
  locked,
  result,
  children,
}: {
  title: string
  hint: string
  locked: string | false
  result: CheckResult | null | undefined
  children: React.ReactNode
}) {
  return (
    <fieldset className="rounded-2xl border bg-card/50 px-5 py-4">
      <legend className="sr-only">{title}</legend>
      <p className="flex items-center gap-2 text-[15px]">
        {title}
        {locked && (
          <span className="flex items-center gap-1 text-[12.5px] text-muted-foreground">
            <Lock className="size-3.5" /> Set by {locked}
          </span>
        )}
      </p>
      <p className="mt-0.5 mb-3 text-[13px] text-muted-foreground">{hint}</p>
      {children}
      {result && (
        <p
          className={cn(
            "mt-3 flex items-start gap-2 text-[13.5px]",
            result.ok ? "text-q-lossless" : "text-destructive",
          )}
        >
          {result.ok ? (
            <CircleCheck className="mt-0.5 size-4 shrink-0" />
          ) : (
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
          )}
          {result.message}
        </p>
      )}
    </fieldset>
  )
}

function Field({ label, wide, children }: { label: string; wide?: boolean; children: React.ReactNode }) {
  return (
    <label className={cn("block", wide && "sm:col-span-3")}>
      <span className="mb-1.5 block text-[13px] text-muted-foreground">{label}</span>
      {children}
    </label>
  )
}
