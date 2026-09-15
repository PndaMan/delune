import { Check, LoaderCircle, X } from "lucide-react"
import { useState } from "react"

import { Cover } from "@/components/cover"
import { Button } from "@/components/ui/button"
import { useArtwork } from "@/lib/artwork"
import { plural } from "@/lib/format"
import { type MusicRequest, REQUEST_STATUS, useDecideRequest, useRemoveRequest, useRequests } from "@/lib/requests"
import { useMe } from "@/lib/session"
import { cn } from "@/lib/utils"

/** Albums people asked for: theirs, or everyone's for people who manage delune. */
export function RequestsSection() {
  const me = useMe()
  const requests = useRequests()
  const items = requests.data ?? []
  if (!items.length) return null
  const pending = items.filter((r) => r.status === "pending").length

  return (
    <section className="mt-10">
      <div className="flex flex-wrap items-end gap-x-6 gap-y-2">
        <h2 className="type-title text-[24px]">Requests</h2>
        <p className="text-sm text-muted-foreground">
          {me.permissions.manage
            ? pending
              ? `${plural(pending, "request")} waiting for you.`
              : "Nothing waiting for approval."
            : "Albums you asked for, and where they've got to."}
        </p>
      </div>
      <ul className="mt-4 space-y-3">
        {items.map((request) => (
          <RequestRow key={request.id} request={request} />
        ))}
      </ul>
    </section>
  )
}

function RequestRow({ request }: { request: MusicRequest }) {
  const me = useMe()
  const artwork = useArtwork(request.artist, request.title)
  const decide = useDecideRequest()
  const remove = useRemoveRequest()
  const [declining, setDeclining] = useState(false)
  const [reason, setReason] = useState("")
  const mine = request.requested_by === me.username
  const canDecide = me.permissions.manage && request.status === "pending"

  return (
    <li className="overflow-hidden rounded-2xl border bg-card/60">
      <div className="flex flex-wrap items-center gap-4 p-4 sm:flex-nowrap sm:p-5">
        <Cover src={artwork.data?.thumb} pending={artwork.isPending} alt="" className="size-14 rounded-xl sm:size-16" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[16px] font-semibold">{request.title}</p>
          <p className="truncate text-sm text-muted-foreground">
            {request.artist ?? "Unknown artist"}
            {!mine && <span className="text-muted-foreground/70">, asked by {request.requested_by}</span>}
            {request.quality_label && <span className="text-muted-foreground/70">, {request.quality_label}</span>}
          </p>
          <p
            className={cn(
              "mt-1.5 text-[13.5px]",
              request.status === "available" && "text-q-lossless",
              (request.status === "declined" || request.status === "failed") && "text-destructive",
              request.status === "pending" && "text-q-hires",
            )}
          >
            {REQUEST_STATUS[request.status]}
            {request.decided_by && request.status !== "pending" && (
              <span className="text-muted-foreground">
                , {request.status === "declined" ? "by" : "approved by"} {request.decided_by}
              </span>
            )}
          </p>
          {request.note && <p className="mt-1 text-[13.5px] text-muted-foreground">“{request.note}”</p>}
          {request.reason && <p className="mt-1 text-[13.5px] text-muted-foreground">Reason: {request.reason}</p>}
        </div>
        <div className="flex w-full items-center justify-end gap-2 sm:w-auto">
          {canDecide && !declining && (
            <>
              <Button variant="outline" size="sm" disabled={decide.isPending} onClick={() => setDeclining(true)}>
                <X /> Decline
              </Button>
              <Button
                size="sm"
                disabled={decide.isPending}
                onClick={() => decide.mutate({ id: request.id, approve: true })}
              >
                {decide.isPending ? <LoaderCircle className="animate-spin" /> : <Check />} Approve
              </Button>
            </>
          )}
          {!canDecide && (mine || me.permissions.manage) && (
            <Button variant="ghost" size="sm" disabled={remove.isPending} onClick={() => remove.mutate(request.id)}>
              {request.status === "pending" ? "Withdraw" : "Remove"}
            </Button>
          )}
        </div>
      </div>
      {declining && (
        <form
          className="flex flex-wrap items-center gap-2 border-t bg-destructive/8 px-4 py-3 sm:px-5"
          onSubmit={(e) => {
            e.preventDefault()
            decide.mutate({ id: request.id, approve: false, reason }, { onSuccess: () => setDeclining(false) })
          }}
        >
          <input
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="Why not? (optional, they'll see it)"
            maxLength={500}
            className="h-9 min-w-0 flex-1 rounded-lg border bg-background/60 px-3 text-[14px] outline-none focus:border-primary/50"
          />
          <Button type="button" variant="ghost" size="sm" onClick={() => setDeclining(false)}>
            Keep it
          </Button>
          <Button type="submit" variant="destructive" size="sm" disabled={decide.isPending}>
            Decline
          </Button>
        </form>
      )}
      {(decide.isError || remove.isError) && (
        <p className="border-t px-5 py-2 text-sm text-destructive">{(decide.error ?? remove.error)?.message}</p>
      )}
    </li>
  )
}
