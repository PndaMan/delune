import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { Ban, X } from "lucide-react"
import { useMemo } from "react"

import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { formatBytes, formatSpeed } from "@/lib/format"
import { type Upload, sharingApi, useSharingStatus, useUploads } from "@/lib/sharing"
import { cn } from "@/lib/utils"

/** Who's downloading from us, who's waiting, and what went out recently. */
export function UploadsPage() {
  const client = useQueryClient()
  const uploads = useUploads()
  const sharing = useSharingStatus()
  const refresh = () => void client.invalidateQueries({ queryKey: ["uploads"] })
  const cancel = useMutation({ mutationFn: sharingApi.cancel, onSuccess: refresh })
  const clear = useMutation({ mutationFn: sharingApi.clear, onSuccess: refresh })
  const ban = useMutation({
    mutationFn: async (username: string) => {
      const current = sharing.data?.settings
      if (!current) return
      await sharingApi.update({ ...current, banned: [...current.banned, username] })
    },
    onSuccess: () => void client.invalidateQueries({ queryKey: ["sharing"] }),
  })

  const groups = useMemo(() => {
    const all = uploads.data ?? []
    return {
      running: all.filter((u) => u.status === "transferring" || u.status === "connecting"),
      queued: all.filter((u) => u.status === "queued"),
      finished: all.filter((u) => u.status === "completed" || u.status === "failed" || u.status === "cancelled"),
    }
  }, [uploads.data])

  if (sharing.data && !sharing.data.settings.enabled && !uploads.data?.length) {
    return (
      <EmptyState
        illumination={0.1}
        title="You're not sharing anything yet"
        action={
          <Button variant="outline" nativeButton={false} render={<Link to="/settings/$section" params={{ section: "sharing" }} />}>
            Set up sharing
          </Button>
        }
      >
        Share your library so others can download from you. Many people on Soulseek only share with those who share back.
      </EmptyState>
    )
  }

  return (
    <div className="space-y-8 pb-24">
      <Group title="Sending now" empty="Nobody is downloading from you right now." uploads={groups.running} onCancel={cancel.mutate} onBan={ban.mutate} />
      <Group title="Waiting" empty="Nobody is waiting." uploads={groups.queued} onCancel={cancel.mutate} onBan={ban.mutate} />
      {groups.finished.length > 0 && (
        <Group
          title="Finished"
          uploads={groups.finished}
          action={
            <Button variant="ghost" size="sm" onClick={() => clear.mutate()}>
              Clear
            </Button>
          }
        />
      )}
    </div>
  )
}

function Group({
  title,
  empty,
  uploads,
  action,
  onCancel,
  onBan,
}: {
  title: string
  empty?: string
  uploads: Upload[]
  action?: React.ReactNode
  onCancel?: (id: number) => void
  onBan?: (username: string) => void
}) {
  return (
    <section>
      <div className="flex items-center gap-3 pb-3">
        <h2 className="type-title text-[19px]">{title}</h2>
        {uploads.length > 0 && <span className="text-sm text-muted-foreground">{uploads.length.toLocaleString()}</span>}
        <div className="ml-auto">{action}</div>
      </div>
      {uploads.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-5 py-6 text-center text-sm text-muted-foreground">{empty}</p>
      ) : (
        <ul className="overflow-hidden rounded-2xl border bg-card/50">
          {uploads.slice(0, 200).map((u) => (
            <UploadRow key={u.id} upload={u} onCancel={onCancel} onBan={onBan} />
          ))}
        </ul>
      )}
    </section>
  )
}

function UploadRow({ upload: u, onCancel, onBan }: { upload: Upload; onCancel?: (id: number) => void; onBan?: (username: string) => void }) {
  const name = u.filename.split("\\").pop() ?? u.filename
  const folder = u.filename.split("\\").slice(1, -1).join(" / ")
  const progress = u.size ? Math.min(1, u.bytes / u.size) : 0
  const active = u.status === "transferring" || u.status === "connecting"
  const detail =
    u.status === "transferring"
      ? `${formatBytes(u.bytes)} of ${formatBytes(u.size)}${formatSpeed(u.speed) ? `, ${formatSpeed(u.speed)}` : ""}`
      : u.status === "connecting"
        ? "Connecting"
        : u.status === "queued"
          ? formatBytes(u.size)
          : u.status === "completed"
            ? "Sent"
            : u.status === "failed"
              ? (u.reason ?? "Failed")
              : "Cancelled"

  return (
    <li className="border-b px-4 py-3 last:border-b-0 sm:px-5">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-[15px]">{name}</p>
          <p className="truncate text-[13px] text-muted-foreground">
            <Link to="/soulseek/users/$username" params={{ username: u.username }} className="text-foreground/85 hover:underline">
              {u.username}
            </Link>
            {folder && <span>, {folder}</span>}
          </p>
        </div>
        <span className={cn("hidden shrink-0 text-[13px] sm:block", u.status === "failed" ? "text-destructive" : "text-muted-foreground")}>
          {detail}
        </span>
        {onBan && (
          <Button variant="ghost" size="icon-sm" onClick={() => onBan(u.username)} aria-label={`Block ${u.username}`} title={`Block ${u.username}`} className="text-muted-foreground">
            <Ban />
          </Button>
        )}
        {onCancel && (
          <Button variant="ghost" size="icon-sm" onClick={() => onCancel(u.id)} aria-label="Cancel upload" className="text-muted-foreground">
            <X />
          </Button>
        )}
      </div>
      <p className={cn("mt-1 text-[12.5px] sm:hidden", u.status === "failed" ? "text-destructive" : "text-muted-foreground")}>{detail}</p>
      {active && (
        <div className="mt-2 h-1 overflow-hidden rounded-full bg-muted">
          <div className="h-full bg-primary transition-[width] duration-500" style={{ width: `${Math.max(progress * 100, 2)}%` }} />
        </div>
      )}
    </li>
  )
}
