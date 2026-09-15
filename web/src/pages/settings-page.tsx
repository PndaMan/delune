import { useQuery } from "@tanstack/react-query"
import { Lock } from "lucide-react"

import { NamingEditor } from "@/components/naming-editor"
import { describeSoulseek, useSoulseekStatus } from "@/components/soulseek-indicator"
import { Moon } from "@/components/moon"
import { PageFrame } from "@/pages/placeholder-pages"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"

export function SettingsPage() {
  return (
    <PageFrame title="Settings" wide>
      <p className="max-w-[60ch] text-[15px] text-muted-foreground">
        Settings aren't saved yet. These screens show what delune will use, and the file naming editor previews exactly
        how your library will be named.
      </p>
      <div className="mt-10 divide-y border-t pb-24">
        <Section title="Sources" description="Where delune looks for music, in order. Soulseek always comes first.">
          <Sources />
        </Section>
        <Section title="Soulseek account" description="delune connects to Soulseek itself; no separate client needed.">
          <SoulseekAccount />
        </Section>
        <Section
          title="File naming"
          description="How folders and files are named when a release is imported. Click a token to insert it."
        >
          <NamingEditor />
        </Section>
      </div>
    </PageFrame>
  )
}

function Section({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return (
    <section className="grid gap-6 py-10 lg:grid-cols-[260px_minmax(0,1fr)] lg:gap-12">
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
