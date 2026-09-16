import { useQuery } from "@tanstack/react-query"
import { getRouteApi } from "@tanstack/react-router"
import { useWindowVirtualizer } from "@tanstack/react-virtual"
import { ChevronRight, Folder, FolderOpen, LoaderCircle, Lock, Search } from "lucide-react"
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react"

import { Cover, type CoverStatus } from "@/components/cover"
import { EmptyState } from "@/components/empty-state"
import { FavouriteStar } from "@/components/favourite-star"
import { ReleaseModal } from "@/components/release-modal"
import { Button } from "@/components/ui/button"
import { api, type Candidate, type ShareFolder, type SoulseekUser } from "@/lib/api"
import { useArtwork } from "@/lib/artwork"
import { useDownloads } from "@/lib/downloads"
import { formatBytes, formatSpeed, plural } from "@/lib/format"
import { TIER_TEXT } from "@/lib/quality"
import { useLibraryAlbum } from "@/lib/library"
import { useRecentList } from "@/lib/recent"
import { buildTree, displayName, initiallyOpen, type TreeNode, visibleRows } from "@/lib/share-tree"
import { initial } from "@/lib/session"
import { albumFromFolder, artistFromFolder } from "@/lib/track-name"
import { cn } from "@/lib/utils"

const route = getRouteApi("/soulseek/users/$username")
const ROW = 44
/** Album folders show their cover, so they're taller. */
const ALBUM_ROW = 64

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
        <div className="flex items-center gap-3">
          <h1 className="type-display min-w-0 truncate text-[clamp(2.2rem,6vw,3.2rem)]">{username}</h1>
          {user?.exists && <FavouriteStar username={user.username} />}
        </div>
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

function savedWhen(at: number) {
  const minutes = Math.round((Date.now() / 1000 - at) / 60)
  if (minutes < 60) return `${Math.max(minutes, 1)} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `${hours} h ago`
  return `${Math.round(hours / 24)} days ago`
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

  // Open the top folders once; a fresh list replacing a saved copy keeps what's open.
  const opened = useRef(false)
  useEffect(() => {
    if (opened.current || !nodes.length) return
    opened.current = true
    setOpen(initiallyOpen(nodes))
  }, [nodes])
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
      {tree.data.saved_at && (
        <p className="mt-1 flex items-center gap-2 text-[13px] text-muted-foreground">
          <LoaderCircle className="size-3.5 animate-spin" />
          Saved {savedWhen(tree.data.saved_at)}, checking for anything new
        </p>
      )}
      {openError && <p className="mt-3 rounded-xl border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm">{openError}</p>}
      {rows.length === 0 ? (
        <p className="px-2 py-10 text-muted-foreground">No folders match “{filter}”.</p>
      ) : (
        <TreeRows
          username={username}
          rows={rows}
          open={open}
          filtering={!!filter.trim()}
          loading={loading}
          onToggle={toggle}
          onOpen={openFolder}
        />
      )}
      <ReleaseModal candidate={release} onClose={() => setRelease(null)} />
    </section>
  )
}

function TreeRows({
  username,
  rows,
  open,
  filtering,
  loading,
  onToggle,
  onOpen,
}: {
  username: string
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
  const height = (index: number) => (rows[index]?.folder?.audio_files ? ALBUM_ROW : ROW)
  const virtualizer = useWindowVirtualizer({ count: rows.length, estimateSize: height, overscan: 12, scrollMargin: margin })
  // Rows change height when the filter changes which rows are albums.
  useLayoutEffect(() => virtualizer.measure(), [rows, virtualizer])

  return (
    <div ref={list} role="tree" aria-label="Shared folders" className="relative mt-2" style={{ height: virtualizer.getTotalSize() }}>
      {virtualizer.getVirtualItems().map((item) => {
        const node = rows[item.index]
        const expandable = node.children.length > 0
        const expanded = filtering || open.has(node.path)
        const folder = node.folder?.audio_files ? node.folder : undefined
        const rowHeight = folder ? ALBUM_ROW : ROW
        return (
          <div
            key={node.path}
            role="treeitem"
            aria-expanded={expandable ? expanded : undefined}
            className="absolute inset-x-0 top-0 flex items-center rounded-lg pr-2 [--indent:10px] hover:bg-accent/50 sm:[--indent:18px]"
            style={{
              height: rowHeight,
              transform: `translateY(${item.start - margin}px)`,
              paddingLeft: `calc(var(--indent) * ${Math.min(node.depth, 8)})`,
            }}
          >
            <button
              type="button"
              aria-label={expandable ? (expanded ? `Close ${node.name}` : `Open ${node.name}`) : undefined}
              disabled={!expandable || filtering}
              onClick={() => onToggle(node.path)}
              className={cn(
                "size-9 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-100",
                expandable ? "flex" : "hidden sm:flex",
              )}
            >
              {expandable ? <ChevronRight className={cn("size-4 transition-transform", expanded && "rotate-90")} /> : null}
            </button>
            {folder ? (
              <AlbumRow username={username} node={node} folder={folder} loading={loading === node.path} onOpen={() => onOpen(node.path)} />
            ) : (
              <button
                type="button"
                onClick={() => expandable && onToggle(node.path)}
                className="flex h-full min-w-0 flex-1 items-center gap-2.5 rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                {expanded && expandable ? (
                  <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
                ) : (
                  <Folder className="size-4 shrink-0 text-muted-foreground" />
                )}
                <span className="min-w-0 flex-1 truncate text-[14.5px] text-foreground/85">{displayName(node.name)}</span>
                {expandable && (
                  <span className="shrink-0 text-[12.5px] text-muted-foreground/70">{plural(node.count, "album")}</span>
                )}
              </button>
            )}
          </div>
        )
      })}
    </div>
  )
}

/**
 * A shared album, shown the way search results are: its cover, the album's own name
 * and artist rather than the folder's, and whether you have it or are getting it.
 */
function AlbumRow({
  username,
  node,
  folder,
  loading,
  onOpen,
}: {
  username: string
  node: TreeNode
  folder: ShareFolder
  loading: boolean
  onOpen: () => void
}) {
  const segments = node.path.split("\\").filter(Boolean)
  const parent = artistFromFolder(segments.at(-2))
  const artwork = useArtwork(parent, node.name)
  const library = useLibraryAlbum(parent, node.name)
  const downloads = useDownloads()
  const job = downloads.data?.find((j) => j.username === username && j.folder === node.path && j.status !== "cancelled")
  const year = /\b(19|20)\d{2}\b/.exec(node.name)?.[0]
  const title = artwork.data?.album ?? albumFromFolder(displayName(node.name))
  const artist = artwork.data?.artist ?? parent
  const status: CoverStatus | undefined =
    job && (job.status === "queued" || job.status === "downloading")
      ? { kind: "downloading", progress: job.total_bytes ? job.bytes / job.total_bytes : 0 }
      : job?.status === "imported" || library.data?.state === "in-library"
        ? { kind: "in-library" }
        : undefined
  const tier = tierFromLabel(folder.quality_label)

  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex h-full min-w-0 flex-1 items-center gap-3 rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span className="relative">
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} status={status} alt="" className="size-12 rounded-lg" />
        {loading && (
          <span className="absolute inset-0 flex items-center justify-center rounded-lg bg-background/60">
            <LoaderCircle className="size-5 animate-spin text-primary" />
          </span>
        )}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[15px] font-medium">{title}</span>
        <span className="block truncate text-[13px] text-muted-foreground">
          {[artist, year].filter(Boolean).join(" · ") || displayName(node.name)}
        </span>
      </span>
      <span className="flex shrink-0 flex-col items-end gap-0.5 text-[12.5px] sm:flex-row sm:items-center sm:gap-4">
        <span className={cn("font-semibold", TIER_TEXT[tier])}>{folder.quality_label ?? "Unknown"}</span>
        <span className="text-muted-foreground">
          {status?.kind === "in-library" ? (
            <span className="text-q-lossless">In library</span>
          ) : status?.kind === "downloading" ? (
            <span className="text-primary">Downloading</span>
          ) : (
            plural(folder.audio_files, "track")
          )}
        </span>
        <span className="hidden w-16 text-right text-muted-foreground md:inline">{formatBytes(folder.bytes)}</span>
      </span>
    </button>
  )
}

/** Share trees carry labels, not full qualities; the label's shape is enough to colour it. */
function tierFromLabel(label: string | null) {
  if (!label) return "unknown" as const
  if (/\b24\/|\/(88|96|176|192)/.test(label)) return "hires" as const
  if (/^(FLAC|ALAC|WAV|AIFF|APE|WV)/.test(label)) return "lossless" as const
  return "lossy" as const
}
