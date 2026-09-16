import { useMutation, useQuery } from "@tanstack/react-query"
import { CircleAlert, CircleCheck, FolderOpen, Link2, LoaderCircle, Lock, RefreshCw, Server, Waves } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { describeSoulseek, useSoulseekStatus } from "@/components/soulseek-indicator"
import {
  type CheckResult,
  reloadAfterRestart,
  type SetupCheck,
  type SetupRequest,
  type SetupStatus,
  setupApi,
} from "@/lib/setup"
import { cn } from "@/lib/utils"

/**
 * The three things delune connects to: the music folder, Navidrome and Soulseek.
 *
 * One card each, so a connection can be changed on its own and applied straight
 * away. Each card says whether it's working right now, checks what you type before
 * saving it, and — for connections set by a flag or environment variable — shows
 * what's in use instead of fields you can't edit.
 */
export function ConnectionsForm({ status }: { status: SetupStatus }) {
  const [applying, setApplying] = useState(false)

  return (
    <div className="space-y-3">
      <LibraryCard status={status} applying={applying} onApply={setApplying} />
      <NavidromeCard status={status} applying={applying} onApply={setApplying} />
      <SoulseekCard status={status} applying={applying} onApply={setApplying} />
      <p className="px-1 pt-1 text-[12.5px] break-words text-muted-foreground">
        {status.locked.library && status.locked.navidrome && status.locked.soulseek
          ? "All three come from the environment delune was started in, so there's nothing to change here."
          : `Saved to ${status.config_path}. Each change is applied as soon as you save it; downloads carry on.`}
      </p>
    </div>
  )
}

type CardProps = { status: SetupStatus; applying: boolean; onApply: (applying: boolean) => void }

/** Saving one connection: check first, then save, then wait for delune to come back. */
function useApply(onApply: (applying: boolean) => void) {
  const [result, setResult] = useState<CheckResult | null>(null)
  // A card only ever sends its own connection, so its own result is the one to show.
  const pick = (request: SetupRequest, check: SetupCheck) =>
    (request.library_dir !== undefined ? check.library : request.navidrome ? check.navidrome : check.soulseek) ?? null

  const check = useMutation({
    meta: { quiet: true },
    mutationFn: setupApi.check,
    onSuccess: ({ check: results }, request) => setResult(pick(request, results)),
  })
  const save = useMutation({
    meta: { quiet: true },
    mutationFn: setupApi.save,
    onSuccess: ({ ok, check: results }, request) => {
      setResult(pick(request, results))
      if (ok) {
        onApply(true)
        void reloadAfterRestart()
      }
    },
  })
  const error = save.error?.message ?? check.error?.message ?? null
  return { result, setResult, check, save, busy: check.isPending || save.isPending, error }
}

function LibraryCard({ status, applying, onApply }: CardProps) {
  const saved = status.library_dir ?? ""
  const [dir, setDir] = useState(saved)
  const { result, setResult, check, save, busy, error } = useApply(onApply)
  const changed = dir.trim() !== saved
  const request: SetupRequest = { library_dir: dir.trim() }

  return (
    <Card
      icon={FolderOpen}
      title="Music folder"
      summary={saved || "Not set — approved albums have nowhere to go"}
      state={saved ? (status.locked.library ? "set" : "good") : "missing"}
      locked={status.locked.library && "DELUNE_LIBRARY_DIR"}
      startOpen={!saved}
    >
      <p className="text-[13px] text-muted-foreground">
        The folder Navidrome scans. Approved albums are moved here, named by your template.
      </p>
      <Field label="Folder">
        <input
          value={dir}
          onChange={(e) => {
            setDir(e.target.value)
            setResult(null)
          }}
          placeholder="/srv/music"
          disabled={busy || applying}
          spellCheck={false}
          autoCapitalize="none"
          className={input}
        />
      </Field>
      <Actions
        changed={changed}
        busy={busy}
        applying={applying}
        canRestart={status.can_restart}
        result={result}
        error={error}
        onCheck={() => check.mutate(request)}
        onSave={() => save.mutate(request)}
      />
    </Card>
  )
}

function NavidromeCard({ status, applying, onApply }: CardProps) {
  const [form, setForm] = useState({
    url: status.navidrome_url ?? "",
    username: status.navidrome_username ?? "",
    password: "",
  })
  const { result, setResult, check, save, busy, error } = useApply(onApply)
  // Only settings delune saved itself can be re-checked: a password set by an
  // environment variable never reaches the config file, so a check would just fail.
  const connected = useConnectionCheck(
    "navidrome",
    status.navidrome_url && !status.locked.navidrome
      ? { navidrome: { url: status.navidrome_url, username: status.navidrome_username ?? "", password: "" } }
      : null,
  )
  const changed =
    form.url.trim() !== (status.navidrome_url ?? "") ||
    form.username.trim() !== (status.navidrome_username ?? "") ||
    form.password !== ""
  const request: SetupRequest = {
    navidrome: { url: form.url.trim(), username: form.username.trim(), password: form.password },
  }
  const edit = (part: Partial<typeof form>) => {
    setForm({ ...form, ...part })
    setResult(null)
  }

  return (
    <Card
      icon={Server}
      title="Navidrome"
      summary={
        status.navidrome_url
          ? `${hostOf(status.navidrome_url)}${status.navidrome_username ? ` · ${status.navidrome_username}` : ""}`
          : "Not connected — anyone who can reach delune can use it"
      }
      state={!status.navidrome_url ? "missing" : status.locked.navidrome ? "set" : liveState(connected)}
      live={status.navidrome_url && !status.locked.navidrome ? liveText(connected) : undefined}
      locked={status.locked.navidrome && "DELUNE_NAVIDROME_*"}
      startOpen={!status.navidrome_url}
    >
      <p className="text-[13px] text-muted-foreground">
        People sign in to delune with their Navidrome accounts, and imports trigger a rescan. Use an admin account.
      </p>
      <Field label="Address">
        <input
          value={form.url}
          onChange={(e) => edit({ url: e.target.value })}
          placeholder="http://localhost:4533"
          inputMode="url"
          disabled={busy || applying}
          spellCheck={false}
          autoCapitalize="none"
          className={input}
        />
      </Field>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Admin username">
          <input
            value={form.username}
            onChange={(e) => edit({ username: e.target.value })}
            autoComplete="off"
            disabled={busy || applying}
            spellCheck={false}
            autoCapitalize="none"
            className={input}
          />
        </Field>
        <Field label="Password">
          <input
            type="password"
            value={form.password}
            onChange={(e) => edit({ password: e.target.value })}
            placeholder={status.navidrome_username ? "Unchanged" : ""}
            autoComplete="new-password"
            disabled={busy || applying}
            className={input}
          />
        </Field>
      </div>
      <Actions
        changed={changed}
        busy={busy}
        applying={applying}
        canRestart={status.can_restart}
        result={result}
        error={error}
        onCheck={() => check.mutate(request)}
        onSave={() => save.mutate(request)}
      />
    </Card>
  )
}

function SoulseekCard({ status, applying, onApply }: CardProps) {
  const [form, setForm] = useState({
    username: status.soulseek_username ?? "",
    password: "",
    port: status.soulseek_port,
  })
  const { result, setResult, check, save, busy, error } = useApply(onApply)
  const soulseek = useSoulseekStatus()
  const live = describeSoulseek(soulseek.data, soulseek.isError)
  const changed =
    form.username.trim() !== (status.soulseek_username ?? "") ||
    form.password !== "" ||
    form.port !== status.soulseek_port
  const request: SetupRequest = {
    soulseek: { username: form.username.trim(), password: form.password, port: form.port },
  }
  const edit = (part: Partial<typeof form>) => {
    setForm({ ...form, ...part })
    setResult(null)
  }

  return (
    <Card
      icon={Waves}
      title="Soulseek"
      summary={
        status.soulseek_username
          ? `${status.soulseek_username}${status.soulseek_port ? ` · port ${status.soulseek_port}` : ""}`
          : "Not signed in — searches won't return anything"
      }
      state={
        status.soulseek_username
          ? live.tone === "good"
            ? "good"
            : live.tone === "bad"
              ? "bad"
              : "checking"
          : "missing"
      }
      live={
        status.soulseek_username
          ? `${live.text}${status.soulseek_port ? ` · port ${status.soulseek_port}` : ""}`
          : undefined
      }
      locked={status.locked.soulseek && "DELUNE_SLSK_*"}
      startOpen={!status.soulseek_username}
    >
      <p className="text-[13px] text-muted-foreground">
        delune signs in to Soulseek itself. A new username is registered the first time it signs in.
      </p>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Username">
          <input
            value={form.username}
            onChange={(e) => edit({ username: e.target.value })}
            autoComplete="off"
            disabled={busy || applying}
            spellCheck={false}
            autoCapitalize="none"
            className={input}
          />
        </Field>
        <Field label="Password">
          <input
            type="password"
            value={form.password}
            onChange={(e) => edit({ password: e.target.value })}
            placeholder={status.soulseek_username ? "Unchanged" : ""}
            autoComplete="new-password"
            disabled={busy || applying}
            className={input}
          />
        </Field>
      </div>
      <Field label="Listening port" hint="Other users connect in on this port, which brings more results.">
        <input
          type="number"
          min={1024}
          max={65535}
          value={form.port ?? ""}
          placeholder="2234"
          onChange={(e) => edit({ port: e.target.value ? Number(e.target.value) : null })}
          disabled={busy || applying}
          className={cn(input, "sm:max-w-40")}
        />
      </Field>
      <Actions
        changed={changed}
        busy={busy}
        applying={applying}
        canRestart={status.can_restart}
        result={result}
        error={error}
        onCheck={() => check.mutate(request)}
        onSave={() => save.mutate(request)}
      />
    </Card>
  )
}

/** Ask the server whether a saved connection is working, so the card can say so. */
function useConnectionCheck(key: string, request: SetupRequest | null) {
  return useQuery({
    queryKey: ["setup", "check", key],
    enabled: request !== null,
    staleTime: 30_000,
    retry: false,
    queryFn: async () => {
      const { check } = await setupApi.check(request as SetupRequest)
      return check.navidrome ?? check.soulseek ?? check.library ?? null
    },
  })
}

type Live = ReturnType<typeof useConnectionCheck>

function liveState(live: Live): State {
  if (live.isPending) return "checking"
  return live.data?.ok ? "good" : "bad"
}

function liveText(live: Live) {
  if (live.isPending) return "Checking"
  return live.data?.message ?? (live.isError ? "Couldn't check just now" : undefined)
}

/** `set` is configured but not something delune can verify from here. */
type State = "good" | "bad" | "set" | "missing" | "checking"

const DOT: Record<State, string> = {
  good: "bg-q-lossless",
  bad: "bg-destructive",
  set: "bg-foreground/40",
  missing: "bg-muted-foreground/25",
  checking: "bg-muted-foreground/40 animate-pulse",
}

/**
 * One connection: what it is, whether it's working, and — opened — how to change it.
 * Cards stay shut once a connection is set, so the page reads as a status list first.
 */
function Card({
  icon: Icon,
  title,
  summary,
  state,
  live,
  locked,
  startOpen,
  children,
}: {
  icon: typeof Server
  title: string
  summary: string
  state: State
  live?: string
  locked: string | false
  startOpen: boolean
  children: React.ReactNode
}) {
  const [open, setOpen] = useState(startOpen)
  return (
    <section className="overflow-hidden rounded-2xl border bg-card/50">
      <button
        type="button"
        onClick={() => !locked && setOpen(!open)}
        aria-expanded={locked ? undefined : open}
        disabled={!!locked}
        className={cn(
          "flex w-full items-center gap-3.5 px-4 py-3.5 text-left outline-none sm:px-5",
          !locked && "hover:bg-accent/40 focus-visible:bg-accent/40",
        )}
      >
        <span className="relative shrink-0">
          <Icon className="size-5 text-muted-foreground" strokeWidth={1.8} />
          <span
            className={cn("absolute -right-1 -bottom-1 size-2.5 rounded-full ring-2 ring-card", DOT[state])}
            aria-hidden
          />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-[15px]">{title}</span>
          <span className="block truncate text-[13px] text-muted-foreground">
            {locked ? `Set by ${locked}` : (live ?? summary)}
          </span>
        </span>
        {locked ? (
          <span className="flex shrink-0 items-center gap-1 text-[12px] text-muted-foreground">
            <Lock className="size-3.5" /> Fixed
          </span>
        ) : (
          <span className="shrink-0 text-[13px] text-muted-foreground">
            {open ? "Close" : state === "missing" ? "Set up" : "Change"}
          </span>
        )}
      </button>
      {locked ? (
        <div className="border-t px-4 py-3 sm:px-5">
          <p className="text-[13px] break-words text-foreground/80">{summary}</p>
          <p className="mt-1 text-[12.5px] text-muted-foreground">Change it where delune is started, then restart.</p>
        </div>
      ) : (
        open && <div className="space-y-3 border-t px-4 py-4 sm:px-5">{children}</div>
      )}
    </section>
  )
}

function Actions({
  changed,
  busy,
  applying,
  canRestart,
  result,
  error,
  onCheck,
  onSave,
}: {
  changed: boolean
  busy: boolean
  applying: boolean
  canRestart: boolean
  result: CheckResult | null
  error: string | null
  onCheck: () => void
  onSave: () => void
}) {
  return (
    <div className="space-y-3 pt-1">
      {applying ? (
        <p className="flex items-center gap-2 text-[13.5px] text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" /> Applying, and reconnecting when delune is back
        </p>
      ) : error ? (
        <p className="flex items-start gap-2 text-[13.5px] text-destructive">
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          {error}
        </p>
      ) : (
        result && (
          <p className={cn("flex items-start gap-2 text-[13.5px]", result.ok ? "text-q-lossless" : "text-destructive")}>
            {result.ok ? (
              <CircleCheck className="mt-0.5 size-4 shrink-0" />
            ) : (
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
            )}
            {result.message}
          </p>
        )
      )}
      <div className="flex gap-2">
        <Button
          type="button"
          variant="outline"
          className="flex-1 sm:flex-none"
          disabled={!changed || busy || applying}
          onClick={onCheck}
        >
          {busy && !applying ? <LoaderCircle className="animate-spin" /> : <Link2 />} Test
        </Button>
        <Button
          type="button"
          className="flex-1 sm:flex-none"
          disabled={!changed || busy || applying || !canRestart}
          onClick={onSave}
        >
          {applying ? <LoaderCircle className="animate-spin" /> : <RefreshCw />} Save and apply
        </Button>
      </div>
    </div>
  )
}

const input =
  "h-11 w-full rounded-xl border bg-background/50 px-3 text-[15px] outline-none focus:border-primary/50 disabled:opacity-60"

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-[13px] text-muted-foreground">{label}</span>
      {children}
      {hint && <span className="mt-1 block text-[12.5px] text-muted-foreground/80">{hint}</span>}
    </label>
  )
}

/** "http://localhost:4533" reads better as "localhost:4533". */
function hostOf(url: string) {
  try {
    return new URL(url).host || url
  } catch {
    return url
  }
}
