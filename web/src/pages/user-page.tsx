import { useQuery } from "@tanstack/react-query"
import { getRouteApi } from "@tanstack/react-router"
import { useWindowVirtualizer } from "@tanstack/react-virtual"
import { ChevronRight, Folder, FolderOpen, LoaderCircle, Lock, Search } from "lucide-react"
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react"

import { EmptyState } from "@/components/empty-state"
import { ReleaseModal } from "@/components/release-modal"
import { Button } from "@/components/ui/button"
import { api, type Candidate, type SoulseekUser } from "@/lib/api"
import { formatBytes, formatSpeed, plural } from "@/lib/format"
import { TIER_TEXT } from "@/lib/quality"
import { useRecentList } from "@/lib/recent"
import { buildTree, displayName, initiallyOpen, type TreeNode, visibleRows } from "@/lib/share-tree"
import { initial } from "@/lib/session"
import { cn } from "@/lib/utils"

const route = getRouteApi("/soulseek/users/$username")
const ROW = 44

/** Another Soulseek user: who they are, and everything they share, to open and download. */
export function UserPage() {
  const { username } = route.useParams()
  const { remember } = useRecentList("delune.recent-users")
  const user = useQuery({
    queryKey: ["soulseek-user", username],
    queryFn: ({ signal }) => api.soulseekUser(username, signal),
    staleTime: 60_000,
    retry: false,
  })
  useEffect(() => {
    if (user.data?.exists) remember(user.data.username)
  }, [user.data, remember])

  return (
    <div className="pb-24">
      <Profile username={username} user={user.data} pending={user.isPending} error={user.error?.message} />
      {user.data?.exists === false ? (
        <EmptyState illumination={0} title={`There's nobody called ${username}`}>
          Soulseek usernames are case-sensitive. Check the spelling, or find them through a search result.
        </EmptyState>
      ) : user.data?.presence === "offline" ? (
        <EmptyState illumination={0.05} title={`${username} is offline`}>
          Their shares can be browsed when they're back online.
        </EmptyState>
      ) : user.data ? (
        <Shares username={username} />
      ) : null}
    </div>
  )
}

function Profile({ username, user, pending, error }: { username: string; user?: SoulseekUser; pending: boolean; error?: string }) {
  const picture = user?.profile?.has_picture ? `/api/v1/soulseek/users/${encodeURIComponent(username)}/picture` : null
  const speed = user ? formatSpeed(user.avg_speed) : null
  const presence = user?.presence ?? "offline"

  return (
    <header className="flex flex-col gap-6 pt-10 pb-8 sm:flex-row sm:items-center sm:pt-14">
      <div className="relative size-24 shrink-0 sm:size-28">
        {picture ? (
          <img src={picture} alt="" className="size-full rounded-3xl object-cover" />
        ) : (
          <span className="flex size-full items-center justify-center rounded-3xl bg-primary/15 text-4xl font-semibold text-primary">
            {initial(username)}
          </span>
        )}
        {user && (
          <span
            className={cn(
              "absolute -right-1 -bottom-1 size-5 rounded-full ring-4 ring-background",
              presence === "online" ? "bg-q-lossless" : presence === "away" ? "bg-q-hires" : "bg-muted-foreground/50",
            )}
            aria-hidden
          />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <h1 className="type-display truncate text-[clamp(2.2rem,6vw,3.2rem)]">{username}</h1>
        {pending ? (
          <p className="mt-2 flex items-center gap-2 text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" /> Asking the Soulseek server about them
          </p>
        ) : error ? (
          <p className="mt-2 text-destructive">{error}</p>
        ) : user?.exists ? (
          <>
            <p className="mt-1.5 text-[16px] text-foreground/85">
              {presence === "online" ? "Online" : presence === "away" ? "Away" : "Offline"}
              {user.country ? ` in ${countryName(user.country)}` : ""}
            </p>
            <dl className="mt-4 flex flex-wrap gap-x-7 gap-y-2 text-sm">
              <Fact label="Shares" value={`${user.files.toLocaleString()} files, ${user.folders.toLocaleString()} folders`} />
              {speed && <Fact label="Upload speed" value={speed} />}
              {user.profile && (
                <Fact
                  label="Queue"
                  value={user.profile.slots_free ? "Free slot now" : `${plural(user.profile.queue_size, "person", "people")} waiting`}
                  good={user.profile.slots_free}
                />
              )}
            </dl>
            {user.profile?.description.trim() && (
              <p className="mt-4 max-w-[70ch] text-[14.5px] whitespace-pre-line text-muted-foreground">
                {user.profile.description.trim()}
              </p>
            )}
          </>
        ) : null}
      </div>
    </header>
  )
}

function Fact({ label, value, good }: { label: string; value: string; good?: boolean }) {
  return (
    <div>
      <dt className="text-[12.5px] text-muted-foreground">{label}</dt>
      <dd className={cn("text-[15px]", good && "text-q-lossless")}>{value}</dd>
    </div>
  )
}

function countryName(code: string) {
  try {
    return new Intl.DisplayNames(undefined, { type: "region" }).of(code) ?? code
  } catch {
    return code
  }
}

function Shares({ username }: { username: string }) {
  const tree = useQuery({
    queryKey: ["share-tree", username],
    queryFn: ({ signal }) => api.shareTree(username, signal),
    staleTime: 5 * 60_000,
    retry: false,
  })
  const nodes = useMemo(() => buildTree(tree.data?.folders ?? []), [tree.data])
  const [open, setOpen] = useState<Set<string>>(new Set())
  const [filter, setFilter] = useState("")
  const [loading, setLoading] = useState<string | null>(null)
  const [release, setRelease] = useState<Candidate | null>(null)
  const [openError, setOpenError] = useState<string | null>(null)

  useEffect(() => setOpen(initiallyOpen(nodes)), [nodes])
  const rows = useMemo(() => visibleRows(nodes, open, filter), [nodes, open, filter])

  const toggle = (path: string) =>
    setOpen((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })

  const openFolder = async (path: string) => {
    setLoading(path)
    setOpenError(null)
    try {
      setRelease(await api.sharedFolder(username, path))
    } catch (e) {
      setOpenError(e instanceof Error ? e.message : "Couldn't open that folder.")
    } finally {
      setLoading(null)
    }
  }

  if (tree.isPending) {
    return (
      <div className="flex items-center gap-3 rounded-2xl border bg-card/50 px-5 py-6 text-muted-foreground">
        <LoaderCircle className="size-5 animate-spin" />
        Fetching everything {username} shares. Big libraries take up to a minute.
      </div>
    )
  }
  if (tree.isError) {
    return (
      <EmptyState
        illumination={0}
        title="Couldn't browse their shares"
        action={
          <Button variant="outline" onClick={() => void tree.refetch()}>
            Try again
          </Button>
        }
      >
        {tree.error.message}
      </EmptyState>
    )
  }
  if (!tree.data.folders.length) {
    return (
      <EmptyState illumination={0.1} title={`${username} isn't sharing anything`}>
        {tree.data.private_folders > 0
          ? `They share ${plural(tree.data.private_folders, "folder")} with their buddies only.`
          : "Some people only download."}
      </EmptyState>
    )
  }

  return (
    <section>
      <div className="sticky top-0 z-10 -mx-4 flex flex-wrap items-center gap-3 px-4 py-3 backdrop-blur-xl supports-[backdrop-filter]:bg-background/40 sm:-mx-10 sm:px-10">
        <label className="flex h-11 min-w-0 flex-1 items-center gap-2.5 rounded-xl border bg-card/70 px-3.5 focus-within:border-primary/50">
          <Search className="size-4 text-muted-foreground" aria-hidden />
          <span className="sr-only">Filter folders</span>
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={`Filter ${tree.data.folders.length.toLocaleString()} folders`}
            className="h-full min-w-0 flex-1 bg-transparent text-[15px] outline-none placeholder:text-muted-foreground/60"
          />
        </label>
        {tree.data.private_folders > 0 && (
          <span className="flex items-center gap-1.5 text-[13px] text-muted-foreground">
            <Lock className="size-3.5" /> {plural(tree.data.private_folders, "private folder")} hidden
          </span>
        )}
      </div>
      {openError && <p className="mt-3 rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm">{openError}</p>}
      {rows.length === 0 ? (
        <p className="px-2 py-10 text-muted-foreground">No folders match “{filter}”.</p>
      ) : (
        <TreeRows rows={rows} open={open} filtering={!!filter.trim()} loading={loading} onToggle={toggle} onOpen={openFolder} />
      )}
      <ReleaseModal candidate={release} onClose={() => setRelease(null)} />
    </section>
  )
}

function TreeRows({
  rows,
  open,
  filtering,
  loading,
  onToggle,
  onOpen,
}: {
  rows: TreeNode[]
  open: Set<string>
  filtering: boolean
  loading: string | null
  onToggle: (path: string) => void
  onOpen: (path: string) => void
}) {
  const list = useRef<HTMLDivElement>(null)
  const [margin, setMargin] = useState(0)
  useLayoutEffect(() => {
    const measure = () => list.current && setMargin(list.current.getBoundingClientRect().top + window.scrollY)
    measure()
    window.addEventListener("resize", measure)
    return () => window.removeEventListener("resize", measure)
  }, [])
  const virtualizer = useWindowVirtualizer({ count: rows.length, estimateSize: () => ROW, overscan: 12, scrollMargin: margin })

  return (
    <div ref={list} role="tree" aria-label="Shared folders" className="relative mt-2" style={{ height: virtualizer.getTotalSize() }}>
      {virtualizer.getVirtualItems().map((item) => {
        const node = rows[item.index]
        const expandable = node.children.length > 0
        const expanded = filtering || open.has(node.path)
        const folder = node.folder?.audio_files ? node.folder : undefined
        return (
          <div
            key={node.path}
            role="treeitem"
            aria-expanded={expandable ? expanded : undefined}
            className="absolute inset-x-0 top-0 flex items-center rounded-lg pr-2 hover:bg-accent/50"
            style={{ height: ROW, transform: `translateY(${item.start - margin}px)`, paddingLeft: `${Math.min(node.depth, 8) * 18}px` }}
          >
            <button
              type="button"
              aria-label={expandable ? (expanded ? `Close ${node.name}` : `Open ${node.name}`) : undefined}
              disabled={!expandable || filtering}
              onClick={() => onToggle(node.path)}
              className="flex size-9 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-100"
            >
              {expandable ? <ChevronRight className={cn("size-4 transition-transform", expanded && "rotate-90")} /> : null}
            </button>
            <button
              type="button"
              onClick={() => (folder ? onOpen(node.path) : expandable && onToggle(node.path))}
              className="flex h-full min-w-0 flex-1 items-center gap-2.5 rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              {loading === node.path ? (
                <LoaderCircle className="size-4 shrink-0 animate-spin text-primary" />
              ) : expanded && expandable ? (
                <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
              ) : (
                <Folder className={cn("size-4 shrink-0", folder ? "text-primary/80" : "text-muted-foreground")} />
              )}
              <span className={cn("min-w-0 flex-1 truncate text-[14.5px]", !folder && "text-foreground/85")}>{displayName(node.name)}</span>
              {folder ? (
                <span className="flex shrink-0 items-center gap-3 text-[12.5px] text-muted-foreground">
                  <span className={cn("hidden sm:inline", TIER_TEXT[tierFromLabel(folder.quality_label)])}>{folder.quality_label}</span>
                  <span className="w-16 text-right">{plural(folder.audio_files, "track")}</span>
                  <span className="hidden w-16 text-right md:inline">{formatBytes(folder.bytes)}</span>
                </span>
              ) : expandable ? (
                <span className="shrink-0 text-[12.5px] text-muted-foreground/60">{node.count.toLocaleString()}</span>
              ) : null}
            </button>
          </div>
        )
      })}
    </div>
  )
}

/** Share trees carry labels, not full qualities; the label's shape is enough to colour it. */
function tierFromLabel(label: string | null) {
  if (!label) return "unknown" as const
  if (/\b24\/|\/(88|96|176|192)/.test(label)) return "hires" as const
  if (/^(FLAC|ALAC|WAV|AIFF|APE|WV)/.test(label)) return "lossless" as const
  return "lossy" as const
}
