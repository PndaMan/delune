import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Camera, LoaderCircle, LogOut, Lock } from "lucide-react"
import { useEffect, useRef } from "react"

import { NamingEditor } from "@/components/naming-editor"
import { SharingSettingsPanel } from "@/components/sharing-settings"
import { describeSoulseek, useSoulseekStatus } from "@/components/soulseek-indicator"
import { Moon } from "@/components/moon"
import { Avatar } from "@/components/profile-menu"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { ACCENTS, avatarUrl, THEMES, useSetAppearance, useSetAvatar } from "@/lib/appearance"
import { useMe, useSignOut } from "@/lib/session"
import { PageFrame } from "@/pages/placeholder-pages"
import { api, type People, type Permissions, type Person } from "@/lib/api"
import { cn } from "@/lib/utils"

export function SettingsPage() {
  const me = useMe()
  useEffect(() => {
    const target = window.location.hash.slice(1)
    if (target) document.getElementById(target)?.scrollIntoView({ block: "start" })
  }, [])
  return (
    <PageFrame title="Settings" wide>
      <p className="max-w-[60ch] text-[15px] text-muted-foreground">
        People, permissions and sharing save as you change them. The other sections show what delune uses today, and the
        file naming editor previews exactly how your library will be named.
      </p>
      <SectionNav manage={me.permissions.manage} />
      <div className="mt-10 divide-y border-t pb-24 md:mt-10">
        <Section id="account" title="Your account" description="delune uses your Navidrome account. Admins in Navidrome are admins here.">
          <Account />
        </Section>
        <Section id="appearance" title="Appearance" description="How delune looks for you, on every device you sign in on.">
          <AppearancePicker />
        </Section>
        {me.permissions.manage && (
          <Section
            id="people"
            title="People"
            description="Everyone who has signed in. Admins can do everything; choose what everyone else can do."
          >
            <PeopleSettings />
          </Section>
        )}
        <Section id="sources" title="Sources" description="Where delune looks for music, in order. Soulseek always comes first.">
          <Sources />
        </Section>
        {me.permissions.manage && (
          <Section
            id="sharing"
            title="Sharing"
            description="Let other Soulseek users browse and download your library. Off until you turn it on."
          >
            <SharingSettingsPanel />
          </Section>
        )}
        <Section id="soulseek" title="Soulseek account" description="delune connects to Soulseek itself; no separate client needed.">
          <SoulseekAccount />
        </Section>
        <Section
          id="naming"
          title="File naming"
          description="How folders and files are named when a release is imported. Click a token to insert it."
        >
          <NamingEditor />
        </Section>
      </div>
    </PageFrame>
  )
}

/** Phones: jump between sections instead of scrolling past all of them. */
function SectionNav({ manage }: { manage: boolean }) {
  const sections = [
    ["account", "Account"],
    ["appearance", "Appearance"],
    ...(manage ? [["people", "People"]] : []),
    ["sources", "Sources"],
    ...(manage ? [["sharing", "Sharing"]] : []),
    ["soulseek", "Soulseek"],
    ["naming", "File naming"],
  ]
  return (
    <nav
      aria-label="Settings sections"
      className="sticky top-0 z-20 -mx-5 mt-6 flex gap-2 overflow-x-auto px-5 py-2.5 backdrop-blur-xl [scrollbar-width:none] supports-[backdrop-filter]:bg-background/60 md:hidden"
    >
      {sections.map(([id, label]) => (
        <a
          key={id}
          href={`#${id}`}
          onClick={(e) => {
            e.preventDefault()
            document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" })
            history.replaceState(null, "", `#${id}`)
          }}
          className="flex h-9 shrink-0 items-center rounded-full border bg-card/60 px-3.5 text-[14px] whitespace-nowrap text-muted-foreground"
        >
          {label}
        </a>
      ))}
    </nav>
  )
}

function Section({
  id,
  title,
  description,
  children,
}: {
  id?: string
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <section id={id} className="grid scroll-mt-16 gap-6 md:scroll-mt-6 py-10 lg:grid-cols-[260px_minmax(0,1fr)] lg:gap-12">
      <div>
        <h2 className="type-title text-[21px]">{title}</h2>
        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{description}</p>
      </div>
      <div className="min-w-0">{children}</div>
    </section>
  )
}

function Sources() {
  const sources = useQuery({ queryKey: ["sources"], queryFn: ({ signal }) => api.sources(signal) })
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
            : "Set DELUNE_SLSK_USERNAME and DELUNE_SLSK_PASSWORD when starting the server. A new username is registered the first time it logs in."}
        </p>
      </div>
    </div>
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
        <span className="absolute inset-0 flex items-center justify-center rounded-full bg-black/50 text-white opacity-0 transition-opacity group-hover:opacity-100">
          {setAvatar.isPending ? <LoaderCircle className="size-5 animate-spin" /> : <Camera className="size-5" />}
        </span>
      </button>
      {me.avatar && (
        <button type="button" onClick={() => setAvatar.mutate(null)} className="text-[12px] text-muted-foreground hover:text-foreground">
          Remove
        </button>
      )}
      <input
        ref={input}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) setAvatar.mutate(file)
          e.target.value = ""
        }}
      />
      {setAvatar.isError && <p className="max-w-40 text-center text-[12px] text-destructive">{setAvatar.error.message}</p>}
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
                <span className="size-3 rounded-full" style={{ background: ACCENTS.find((a) => a.id === accent)?.color }} />
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
              accent === a.id ? "border-transparent bg-foreground text-background" : "text-muted-foreground hover:text-foreground",
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
        <Button variant="outline" className="w-full sm:w-auto" onClick={() => signOut.mutate()} disabled={signOut.isPending}>
          <LogOut /> Sign out
        </Button>
      )}
    </div>
  )
}

const PERMISSIONS: { key: keyof Permissions; label: string; description: string }[] = [
  { key: "search", label: "Search", description: "Search Soulseek and open releases" },
  { key: "download", label: "Download", description: "Start downloads; they still wait for review" },
  { key: "skip_approval", label: "Skip approval", description: "Import their own downloads without an admin" },
  { key: "manage", label: "Manage", description: "See everyone's downloads, approve imports, manage people" },
]

function PeopleSettings() {
  const me = useMe()
  const client = useQueryClient()
  const people = useQuery({ queryKey: ["people"], queryFn: ({ signal }) => api.people(signal) })
  const onSaved = (data: People) => client.setQueryData(["people"], data)
  const approval = useMutation({ mutationFn: api.setRequireApproval, onSuccess: onSaved })
  const permissions = useMutation({
    mutationFn: ({ username, next }: { username: string; next: Permissions }) => api.setPermissions(username, next),
    onSuccess: onSaved,
  })

  if (me.mode === "open") {
    return (
      <p className="rounded-2xl border bg-card/50 px-5 py-5 text-[15px] text-muted-foreground">
        Connect delune to Navidrome to let other people sign in with their own accounts.
      </p>
    )
  }
  if (!people.data) return <div className="h-40 animate-pulse rounded-xl bg-muted/50" />
  const { require_approval } = people.data

  return (
    <div className="space-y-5">
      <label className="flex cursor-pointer items-start gap-4 rounded-2xl border bg-card/50 px-5 py-4">
        <div className="min-w-0 flex-1">
          <p className="text-[15px]">Imports need an admin's approval</p>
          <p className="mt-1 text-sm text-muted-foreground">
            People review their own downloads either way. When this is on, only admins and people allowed to skip approval
            can move them into the library.
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
          />
        ))}
      </ul>
      {(approval.isError || permissions.isError) && (
        <p className="text-sm text-destructive">{(approval.error ?? permissions.error)?.message}</p>
      )}
    </div>
  )
}

function PersonRow({
  person,
  requireApproval,
  onChange,
}: {
  person: Person
  requireApproval: boolean
  onChange: (next: Permissions) => void
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
                  on ? "border-transparent bg-foreground text-background" : "bg-transparent text-muted-foreground hover:text-foreground",
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
