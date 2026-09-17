import { Dialog } from "@base-ui/react/dialog"
import { useQueryClient } from "@tanstack/react-query"
import { FileAudio, FolderOpen, FolderUp, LoaderCircle, Plus, Upload, X } from "lucide-react"
import { useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { formatBytes, plural } from "@/lib/format"
import { useMe } from "@/lib/session"
import { toast } from "@/lib/toast"
import { UploadError, uploadFiles } from "@/lib/upload"
import { cn } from "@/lib/utils"

const AUDIO = /\.(flac|alac|wav|aiff?|mp3|m4a|aac|opus|ogg|oga|wv|ape)$/i
const IMAGE = /\.(jpe?g|png|webp)$/i
const ZIP = /\.zip$/i

const usable = (file: File) => AUDIO.test(file.name) || IMAGE.test(file.name) || ZIP.test(file.name)

/** Every file inside a dropped folder. */
async function filesIn(entry: FileSystemEntry): Promise<File[]> {
  if (entry.isFile) {
    return new Promise((resolve) => (entry as FileSystemFileEntry).file((f) => resolve([f]), () => resolve([])))
  }
  if (!entry.isDirectory) return []
  const reader = (entry as FileSystemDirectoryEntry).createReader()
  const all: File[] = []
  // readEntries hands back folders in batches until it returns none.
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve) => reader.readEntries(resolve, () => resolve([])))
    if (!batch.length) break
    for (const child of batch) all.push(...(await filesIn(child)))
  }
  return all
}

/** The "+" on the Downloads page: add tracks, a folder or a zip you already have. */
export function UploadButton() {
  const me = useMe()
  const [open, setOpen] = useState(false)
  if (!me.permissions.download) return null
  return (
    <>
      <Button onClick={() => setOpen(true)} className="rounded-full" aria-label="Add music you have">
        <Plus /> <span className="max-sm:hidden">Add music</span>
      </Button>
      <UploadDialog open={open} onClose={() => setOpen(false)} />
    </>
  )
}

function UploadDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const client = useQueryClient()
  const [files, setFiles] = useState<File[]>([])
  const [title, setTitle] = useState("")
  const [artist, setArtist] = useState("")
  const [follow, setFollow] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [progress, setProgress] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const filesInput = useRef<HTMLInputElement>(null)
  const folderInput = useRef<HTMLInputElement>(null)
  const stopper = useRef<AbortController | null>(null)

  const add = (more: File[]) => {
    const keep = more.filter(usable)
    setFiles((current) => {
      const seen = new Set(current.map((f) => `${f.webkitRelativePath || f.name}:${f.size}`))
      return [...current, ...keep.filter((f) => !seen.has(`${f.webkitRelativePath || f.name}:${f.size}`))]
    })
    // Suggest the album from the folder the tracks came in.
    const folder = keep.find((f) => f.webkitRelativePath)?.webkitRelativePath.split("/").at(-2)
    if (folder && !title) setTitle(folder)
    setError(null)
  }

  const reset = () => {
    setFiles([])
    setTitle("")
    setArtist("")
    setFollow(false)
    setProgress(null)
    setError(null)
  }

  const onDrop = async (event: React.DragEvent) => {
    event.preventDefault()
    setDragging(false)
    const entries = [...event.dataTransfer.items]
      .map((item) => item.webkitGetAsEntry?.())
      .filter((e): e is FileSystemEntry => !!e)
    if (entries.length) {
      const found = (await Promise.all(entries.map(filesIn))).flat()
      const folder = entries.find((e) => e.isDirectory)?.name
      if (folder && !title) setTitle(folder)
      add(found)
    } else {
      add([...event.dataTransfer.files])
    }
  }

  const upload = () => {
    const controller = new AbortController()
    stopper.current = controller
    setProgress(0)
    setError(null)
    uploadFiles(
      files,
      { title: title.trim() || null, artist: artist.trim() || null, follow },
      setProgress,
      controller.signal,
    )
      .then(() => {
        void client.invalidateQueries({ queryKey: ["downloads"] })
        toast(`Checking ${plural(tracks, "track")}; they'll be in Review shortly`)
        reset()
        onClose()
      })
      .catch((e: unknown) => {
        setProgress(null)
        if (e instanceof DOMException && e.name === "AbortError") return
        setError(e instanceof UploadError ? e.message : "The upload didn't work. Check your connection and try again.")
      })
      .finally(() => {
        stopper.current = null
      })
  }

  const stop = () => stopper.current?.abort()

  const tracks = files.filter((f) => AUDIO.test(f.name)).length
  const zips = files.filter((f) => ZIP.test(f.name)).length
  const pictures = files.filter((f) => IMAGE.test(f.name)).length
  const size = files.reduce((n, f) => n + f.size, 0)
  const busy = progress !== null

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) onClose()
      }}
    >
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-[#05060f]/70 backdrop-blur-md transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0" />
        <Dialog.Popup className="fixed inset-x-0 bottom-0 z-50 max-h-[92dvh] overflow-y-auto rounded-t-3xl border bg-card p-5 pb-[max(1.25rem,env(safe-area-inset-bottom))] shadow-2xl outline-none sm:top-1/2 sm:bottom-auto sm:left-1/2 sm:w-[560px] sm:-translate-x-1/2 sm:-translate-y-1/2 sm:rounded-3xl sm:p-6">
          <div className="flex items-start gap-3">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="type-title text-[22px]">Add music you have</Dialog.Title>
              <Dialog.Description className="mt-1 text-[14px] text-muted-foreground">
                Tracks, a folder or a zip. They're checked like any download, named from their tags, joined to the album
                if you already have part of it, and wait in Review.
              </Dialog.Description>
            </div>
            <Dialog.Close
              disabled={busy}
              aria-label="Close"
              className="flex size-9 items-center justify-center rounded-full text-muted-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
            >
              <X className="size-5" />
            </Dialog.Close>
          </div>

          <div
            onDragOver={(e) => {
              e.preventDefault()
              setDragging(true)
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={(e) => void onDrop(e)}
            className={cn(
              "mt-5 flex flex-col items-center gap-3 rounded-2xl border-2 border-dashed px-4 py-7 text-center transition-colors",
              dragging ? "border-primary bg-primary/10" : "border-border",
            )}
          >
            <Upload className="size-7 text-muted-foreground" />
            <p className="text-[14.5px]">
              <span className="max-sm:hidden">Drop files or a folder here, or </span>choose them:
            </p>
            <div className="flex flex-wrap justify-center gap-2">
              <Button variant="outline" onClick={() => filesInput.current?.click()} disabled={busy}>
                <FileAudio /> Tracks or a zip
              </Button>
              <Button variant="outline" onClick={() => folderInput.current?.click()} disabled={busy}>
                <FolderOpen /> A folder
              </Button>
            </div>
            <input
              ref={filesInput}
              type="file"
              multiple
              accept="audio/*,.flac,.alac,.wav,.aif,.aiff,.mp3,.m4a,.aac,.opus,.ogg,.wv,.ape,.zip,image/jpeg,image/png,image/webp"
              className="hidden"
              onChange={(e) => {
                add([...(e.target.files ?? [])])
                e.target.value = ""
              }}
            />
            <input
              ref={folderInput}
              type="file"
              multiple
              // @ts-expect-error: not in React's types, but every current browser supports it.
              webkitdirectory=""
              className="hidden"
              onChange={(e) => {
                add([...(e.target.files ?? [])])
                e.target.value = ""
              }}
            />
          </div>

          {files.length > 0 && (
            <div className="mt-4 space-y-4">
              <div className="flex items-center gap-3 rounded-xl bg-accent/40 px-4 py-3">
                <FolderUp className="size-5 shrink-0 text-primary" />
                <p className="min-w-0 flex-1 text-[14px]">
                  {[
                    tracks && plural(tracks, "track"),
                    zips && plural(zips, "zip"),
                    pictures && plural(pictures, "picture"),
                  ]
                    .filter(Boolean)
                    .join(", ")}{" "}
                  <span className="text-muted-foreground">· {formatBytes(size)}</span>
                </p>
                <Button variant="ghost" size="sm" onClick={() => setFiles([])} disabled={busy}>
                  Clear
                </Button>
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="block">
                  <span className="text-[13px] text-muted-foreground">Album</span>
                  <input
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    placeholder="From the files' tags"
                    className="mt-1 h-10 w-full rounded-lg border bg-background/50 px-3 text-[14px] outline-none focus-visible:border-ring"
                  />
                </label>
                <label className="block">
                  <span className="text-[13px] text-muted-foreground">Artist</span>
                  <input
                    value={artist}
                    onChange={(e) => setArtist(e.target.value)}
                    placeholder="From the files' tags"
                    className="mt-1 h-10 w-full rounded-lg border bg-background/50 px-3 text-[14px] outline-none focus-visible:border-ring"
                  />
                </label>
              </div>
              <label className="flex items-center justify-between gap-3">
                <span>
                  <span className="block text-[14.5px]">Keep this album complete</span>
                  <span className="block text-[13px] text-muted-foreground">
                    Look for any tracks it's missing, now and as the artist adds them.
                  </span>
                </span>
                <Switch checked={follow} onCheckedChange={setFollow} disabled={busy} />
              </label>
            </div>
          )}

          {error && <p className="mt-4 rounded-xl bg-destructive/10 px-4 py-3 text-[14px] text-destructive">{error}</p>}

          <div className="mt-5">
            {busy ? (
              <div>
                <div className="flex items-center justify-between text-[13.5px] text-muted-foreground">
                  <span className="flex items-center gap-2">
                    <LoaderCircle className="size-4 animate-spin" />
                    {progress < 1 ? "Uploading" : "Checking what arrived"}
                  </span>
                  <span className="tabular-nums">{Math.round(progress * 100)}%</span>
                </div>
                <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted">
                  <div className="h-full bg-primary transition-[width]" style={{ width: `${progress * 100}%` }} />
                </div>
                <div className="mt-3 flex items-center justify-between gap-3">
                  <p className="text-[12.5px] text-muted-foreground">
                    Keep this open; a dropped connection picks up again.
                  </p>
                  {progress < 1 && (
                    <Button variant="ghost" size="sm" onClick={stop}>
                      Stop
                    </Button>
                  )}
                </div>
              </div>
            ) : (
              <Button size="lg" className="h-12 w-full rounded-xl" disabled={!files.length} onClick={upload}>
                <Upload /> {files.length ? `Add ${plural(files.length, "file")}` : "Choose something to add"}
              </Button>
            )}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
