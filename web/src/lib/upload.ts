import type { DownloadJob, FinishUpload, UploadReceived, UploadSession } from "@/lib/api.generated"

// Uploads go up in pieces. Proxies in front of delune refuse large requests
// (Cloudflare stops at 100 MB) and cut off slow ones (Traefik after a minute), so no
// piece is big or slow, and a piece that fails is sent again from where it stopped.

const MB = 1024 * 1024
const MIN_PIECE = 1 * MB
const FIRST_PIECE = 8 * MB
/** Each piece aims to take about this long. */
const PIECE_SECONDS = 10
const RETRIES = 8

export class UploadError extends Error {}

type Piece = { status: number; body: string }

function send(url: string, data: Blob, onSent: (bytes: number) => void, signal: AbortSignal): Promise<Piece> {
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest()
    request.open("PUT", url)
    request.setRequestHeader("Content-Type", "application/octet-stream")
    request.timeout = 5 * 60_000
    request.upload.onprogress = (e) => onSent(e.loaded)
    request.onload = () => resolve({ status: request.status, body: request.responseText })
    request.onerror = () => resolve({ status: 0, body: "" })
    request.ontimeout = () => resolve({ status: 0, body: "" })
    const abort = () => {
      request.abort()
      reject(new DOMException("Stopped", "AbortError"))
    }
    if (signal.aborted) return abort()
    signal.addEventListener("abort", abort, { once: true })
    request.send(data)
  })
}

const wait = (ms: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    const timer = window.setTimeout(resolve, ms)
    signal.addEventListener("abort", () => {
      window.clearTimeout(timer)
      reject(new DOMException("Stopped", "AbortError"))
    })
  })

function receivedIn(body: string): number | undefined {
  try {
    const received = (JSON.parse(body) as Partial<UploadReceived>).received
    return typeof received === "number" ? received : undefined
  } catch {
    return undefined
  }
}

function messageOf(body: string, fallback: string) {
  try {
    return (JSON.parse(body) as { message?: string }).message ?? fallback
  } catch {
    return fallback
  }
}

async function json<T>(response: Response, fallback: string): Promise<T> {
  const text = await response.text()
  if (!response.ok) throw new UploadError(messageOf(text, fallback))
  return JSON.parse(text) as T
}

/**
 * Send `files` to delune and turn them into a download. `onProgress` gets the share
 * sent so far, from 0 to 1.
 */
export async function uploadFiles(
  files: File[],
  details: FinishUpload,
  onProgress: (share: number) => void,
  signal: AbortSignal,
): Promise<DownloadJob> {
  const session = await json<UploadSession>(
    await fetch("/api/v1/uploads/sessions", { method: "POST", signal }),
    "The upload couldn't start.",
  )
  const total = Math.max(
    1,
    files.reduce((n, f) => n + f.size, 0),
  )
  let done = 0
  let piece = Math.min(FIRST_PIECE, session.max_chunk)

  try {
    for (const [index, file] of files.entries()) {
      let offset = 0
      let failures = 0
      // An empty file still goes up once, so the server knows of it.
      do {
        const size = Math.min(piece, file.size - offset)
        const url =
          `/api/v1/uploads/sessions/${session.id}/files/${index}` +
          `?name=${encodeURIComponent(file.webkitRelativePath || file.name)}&offset=${offset}`
        const started = performance.now()
        const result = await send(
          url,
          file.slice(offset, offset + size),
          (sent) => onProgress((done + sent) / total),
          signal,
        )
        const received = result.status === 200 || result.status === 409 ? receivedIn(result.body) : undefined
        if (received !== undefined) {
          // 409: the server has a different amount than we thought; carry on from there.
          if (result.status === 200) {
            // Size the next piece to take about PIECE_SECONDS on this connection.
            const perSecond = size / Math.max(0.2, (performance.now() - started) / 1000)
            piece = Math.max(MIN_PIECE, Math.min(session.max_chunk, Math.round(perSecond * PIECE_SECONDS)))
            failures = 0
          }
          done += received - offset
          offset = received
          onProgress(done / total)
          if (file.size === 0) break
          continue
        }
        if ([400, 401, 403, 404, 413].includes(result.status)) {
          throw new UploadError(messageOf(result.body, "The upload was refused."))
        }
        // Dropped connection, a proxy's error, a timeout, or the last try still arriving:
        // smaller pieces, and try again.
        failures += 1
        if (failures > RETRIES) {
          throw new UploadError("The connection keeps dropping. Check it and try again.")
        }
        piece = Math.max(MIN_PIECE, Math.floor(piece / 2))
        await wait(Math.min(30_000, 1000 * 2 ** (failures - 1)), signal)
      } while (offset < file.size)
    }

    onProgress(1)
    return await json<DownloadJob>(
      await fetch(`/api/v1/uploads/sessions/${session.id}/finish`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(details),
        signal,
      }),
      "The upload couldn't be finished.",
    )
  } catch (error) {
    void fetch(`/api/v1/uploads/sessions/${session.id}`, {
      method: "DELETE",
    }).catch(() => {})
    throw error
  }
}
