import { Dialog } from "@base-ui/react/dialog"
import { Link, useNavigate } from "@tanstack/react-router"
import {
  ArrowDownToLine,
  ArrowUpRight,
  CircleAlert,
  CircleCheck,
  ExternalLink,
  FileUp,
  LoaderCircle,
  RefreshCw,
  X,
} from "lucide-react"
import { useRef, useState } from "react"

import { Cover } from "@/components/cover"
import { Button } from "@/components/ui/button"
import {
  type BandcampPurchase,
  formatPrice,
  useBandcampAccount,
  useBandcampOffer,
  useBandcampPurchases,
  useDownloadPurchase,
  useLinkBandcamp,
  useSyncBandcamp,
  useUnlinkBandcamp,
} from "@/lib/bandcamp"
import { useDownloads } from "@/lib/downloads"
import { useMe } from "@/lib/session"
import { cn } from "@/lib/utils"

/** Bandcamp's mark: a leaning parallelogram. */
// `strokeWidth` is accepted so the mark can stand in for an icon; it has no strokes.
export function BandcampGlyph({ className }: { className?: string; strokeWidth?: number }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden className={cn("size-4", className)}>
      <path d="M0 18.75 7.44 5.25H24l-7.44 13.5z" fill="currentColor" />
    </svg>
  )
}

const FORMATS = [
  { id: "flac", label: "FLAC" },
  { id: "alac", label: "ALAC" },
  { id: "mp3-320", label: "MP3 320" },
  { id: "mp3-v0", label: "MP3 V0" },
] as const

function ago(at: number) {
  const minutes = Math.round((Date.now() / 1000 - at) / 60)
  if (minutes < 1) return "just now"
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `${hours} h ago`
  return `${Math.round(hours / 24)} days ago`
}

/**
 * The Bandcamp line in an album view: what it costs there, or that you own it and
 * can fetch the files again. Nothing shows when the album isn't on Bandcamp.
 */
export function BandcampOfferLine({ artist, title }: { artist: string | null; title: string }) {
  const offer = useBandcampOffer(artist, title)
  const download = useDownloadPurchase()
  const me = useMe()
  const data = offer.data
  if (!data) return null

  if (data.owned && data.purchase) {
    const id = data.purchase
    return (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-t px-5 py-3 sm:px-6">
        <p className="flex min-w-0 flex-1 items-center gap-2 text-[14px]">
          <BandcampGlyph className="text-[#1da0c3]" />
          <span className="truncate">Bought on Bandcamp</span>
        </p>
        {download.isSuccess ? (
          <Button variant="ghost" size="sm" nativeButton={false} render={<Link to="/downloads" />}>
            <CircleCheck /> Downloading
          </Button>
        ) : (
          me.permissions.download && (
            <Button variant="outline" size="sm" disabled={download.isPending} onClick={() => download.mutate({ id })}>
              {download.isPending ? <LoaderCircle className="animate-spin" /> : <ArrowDownToLine />}
              Get the files
            </Button>
          )
        )}
      </div>
    )
  }

  const price =
    data.price === null
      ? null
      : data.name_your_price
        ? data.price > 0
          ? `${formatPrice(data.price, data.currency)} or more`
          : "Name your price"
        : formatPrice(data.price, data.currency)
  return (
    <a
      href={data.url}
      target="_blank"
      rel="noreferrer"
      className="group flex items-center gap-3 border-t px-5 py-3 text-[14px] outline-none hover:bg-accent/50 focus-visible:bg-accent/50 sm:px-6"
    >
      <BandcampGlyph className="text-[#1da0c3]" />
      <span className="min-w-0 flex-1 truncate">
        Buy on Bandcamp
        {price && <span className="text-muted-foreground"> · {price}</span>}
      </span>
      <ArrowUpRight className="size-4 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5" />
    </a>
  )
}

/**
 * Settings: link your own Bandcamp account. One paste (or a cookies.txt file) and
 * delune lists what you've bought and can fetch it for you.
 */
export function BandcampSettings({ onOpenPurchases }: { onOpenPurchases: () => void }) {
  const account = useBandcampAccount()
  const link = useLinkBandcamp()
  const unlink = useUnlinkBandcamp()
  const sync = useSyncBandcamp()
  const [pasted, setPasted] = useState("")
  const [help, setHelp] = useState(false)
  const file = useRef<HTMLInputElement>(null)
  const a = account.data

  if (account.isPending) return <div className="h-40 animate-pulse rounded-2xl border bg-card/40" />

  if (a?.linked) {
    return (
      <div className="overflow-hidden rounded-2xl border bg-card/50">
        <div className="flex flex-wrap items-center gap-4 px-5 py-5">
          <span className="flex size-12 shrink-0 items-center justify-center rounded-2xl bg-[#1da0c3]/15 text-[#1da0c3]">
            <BandcampGlyph className="size-6" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="truncate text-[16.5px] font-semibold">{a.name ?? a.username}</p>
            <p className="mt-0.5 text-[13.5px] text-muted-foreground">
              {a.syncing
                ? "Fetching your purchases…"
                : `${a.purchases.toLocaleString()} ${a.purchases === 1 ? "purchase" : "purchases"}${
                    a.synced_at ? ` · checked ${ago(a.synced_at)}` : ""
                  }`}
            </p>
          </div>
          <Button onClick={onOpenPurchases} disabled={!a.purchases} className="w-full sm:w-auto">
            See purchases
          </Button>
        </div>
        {a.problem && (
          <p className="flex items-start gap-2 border-t bg-destructive/8 px-5 py-3 text-[13.5px]">
            <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" /> {a.problem}
          </p>
        )}
        <div className="flex items-center justify-between gap-2 border-t px-3 py-2">
          <Button variant="ghost" size="sm" disabled={a.syncing || sync.isPending} onClick={() => sync.mutate()}>
            <RefreshCw className={cn(a.syncing && "animate-spin")} /> Check again
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="text-muted-foreground hover:text-destructive"
            disabled={unlink.isPending}
            onClick={() => unlink.mutate()}
          >
            Unlink
          </Button>
        </div>
      </div>
    )
  }

  const submit = (value: string) => {
    if (value.trim()) link.mutate(value.trim(), { onSuccess: () => setPasted("") })
  }

  return (
    <div className="overflow-hidden rounded-2xl border bg-card/50">
      <div className="px-5 pt-5">
        <p className="flex items-center gap-2 text-[16px] font-semibold">
          <BandcampGlyph className="size-5 text-[#1da0c3]" /> Link your Bandcamp
        </p>
        <p className="mt-1 max-w-[60ch] text-[14px] text-muted-foreground">
          See what you've bought, spot albums you already own, and let delune fetch the files for review. Your login
          stays on this server and is only ever sent to Bandcamp.
        </p>
      </div>
      <form
        className="flex flex-col gap-2 px-5 pt-4 sm:flex-row"
        onSubmit={(e) => {
          e.preventDefault()
          submit(pasted)
        }}
      >
        <input
          value={pasted}
          onChange={(e) => setPasted(e.target.value)}
          placeholder="Paste your identity cookie"
          type="password"
          autoComplete="off"
          spellCheck={false}
          className="h-11 w-full min-w-0 rounded-xl border bg-background/60 px-3.5 text-[15px] outline-none focus:border-primary/50 sm:flex-1"
        />
        <Button type="submit" size="lg" className="h-11 rounded-xl px-5" disabled={!pasted.trim() || link.isPending}>
          {link.isPending ? <LoaderCircle className="animate-spin" /> : null} Link
        </Button>
      </form>
      {link.isError && <p className="px-5 pt-2 text-[13.5px] text-destructive">{link.error.message}</p>}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-5 pt-3 pb-4 text-[13.5px]">
        <button
          type="button"
          className="flex items-center gap-1.5 text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
          onClick={() => file.current?.click()}
        >
          <FileUp className="size-3.5" /> Use a cookies.txt file
        </button>
        <input
          ref={file}
          type="file"
          accept=".txt,text/plain"
          className="hidden"
          onChange={async (e) => {
            const chosen = e.target.files?.[0]
            if (chosen) submit(await chosen.text())
            e.target.value = ""
          }}
        />
        <button
          type="button"
          aria-expanded={help}
          className="text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
          onClick={() => setHelp((v) => !v)}
        >
          Where do I find it?
        </button>
      </div>
      {help && (
        <ol className="list-decimal space-y-1.5 border-t bg-background/30 py-4 pr-5 pl-10 text-[13.5px] text-muted-foreground">
          <li>
            Sign in at{" "}
            <a
              href="https://bandcamp.com"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-0.5 text-foreground underline underline-offset-4"
            >
              bandcamp.com <ExternalLink className="size-3" />
            </a>{" "}
            on a computer.
          </li>
          <li>
            Open the developer tools (<kbd className="font-sans">F12</kbd>), then Application (Chrome) or Storage
            (Firefox), then Cookies.
          </li>
          <li>
            Copy the value of the cookie called <code className="text-foreground">identity</code> and paste it above.
          </li>
          <li>Or export bandcamp.com's cookies with a cookies.txt extension and choose that file.</li>
        </ol>
      )}
    </div>
  )
}

/** Opens your Bandcamp purchases from the search page, once an account is linked. */
export function PurchasesButton() {
  const account = useBandcampAccount()
  const navigate = useNavigate()
  if (!account.data?.linked) return null
  return (
    <Button
      variant="outline"
      onClick={() => void navigate({ to: "/", search: (old: { q?: string }) => ({ ...old, purchases: true }) })}
    >
      <BandcampGlyph className="text-[#1da0c3]" /> Purchases
    </Button>
  )
}

/** Settings: the Bandcamp card, with the purchases sheet it opens. */
export function BandcampGroup() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <BandcampSettings onOpenPurchases={() => setOpen(true)} />
      <PurchasesSheet open={open} onClose={() => setOpen(false)} />
    </>
  )
}

/** Everything bought on Bandcamp, each ready to fetch for review. */
export function PurchasesSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const account = useBandcampAccount()
  const purchases = useBandcampPurchases(open && !!account.data?.linked)
  const [format, setFormat] = useState<string>("flac")
  const [filter, setFilter] = useState("")
  const list = (purchases.data ?? []).filter((p) =>
    `${p.artist} ${p.title}`.toLowerCase().includes(filter.trim().toLowerCase()),
  )

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup className="fixed inset-x-0 bottom-0 z-50 flex h-[92dvh] flex-col overflow-hidden rounded-t-3xl border-t bg-card outline-none transition-transform duration-200 data-ending-style:translate-y-full data-starting-style:translate-y-full sm:inset-0 sm:m-auto sm:h-[min(88dvh,820px)] sm:w-[min(94vw,680px)] sm:rounded-3xl sm:border sm:shadow-[0_40px_120px_-20px_rgb(0_0_0/0.8)] sm:data-ending-style:translate-y-0 sm:data-starting-style:translate-y-0">
          <header className="border-b p-5 pb-4 sm:p-6 sm:pb-4">
            <div className="flex items-center gap-3">
              <BandcampGlyph className="size-5 text-[#1da0c3]" />
              <Dialog.Title className="type-title flex-1 text-[22px]">Bought on Bandcamp</Dialog.Title>
              <Dialog.Close
                aria-label="Close"
                className="flex size-9 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                <X className="size-4" />
              </Dialog.Close>
            </div>
            <p className="mt-1 text-[14px] text-muted-foreground">
              Fetch anything you've bought; it arrives in Review like any other download.
            </p>
            <div className="mt-4 flex flex-wrap items-center gap-2">
              <input
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                placeholder="Filter purchases"
                className="h-10 min-w-0 flex-1 basis-40 rounded-xl border bg-background/60 px-3.5 text-[15px] outline-none focus:border-primary/50"
              />
              <div role="radiogroup" aria-label="Format" className="flex rounded-xl border bg-background/40 p-0.5">
                {FORMATS.map((f) => (
                  <button
                    key={f.id}
                    type="button"
                    role="radio"
                    aria-checked={format === f.id}
                    onClick={() => setFormat(f.id)}
                    className={cn(
                      "rounded-[10px] px-2.5 py-1.5 text-[12.5px] whitespace-nowrap text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      format === f.id && "bg-accent text-foreground",
                    )}
                  >
                    {f.label}
                  </button>
                ))}
              </div>
            </div>
          </header>
          <div className="min-h-0 flex-1 overflow-y-auto p-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] sm:p-3">
            {account.data && !account.data.linked ? (
              <div className="px-4 py-10 text-center text-[14.5px] text-muted-foreground">
                <p>Link your Bandcamp account to see what you've bought.</p>
                <Button
                  className="mt-4"
                  nativeButton={false}
                  render={<Link to="/settings/$section" params={{ section: "bandcamp" }} onClick={onClose} />}
                >
                  Link Bandcamp
                </Button>
              </div>
            ) : purchases.isError ? (
              <div className="px-4 py-10 text-center text-[14.5px]">
                <p className="text-destructive">{purchases.error.message}</p>
                <Button variant="outline" className="mt-4" onClick={() => void purchases.refetch()}>
                  Try again
                </Button>
              </div>
            ) : purchases.isPending || account.isPending ? (
              <div className="space-y-2 p-3">
                {[0, 1, 2, 3].map((i) => (
                  <div key={i} className="h-16 animate-pulse rounded-xl bg-muted/40" />
                ))}
              </div>
            ) : list.length ? (
              <ul>
                {list.map((p) => (
                  <PurchaseRow key={p.id} purchase={p} format={format} />
                ))}
              </ul>
            ) : (
              <p className="px-4 py-10 text-center text-[14.5px] text-muted-foreground">
                {filter ? `Nothing you've bought matches “${filter}”.` : "No purchases yet."}
              </p>
            )}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function PurchaseRow({ purchase: p, format }: { purchase: BandcampPurchase; format: string }) {
  const me = useMe()
  const download = useDownloadPurchase()
  const downloads = useDownloads()
  const job = (downloads.data ?? []).find((j) => j.id === p.job)
  const state = job
    ? job.status === "downloading" || job.status === "queued"
      ? "Downloading"
      : job.status === "imported"
        ? "In your library"
        : job.status === "failed" || job.status === "cancelled"
          ? null
          : "Ready for review"
    : null

  return (
    <li className="flex items-center gap-3 rounded-xl px-2.5 py-2 hover:bg-accent/40">
      <Cover src={p.art} alt="" className="size-14 shrink-0 rounded-lg" />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px]">{p.title}</p>
        <p className="truncate text-[13px] text-muted-foreground">
          {p.artist}
          {p.purchased_at ? ` · ${new Date(p.purchased_at * 1000).getFullYear()}` : ""}
        </p>
      </div>
      {state ? (
        <Button
          variant="ghost"
          size="sm"
          nativeButton={false}
          render={<Link to={state === "Downloading" ? "/downloads" : "/review"} />}
        >
          {state === "Downloading" ? <LoaderCircle className="animate-spin" /> : <CircleCheck />} {state}
        </Button>
      ) : p.downloadable && me.permissions.download ? (
        <Button
          variant="outline"
          size="sm"
          disabled={download.isPending}
          onClick={() => download.mutate({ id: p.id, format })}
        >
          {download.isPending ? <LoaderCircle className="animate-spin" /> : <ArrowDownToLine />}
          {job?.status === "failed" ? "Try again" : "Get"}
        </Button>
      ) : p.url ? (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Open on Bandcamp"
          nativeButton={false}
          render={<a href={p.url} target="_blank" rel="noreferrer" />}
        >
          <ArrowUpRight />
        </Button>
      ) : null}
    </li>
  )
}
