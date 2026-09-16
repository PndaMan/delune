import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"
import {
  ArrowDownToLine,
  Camera,
  ChevronLeft,
  ChevronRight,
  Globe,
  Library,
  LoaderCircle,
  Lock,
  LogOut,
  Palette,
  Plug,
  Share2,
  Sparkles,
  UserRound,
  Users,
} from "lucide-react"
import { useEffect, useRef, useState } from "react"

import { Choice } from "@/components/choice"
import { BandcampGlyph, BandcampGroup } from "@/components/bandcamp"
import { ConnectionsForm } from "@/components/connections-form"
import { ExternalSourcePanel } from "@/components/external-source"
import { NamingEditor } from "@/components/naming-editor"
import { SharingSettingsPanel, TransfersPanel } from "@/components/sharing-settings"
import { describeSoulseek, useSoulseekStatus } from "@/components/soulseek-indicator"
import { Moon } from "@/components/moon"
import { Avatar } from "@/components/profile-menu"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { ACCENTS, avatarUrl, THEMES, useSetAppearance, useSetAvatar } from "@/lib/appearance"
import { type AutomationSettings, useAutomation } from "@/lib/automation"
import { plural } from "@/lib/format"
import { useMe, useSignOut } from "@/lib/session"
import { useSetupStatus } from "@/lib/setup"
import { PageFrame } from "@/pages/placeholder-pages"
import {
  api,
  toApiError,
  type People,
  type Permissions,
  type Person,
  type SessionInfo,
  type SoulseekStatus,
} from "@/lib/api"
import { cn } from "@/lib/utils"

const UPGRADE_OPTIONS: { value: AutomationSettings["upgrade_to"]; label: string }[] = [
  { value: "lossless", label: "Lossless" },
  { value: "hi-res", label: "Hi-res" },
]

const GROUPS = [
  {
    id: "account",
    title: "Your account",
    blurb: "Who you're signed in as, and your devices.",
    icon: UserRound,
    manage: false,
    panel: () => <Account />,
  },
  {
    id: "appearance",
    title: "Appearance",
    blurb: "Theme and accent, saved to your account.",
    icon: Palette,
    manage: false,
    panel: () => <AppearancePicker />,
  },
  {
    id: "bandcamp",
    title: "Bandcamp",
    blurb: "Link your account to see and fetch what you've bought.",
    icon: BandcampGlyph,
    manage: false,
    panel: () => <BandcampGroup />,
  },
  {
    id: "library",
    title: "Library and imports",
    blurb: "How imported music is named, and what's added to it.",
    icon: Library,
    // Naming and import options shape everyone's library; they're for whoever runs delune.
    manage: true,
    panel: () => <LibraryGroup />,
  },
  {
    id: "transfers",
    title: "Downloads",
    blurb: "How much delune downloads at once, and how fast.",
    icon: ArrowDownToLine,
    manage: true,
    panel: () => <TransfersPanel />,
  },
  {
    id: "automation",
    title: "Automation",
    blurb: "What delune does on its own. Everything still waits for review.",
    icon: Sparkles,
    manage: true,
    panel: () => <AutomationSettingsPanel />,
  },
  {
    id: "people",
    title: "People",
    blurb: "Who can sign in, and what they may do.",
    icon: Users,
    manage: true,
    panel: () => <PeopleSettings />,
  },
  {
    id: "sharing",
    title: "Sharing",
    blurb: "Share your library back with Soulseek. Off until you turn it on.",
    icon: Share2,
    manage: true,
    panel: () => <SharingSettingsPanel />,
  },
  {
    id: "connections",
    title: "Connections",
    blurb: "Music folder, Navidrome and the Soulseek account.",
    icon: Plug,
    manage: true,
    panel: () => <ConnectionsGroup />,
  },
  {
    id: "sources",
    title: "Other sources",
    blurb: "Where delune looks, and a downloader of your own.",
    icon: Globe,
    manage: true,
    panel: () => <OtherSourcesGroup />,
  },
] as const

function visibleGroups(manage: boolean) {
  return GROUPS.filter((group) => manage || !group.manage)
}

/**
 * Settings, grouped: a list to pick from on phones, a sidebar beside the group on
 * wider screens. The groups are ordered by how often people need them.
 */
export function SettingsPage({ section }: { section?: string }) {
  const me = useMe()
  const navigate = useNavigate()
  const groups = visibleGroups(me.permissions.manage)
  const active = groups.find((group) => group.id === section)
  // Older links named the group in the hash (/settings#sharing); send them to its page.
  const legacy = !section && groups.find((group) => `#${group.id}` === window.location.hash)
  useEffect(() => {
    if (legacy) void navigate({ to: "/settings/$section", params: { section: legacy.id }, hash: "", replace: true })
  }, [legacy, navigate])

  // Phones: the list is its own screen, so a group can be opened and closed.
  if (!active) {
    return (
      <PageFrame title="Settings" wide>
        <div className="mt-2 pb-12 md:hidden">
          <ul className="overflow-hidden rounded-2xl border bg-card/50">
            {groups.map((group) => (
              <li key={group.id}>
                <Link
                  to="/settings/$section"
                  params={{ section: group.id }}
                  className="flex items-center gap-4 border-b px-4 py-3.5 outline-none last:border-b-0 hover:bg-accent/60 focus-visible:bg-accent/60"
                >
                  <group.icon className="size-5 shrink-0 text-muted-foreground" strokeWidth={1.8} />
                  <span className="min-w-0 flex-1">
                    <span className="block text-[15.5px]">{group.title}</span>
                    <span className="mt-0.5 block text-[13px] text-muted-foreground">{group.blurb}</span>
                  </span>
                  <ChevronRight className="size-4 shrink-0 text-muted-foreground/70" />
                </Link>
              </li>
            ))}
          </ul>
        </div>
        <div className="hidden md:block">
          <SettingsPane groups={groups} active={groups[0]} heading />
        </div>
      </PageFrame>
    )
  }

  return (
    <PageFrame title={active.title} wide>
      <button
        type="button"
        onClick={() => void navigate({ to: "/settings" })}
        className="-mt-1 mb-2 flex items-center gap-1 text-[14px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring md:hidden"
      >
        <ChevronLeft className="size-4" /> All settings
      </button>
      <p className="max-w-[60ch] text-[15px] text-muted-foreground md:hidden">{active.blurb}</p>
      <div className="mt-4 pb-12 md:hidden">
        <active.panel />
      </div>
      <div className="hidden md:block">
        <SettingsPane groups={groups} active={active} />
      </div>
    </PageFrame>
  )
}

/** Wide screens: the groups down the side, the chosen one beside them. */
function SettingsPane({
  groups,
  active,
  heading,
}: {
  groups: readonly (typeof GROUPS)[number][]
  active: (typeof GROUPS)[number]
  /** The page title already names the group when one is open. */
  heading?: boolean
}) {
  return (
    <div className="mt-6 grid gap-10 pb-24 lg:grid-cols-[220px_minmax(0,1fr)]">
      <nav aria-label="Settings" className="lg:sticky lg:top-6 lg:self-start">
        <ul className="flex gap-1 overflow-x-auto [scrollbar-width:none] lg:block lg:space-y-0.5 lg:overflow-visible">
          {groups.map((group) => (
            <li key={group.id}>
              <Link
                to="/settings/$section"
                params={{ section: group.id }}
                aria-current={group.id === active.id ? "page" : undefined}
                className={cn(
                  "flex items-center gap-3 rounded-xl px-3 py-2 text-[14.5px] whitespace-nowrap text-muted-foreground outline-none transition-colors hover:bg-accent/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
                  group.id === active.id && "bg-accent text-foreground",
                )}
              >
                <group.icon className="size-[18px] shrink-0" strokeWidth={1.8} />
                {group.title}
              </Link>
            </li>
          ))}
        </ul>
      </nav>
      <div className="min-w-0">
        {heading && <h2 className="type-title text-[21px]">{active.title}</h2>}
        <p className={cn("mb-6 max-w-[62ch] text-[15px] text-muted-foreground", heading && "mt-1")}>{active.blurb}</p>
        <active.panel />
      </div>
    </div>
  )
}

/** Naming, then what's added to each import. */
function LibraryGroup() {
  const me = useMe()
  return (
    <div className="space-y-10">
      <Part title="File naming" hint="How folders and files are named when a release is imported.">
        <NamingEditor editable={me.permissions.manage} />
      </Part>
      <Part
        title="Lyrics and artwork"
        hint="Added to every import. Lyrics come from LRCLIB, with timings where it has them."
      >
        <ImportOptionsPanel editable={me.permissions.manage} />
      </Part>
    </div>
  )
}

/** The connections themselves, then how well other Soulseek users can reach this delune. */
function ConnectionsGroup() {
  return (
    <div className="space-y-8">
      <Connections />
      <Part title="Being found" hint="Other Soulseek users answering delune's searches, and reaching it to download.">
        <SoulseekAccount />
      </Part>
    </div>
  )
}

function OtherSourcesGroup() {
  return (
    <div className="space-y-10">
      <Part title="Where delune looks" hint="In order. Soulseek always comes first.">
        <Sources />
      </Part>
      <Part title="Fetch with your own downloader" hint="For links delune can't fetch itself. Off by default.">
        <ExternalSourcePanel />
      </Part>
    </div>
  )
}

function Part({ title, hint, children }: { title: string; hint: string; children: React.ReactNode }) {
  return (
    <section>
      <h3 className="text-[16px] font-semibold">{title}</h3>
      <p className="mt-0.5 mb-4 max-w-[62ch] text-[14px] text-muted-foreground">{hint}</p>
      {children}
    </section>
  )
}

function Sources() {
  const sources = useQuery({ queryKey: ["sources"], queryFn: ({ signal }) => api.sources(signal) })
  if (sources.isError) return <LoadFailed error={sources.error} retry={() => void sources.refetch()} />
  if (!sources.data) return <div className="h-40 animate-pulse rounded-xl bg-muted/50" />
  const ordered = [...sources.data].sort((a, b) => (a.order ?? 99) - (b.order ?? 99))

  return (
    <ol className="overflow-hidden rounded-2xl border bg-card/50">
      {ordered.map((s) => (
        <li key={s.provider} className="flex items-center gap-4 border-b px-5 py-3.5 last:border-b-0">
          <span className={cn("w-5 text-sm", s.enabled ? "text-foreground" : "text-muted-foreground/40")}>
            {s.order ?? ""}
          </span>
          <span className={cn("flex-1 text-[15px]", !s.enabled && "text-muted-foreground")}>{s.name}</span>
          {s.role === "peer-to-peer" ? (
            <span className="flex items-center gap-1.5 text-sm text-q-lossless">
              <Lock className="size-3.5" /> Always searched first
            </span>
          ) : (
            <span className="text-sm text-muted-foreground/70">Off, not available yet</span>
          )}
        </li>
      ))}
    </ol>
  )
}

function Connections() {
  const setup = useSetupStatus(true)
  if (setup.isError) return <p className="text-sm text-destructive">{setup.error.message}</p>
  if (!setup.data) return <div className="h-64 animate-pulse rounded-2xl bg-muted/40" />
  return <ConnectionsForm status={setup.data} />
}

function SoulseekAccount() {
  const status = useSoulseekStatus()
  const { illumination, text, tone } = describeSoulseek(status.data, status.isError)
  return (
    <div className="flex items-center gap-5 rounded-2xl border bg-card/50 px-5 py-5">
      <Moon illumination={illumination} size={44} glow={tone === "good"} />
      <div>
        <p className={cn("text-[15px]", tone === "bad" && "text-destructive")}>{text}</p>
        <p className="mt-1 text-sm text-muted-foreground">
          {tone === "good"
            ? "Searches are rate-limited to keep the account in good standing."
            : "Add a Soulseek account under Connections. A new username is registered the first time it signs in."}
        </p>
        {status.data?.state === "online" && <Reachability status={status.data} />}
      </div>
    </div>
  )
}

/** Whether other Soulseek users can connect in, which brings more and faster results. */
function Reachability({ status }: { status: SoulseekStatus }) {
  const mapping = status.port_mapping
  const port = status.listen_port
  let text: string
  let good = false
  if (!port) {
    text = "delune isn't listening for other users, so only people it can reach itself will answer searches."
  } else if (status.reachable) {
    good = true
    text = `Port ${port} is open: other users have connected to delune directly.`
  } else if (mapping.state === "mapped") {
    text = `Your router forwards port ${port} to delune${mapping.external_ip ? ` at ${mapping.external_ip}` : ""}. Nobody has connected in yet.`
  } else {
    text = `Nobody has connected to port ${port} from the internet yet. If searches return few results, forward port ${port} on your router${mapping.state === "failed" ? "" : " or turn on automatic port forwarding under Sharing"}.`
  }
  return (
    <p className={cn("mt-2 text-sm", good ? "text-q-lossless" : "text-muted-foreground")}>
      {text}
      {mapping.state === "failed" && mapping.message && (
        <span className="block text-destructive">Automatic port forwarding: {mapping.message}</span>
      )}
      {status.public_ip && (
        <span className="block text-muted-foreground">
          Soulseek sees you at {status.public_ip}. Behind a VPN, that should be the VPN's address.
        </span>
      )}
    </p>
  )
}

function AvatarPicker() {
  const me = useMe()
  const setAvatar = useSetAvatar()
  const input = useRef<HTMLInputElement>(null)
  return (
    <div className="flex flex-col items-center gap-1.5">
      <button
        type="button"
        onClick={() => input.current?.click()}
        className="group relative rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring"
        aria-label="Change profile picture"
        title="Change profile picture"
      >
        <Avatar name={me.username} src={avatarUrl(me.username, me.avatar)} className="size-14 text-xl" />
        <span className="absolute inset-0 flex items-center justify-center rounded-full bg-black/50 text-white opacity-0 transition-opacity group-hover:opacity-100 [@media(hover:none)]:opacity-60">
          {setAvatar.isPending ? <LoaderCircle className="size-5 animate-spin" /> : <Camera className="size-5" />}
        </span>
      </button>
      {me.avatar && (
        <button
          type="button"
          onClick={() => setAvatar.mutate(null)}
          className="text-[12px] text-muted-foreground hover:text-foreground"
        >
          Remove
        </button>
      )}
      <input
        ref={input}
        type="file"
        accept="image/*"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0]
          // Clear the input only once the picture has been read: clearing it first
          // can leave the browser unable to read the file.
          if (file) setAvatar.mutate(file, { onSettled: () => input.current && (input.current.value = "") })
        }}
      />
      {setAvatar.isError && (
        <p className="max-w-40 text-center text-[12px] text-destructive">{setAvatar.error.message}</p>
      )}
    </div>
  )
}

function AppearancePicker() {
  const me = useMe()
  const set = useSetAppearance()
  const { theme, accent } = me.appearance
  return (
    <div className="space-y-6">
      <div role="radiogroup" aria-label="Theme" className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
        {THEMES.map((t) => (
          <button
            key={t.id}
            type="button"
            role="radio"
            aria-checked={theme === t.id}
            onClick={() => set.mutate({ theme: t.id, accent })}
            className={cn(
              "overflow-hidden rounded-2xl border text-left transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring",
              theme === t.id ? "border-primary ring-1 ring-primary" : "hover:border-foreground/30",
            )}
          >
            <span className="flex h-16" aria-hidden>
              <span className="flex-1" style={{ background: t.swatch[0] }} />
              <span className="flex w-1/3 items-end justify-center pb-2" style={{ background: t.swatch[1] }}>
                <span
                  className="size-3 rounded-full"
                  style={{ background: ACCENTS.find((a) => a.id === accent)?.color }}
                />
              </span>
            </span>
            <span className="block px-3 py-2">
              <span className="block text-[14px] font-medium">{t.label}</span>
              <span className="block text-[12px] leading-snug text-muted-foreground">{t.description}</span>
            </span>
          </button>
        ))}
      </div>
      <div role="radiogroup" aria-label="Accent colour" className="flex flex-wrap gap-2">
        {ACCENTS.map((a) => (
          <button
            key={a.id}
            type="button"
            role="radio"
            aria-checked={accent === a.id}
            onClick={() => set.mutate({ theme, accent: a.id })}
            className={cn(
              "flex h-10 items-center gap-2 rounded-full border px-3.5 text-[14px] transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring",
              accent === a.id
                ? "border-transparent bg-foreground text-background"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            <span className="size-3.5 rounded-full" style={{ background: a.color }} aria-hidden />
            {a.label}
          </button>
        ))}
      </div>
      {set.isError && <p className="text-sm text-destructive">{set.error.message}</p>}
    </div>
  )
}

type ImportOptions = { lyrics: "off" | "sidecar" | "embed" | "both"; embed_cover: boolean }

const LYRICS_CHOICES: { id: ImportOptions["lyrics"]; label: string; description: string }[] = [
  { id: "sidecar", label: "Beside the track", description: "A .lrc file next to each song; most players read it" },
  { id: "embed", label: "In the file", description: "Stored in the track's tags" },
  { id: "both", label: "Both", description: "A .lrc file and the tags" },
  { id: "off", label: "Off", description: "No lyrics" },
]

function ImportOptionsPanel({ editable }: { editable: boolean }) {
  const client = useQueryClient()
  const options = useQuery({
    queryKey: ["import-options"],
    queryFn: async () => {
      const res = await fetch("/api/v1/import-options")
      if (!res.ok) throw await toApiError(res)
      return (await res.json()) as ImportOptions
    },
  })
  const save = useMutation({
    mutationFn: async (next: ImportOptions) => {
      const res = await fetch("/api/v1/import-options", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(next),
      })
      if (!res.ok) throw new Error("Couldn't save.")
      return (await res.json()) as ImportOptions
    },
    onSuccess: (next) => client.setQueryData(["import-options"], next),
  })
  if (options.isError) return <LoadFailed error={options.error} retry={() => void options.refetch()} />
  if (!options.data) return <div className="h-32 animate-pulse rounded-xl bg-muted/50" />
  const o = options.data
  return (
    <div className="space-y-4">
      <div role="radiogroup" aria-label="Lyrics" className="grid gap-2 sm:grid-cols-2">
        {LYRICS_CHOICES.map((choice) => (
          <button
            key={choice.id}
            type="button"
            role="radio"
            aria-checked={o.lyrics === choice.id}
            disabled={!editable}
            onClick={() => save.mutate({ ...o, lyrics: choice.id })}
            className={cn(
              "rounded-2xl border px-4 py-3 text-left transition-colors disabled:cursor-default",
              o.lyrics === choice.id ? "border-primary ring-1 ring-primary" : "enabled:hover:border-foreground/30",
            )}
          >
            <span className="block text-[15px]">{choice.label}</span>
            <span className="block text-[13px] text-muted-foreground">{choice.description}</span>
          </button>
        ))}
      </div>
      <label
        className={cn("flex items-start gap-4 rounded-2xl border bg-card/50 px-5 py-4", editable && "cursor-pointer")}
      >
        <span className="min-w-0 flex-1">
          <span className="block text-[15px]">Embed cover art</span>
          <span className="mt-1 block text-sm text-muted-foreground">
            Put the album cover inside each track, as well as a cover file in the folder.
          </span>
        </span>
        <Switch
          checked={o.embed_cover}
          disabled={!editable}
          onCheckedChange={(embed_cover) => save.mutate({ ...o, embed_cover })}
          className="mt-1"
        />
      </label>
    </div>
  )
}

function AutomationSettingsPanel() {
  const { settings, save } = useAutomation()
  if (settings.isError) return <LoadFailed error={settings.error} retry={() => void settings.refetch()} />
  if (!settings.data) return <div className="h-32 animate-pulse rounded-xl bg-muted/50" />
  const s = settings.data
  const set = (patch: Partial<AutomationSettings>) => save.mutate({ ...s, ...patch })
  const row = (title: string, description: string, checked: boolean, onChange: (v: boolean) => void) => (
    <label className="flex cursor-pointer items-start gap-4 border-b px-5 py-4 last:border-b-0">
      <span className="min-w-0 flex-1">
        <span className="block text-[15px]">{title}</span>
        <span className="mt-1 block text-sm text-muted-foreground">{description}</span>
      </span>
      <Switch checked={checked} onCheckedChange={onChange} className="mt-1" />
    </label>
  )
  return (
    <div className="overflow-hidden rounded-2xl border bg-card/50">
      {row(
        "Quality upgrades",
        "Slowly look through the library for lossy albums and search for better copies.",
        s.quality_upgrades,
        (quality_upgrades) => set({ quality_upgrades }),
      )}
      {s.quality_upgrades && (
        <div className="flex items-center gap-3 border-b px-5 py-3 text-sm">
          <span className="text-muted-foreground">Upgrade to</span>
          <Choice
            value={s.upgrade_to}
            onChange={(upgrade_to) => set({ upgrade_to })}
            options={UPGRADE_OPTIONS}
            label="Upgrade to"
            className="h-9"
          />
        </div>
      )}
      {row(
        "Download what it finds",
        "Otherwise finds stay on the wishlist for someone to download.",
        s.auto_download,
        (auto_download) => set({ auto_download }),
      )}
    </div>
  )
}

function Account() {
  const me = useMe()
  const signOut = useSignOut()
  const allowed = [
    me.permissions.search && "search",
    me.permissions.download && "download",
    me.can_import ? "import your downloads" : "import once an admin approves",
    me.permissions.manage && "manage people",
  ].filter(Boolean)
  return (
    <div className="flex flex-wrap items-center gap-4 rounded-2xl border bg-card/50 px-5 py-5">
      <AvatarPicker />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[17px] font-semibold">{me.username}</p>
        <p className="mt-0.5 text-sm text-muted-foreground">
          {me.mode === "open"
            ? "No Navidrome is configured, so there's no sign-in and you're the admin."
            : `${me.admin ? "Admin" : "Member"}. You can ${allowed.join(", ")}.`}
        </p>
      </div>
      {me.mode === "navidrome" && (
        <Button
          variant="outline"
          className="w-full sm:w-auto"
          onClick={() => signOut.mutate()}
          disabled={signOut.isPending}
        >
          <LogOut /> Sign out
        </Button>
      )}
      {me.mode === "navidrome" && <Devices />}
    </div>
  )
}

/** A settings panel whose data didn't load, with a way to try again. */
function LoadFailed({ error, retry }: { error: Error; retry: () => void }) {
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-destructive/30 bg-destructive/10 px-5 py-4">
      <p className="min-w-0 flex-1 text-[14px]">Couldn't load this: {error.message}</p>
      <Button variant="outline" size="sm" onClick={retry}>
        Try again
      </Button>
    </div>
  )
}

/** Where you're signed in. Folded away: most people never need it. */
function Devices() {
  const [open, setOpen] = useState(false)
  const client = useQueryClient()
  const devices = useQuery({ queryKey: ["devices"], queryFn: ({ signal }) => api.devices(signal) })
  const onDone = (next: SessionInfo[]) => client.setQueryData(["devices"], next)
  const one = useMutation({ meta: { quiet: true }, mutationFn: api.signOutDevice, onSuccess: onDone })
  const others = useMutation({ meta: { quiet: true }, mutationFn: api.signOutOtherDevices, onSuccess: onDone })
  if (!devices.data) return null
  const elsewhere = devices.data.filter((d) => !d.current).length
  return (
    <div className="w-full border-t pt-3">
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-[14px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronRight className={cn("size-4 transition-transform", open && "rotate-90")} />
          Signed in on {plural(devices.data.length, "device")}
          {elsewhere > 0 && !open && <span className="text-muted-foreground/70">, {elsewhere} elsewhere</span>}
        </button>
        {open && elsewhere > 0 && (
          <Button variant="ghost" size="sm" disabled={others.isPending} onClick={() => others.mutate()}>
            Sign out everywhere else
          </Button>
        )}
      </div>
      <ul className={cn("mt-2 space-y-1", !open && "hidden")}>
        {devices.data.map((device) => (
          <li key={device.id} className="flex items-center gap-3 rounded-lg py-1.5 text-[14px]">
            <span className="min-w-0 flex-1 truncate">
              {device.device}
              <span className="text-muted-foreground">
                {device.current
                  ? ", this device"
                  : `, last used ${new Date(device.last_seen * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" })}`}
              </span>
            </span>
            {!device.current && (
              <Button variant="ghost" size="sm" disabled={one.isPending} onClick={() => one.mutate(device.id)}>
                Sign out
              </Button>
            )}
          </li>
        ))}
      </ul>
      {(one.isError || others.isError) && (
        <p className="mt-1 text-sm text-destructive">{(one.error ?? others.error)?.message}</p>
      )}
    </div>
  )
}

const PERMISSIONS: { key: keyof Permissions; label: string; description: string }[] = [
  { key: "search", label: "Search", description: "Search Soulseek and open releases" },
  { key: "download", label: "Download", description: "Start downloads; they still wait for review" },
  { key: "request", label: "Request", description: "Ask for albums for someone who manages delune to approve" },
  { key: "skip_approval", label: "Skip approval", description: "Import their own downloads without an admin" },
  { key: "manage", label: "Manage", description: "See everyone's downloads, approve imports, manage people" },
]

function PeopleSettings() {
  const me = useMe()
  const client = useQueryClient()
  const people = useQuery({ queryKey: ["people"], queryFn: ({ signal }) => api.people(signal) })
  const onSaved = (data: People) => client.setQueryData(["people"], data)
  const approval = useMutation({ meta: { quiet: true }, mutationFn: api.setRequireApproval, onSuccess: onSaved })
  const permissions = useMutation({
    meta: { quiet: true },
    mutationFn: ({ username, next }: { username: string; next: Permissions }) => api.setPermissions(username, next),
    onSuccess: onSaved,
  })
  const signOutPerson = useMutation({ meta: { quiet: true }, mutationFn: api.signOutPerson, onSuccess: onSaved })

  if (me.mode === "open") {
    return (
      <p className="rounded-2xl border bg-card/50 px-5 py-5 text-[15px] text-muted-foreground">
        Connect delune to Navidrome to let other people sign in with their own accounts.
      </p>
    )
  }
  if (people.isError) return <LoadFailed error={people.error} retry={() => void people.refetch()} />
  if (!people.data) return <div className="h-40 animate-pulse rounded-xl bg-muted/50" />
  const { require_approval } = people.data

  return (
    <div className="space-y-5">
      <label className="flex cursor-pointer items-start gap-4 rounded-2xl border bg-card/50 px-5 py-4">
        <div className="min-w-0 flex-1">
          <p className="text-[15px]">Imports need an admin's approval</p>
          <p className="mt-1 text-sm text-muted-foreground">
            People review their own downloads either way. When this is on, only admins and people allowed to skip
            approval can move them into the library.
          </p>
        </div>
        <Switch checked={require_approval} onCheckedChange={(checked) => approval.mutate(checked)} className="mt-1" />
      </label>

      <ul className="overflow-hidden rounded-2xl border bg-card/50">
        {people.data.people.map((person) => (
          <PersonRow
            key={person.username}
            person={person}
            requireApproval={require_approval}
            onChange={(next) => permissions.mutate({ username: person.username, next })}
            onSignOut={person.username === me.username ? undefined : () => signOutPerson.mutate(person.username)}
          />
        ))}
      </ul>
      {(approval.isError || permissions.isError || signOutPerson.isError) && (
        <p className="text-sm text-destructive">
          {(approval.error ?? permissions.error ?? signOutPerson.error)?.message}
        </p>
      )}
    </div>
  )
}

function PersonRow({
  person,
  requireApproval,
  onChange,
  onSignOut,
}: {
  person: Person
  requireApproval: boolean
  onChange: (next: Permissions) => void
  onSignOut?: () => void
}) {
  const lastSeen = new Date(person.last_login * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" })
  return (
    <li className="flex flex-col gap-3 border-b px-5 py-4 last:border-b-0 md:flex-row md:items-center md:gap-5">
      <div className="flex min-w-0 items-center gap-3 md:w-56">
        <Avatar name={person.username} src={avatarUrl(person.username, person.avatar)} />
        <div className="min-w-0">
          <p className="truncate text-[15px] font-medium">{person.username}</p>
          <p className="text-[13px] text-muted-foreground">
            {person.admin ? "Admin" : "Member"}, signed in {lastSeen}
          </p>
          {onSignOut && person.sessions > 0 && (
            <button
              type="button"
              onClick={onSignOut}
              className="mt-0.5 text-[12.5px] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
            >
              Sign out of {person.sessions === 1 ? "their device" : `all ${person.sessions} devices`}
            </button>
          )}
        </div>
      </div>
      {person.admin ? (
        <p className="text-sm text-muted-foreground md:ml-auto">Can do everything. Roles come from Navidrome.</p>
      ) : (
        <div className="flex flex-wrap gap-2 md:ml-auto md:justify-end">
          {PERMISSIONS.map(({ key, label, description }) => {
            const on = person.permissions[key]
            const moot = key === "skip_approval" && !requireApproval
            return (
              <button
                key={key}
                type="button"
                aria-pressed={on}
                title={moot ? "Imports don't need approval right now" : description}
                onClick={() => onChange({ ...person.permissions, [key]: !on })}
                className={cn(
                  "h-9 rounded-full border px-3.5 text-[13.5px] transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  on
                    ? "border-transparent bg-foreground text-background"
                    : "bg-transparent text-muted-foreground hover:text-foreground",
                  moot && "opacity-50",
                )}
              >
                {label}
              </button>
            )
          })}
        </div>
      )}
    </li>
  )
}
