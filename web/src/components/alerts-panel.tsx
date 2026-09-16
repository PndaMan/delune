import { useQueryClient } from "@tanstack/react-query"
import { BellRing, Check, LoaderCircle, Smartphone, Trash2, X } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import {
  type AlertSettings,
  isStandalone,
  KIND_LABELS,
  pushSupported,
  subscribeThisDevice,
  thisDeviceId,
  unsubscribeDevice,
  useAlertSettings,
  useSaveAlertSettings,
  useTestAlerts,
} from "@/lib/alerts"
import { formatAgo } from "@/lib/format"
import { useMe } from "@/lib/session"
import { cn } from "@/lib/utils"

/** Where your notifications go besides delune: this phone or browser, ntfy, Discord. */
export function AlertsPanel() {
  const settings = useAlertSettings()
  if (settings.isPending) {
    return (
      <p className="flex items-center gap-2 px-5 py-8 text-muted-foreground">
        <LoaderCircle className="size-4 animate-spin" /> Loading
      </p>
    )
  }
  if (!settings.data) return <p className="px-5 py-6 text-destructive">{settings.error?.message}</p>
  // Keyed so the form starts from what's saved.
  return <Form key={`${settings.data.ntfy}|${settings.data.discord}`} settings={settings.data} />
}

function Form({ settings }: { settings: AlertSettings }) {
  const client = useQueryClient()
  const me = useMe()
  const save = useSaveAlertSettings()
  const test = useTestAlerts()
  const [ntfy, setNtfy] = useState(settings.ntfy ?? "")
  const [discord, setDiscord] = useState(settings.discord ?? "")
  const [busy, setBusy] = useState(false)
  const [pushError, setPushError] = useState<string | null>(null)
  const here = thisDeviceId()
  const onThisDevice = !!here && settings.devices.some((d) => d.id === here)
  const setSettings = (next: AlertSettings) => client.setQueryData(["notifications", "settings"], next)

  const persist = (change: Partial<{ ntfy: string; discord: string; muted: typeof settings.muted }>) =>
    save.mutate({
      ntfy: (change.ntfy ?? ntfy) || null,
      discord: (change.discord ?? discord) || null,
      muted: change.muted ?? settings.muted,
    })

  const togglePush = async (on: boolean) => {
    setBusy(true)
    setPushError(null)
    try {
      if (on) {
        if (!settings.push_key) throw new Error("This server can't send push notifications.")
        setSettings(await subscribeThisDevice(settings.push_key))
      } else if (here) {
        setSettings(await unsubscribeDevice(here))
      }
    } catch (e) {
      setPushError(e instanceof Error ? e.message : "Couldn't change notifications on this device.")
    } finally {
      setBusy(false)
    }
  }

  const iphone = /iPhone|iPad/.test(navigator.userAgent)
  const dirty = ntfy !== (settings.ntfy ?? "") || discord !== (settings.discord ?? "")

  return (
    <div className="divide-y">
      <section className="px-5 py-5">
        <div className="flex items-center gap-4">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/15 text-primary">
            <Smartphone className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-[15px] font-medium">Notifications on this device</p>
            <p className="text-[13.5px] text-muted-foreground">
              {pushSupported()
                ? "Hear about finished downloads even when delune isn't open."
                : iphone && !isStandalone()
                  ? "On iPhone, add delune to your home screen (Share → Add to Home Screen), then open it from there."
                  : "This browser can't show push notifications."}
            </p>
          </div>
          {busy ? (
            <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
          ) : (
            <Switch
              checked={onThisDevice}
              disabled={!pushSupported()}
              onCheckedChange={(on) => void togglePush(on)}
              aria-label="Notifications on this device"
            />
          )}
        </div>
        {pushError && <p className="mt-2 text-[13.5px] text-destructive">{pushError}</p>}
        {settings.devices.length > 0 && (
          <ul className="mt-4 space-y-1">
            {settings.devices.map((device) => (
              <li key={device.id} className="flex items-center gap-3 rounded-lg px-2 py-1.5 text-[14px]">
                <BellRing className="size-4 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate">
                  {device.label}
                  {device.id === here && <span className="text-muted-foreground"> · this one</span>}
                </span>
                <span className="text-[12.5px] text-muted-foreground">added {formatAgo(device.added_at)}</span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Stop notifications on ${device.label}`}
                  className="text-muted-foreground"
                  onClick={async () => setSettings(await unsubscribeDevice(device.id))}
                >
                  <Trash2 />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="space-y-4 px-5 py-5">
        <div>
          <p className="text-[15px] font-medium">Elsewhere</p>
          <p className="text-[13.5px] text-muted-foreground">
            Send them to an ntfy topic (the ntfy app on any phone), or a Discord channel's webhook.
          </p>
        </div>
        <label className="block">
          <span className="text-[13px] text-muted-foreground">ntfy topic</span>
          <input
            value={ntfy}
            onChange={(e) => setNtfy(e.target.value)}
            placeholder={me.permissions.manage ? "https://ntfy.sh/your-topic or http://ntfy.home/…" : "https://ntfy.sh/your-topic"}
            className="mt-1 h-10 w-full rounded-lg border bg-background/50 px-3 text-[14px] outline-none focus-visible:border-ring"
            inputMode="url"
            autoComplete="off"
          />
        </label>
        <label className="block">
          <span className="text-[13px] text-muted-foreground">Discord webhook</span>
          <input
            value={discord}
            onChange={(e) => setDiscord(e.target.value)}
            placeholder="https://discord.com/api/webhooks/…"
            className="mt-1 h-10 w-full rounded-lg border bg-background/50 px-3 text-[14px] outline-none focus-visible:border-ring"
            inputMode="url"
            autoComplete="off"
          />
        </label>
        {save.isError && <p className="text-[13.5px] text-destructive">{save.error.message}</p>}
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => persist({})} disabled={!dirty || save.isPending}>
            {save.isPending ? <LoaderCircle className="animate-spin" /> : null} Save
          </Button>
          <Button variant="outline" onClick={() => test.mutate()} disabled={test.isPending || dirty}>
            {test.isPending ? <LoaderCircle className="animate-spin" /> : null} Send a test
          </Button>
        </div>
        {test.data && (
          <ul className="space-y-1 text-[13.5px]">
            {test.data.length === 0 && (
              <li className="text-muted-foreground">Nothing to send to yet: turn one of these on first.</li>
            )}
            {test.data.map((r) => (
              <li key={r.channel} className={cn("flex items-center gap-2", r.ok ? "text-q-lossless" : "text-destructive")}>
                {r.ok ? <Check className="size-4" /> : <X className="size-4" />}
                <span className="text-foreground">{r.channel}</span>
                <span>· {r.message}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="px-5 py-5">
        <p className="text-[15px] font-medium">What to send</p>
        <p className="text-[13.5px] text-muted-foreground">Everything still shows in delune's bell.</p>
        <ul className="mt-3 space-y-2">
          {KIND_LABELS.filter(([kind]) => kind !== "request-new" || me.permissions.manage).map(([kind, label]) => {
            const on = !settings.muted.includes(kind)
            return (
              <li key={kind} className="flex items-center justify-between gap-3">
                <span className="text-[14.5px]">{label}</span>
                <Switch
                  checked={on}
                  onCheckedChange={(next) =>
                    persist({ muted: next ? settings.muted.filter((k) => k !== kind) : [...settings.muted, kind] })
                  }
                  aria-label={label}
                />
              </li>
            )
          })}
        </ul>
      </section>
    </div>
  )
}
