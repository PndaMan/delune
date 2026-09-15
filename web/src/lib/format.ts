export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`
  return `${bytes} B`
}

export function formatSpeed(bytesPerSec: number): string | null {
  if (!bytesPerSec) return null
  if (bytesPerSec >= 1e6) return `${(bytesPerSec / 1e6).toFixed(1)} MB/s`
  return `${Math.round(bytesPerSec / 1e3)} KB/s`
}

/** 3845 → "1 h 04 min", 2100 → "35 min" */
export function formatRuntime(secs: number | null): string | null {
  if (!secs) return null
  const minutes = Math.round(secs / 60)
  if (minutes < 60) return `${minutes} min`
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`
}

/** 284 → "4:44" */
export function formatTrackTime(secs: number | null): string {
  if (!secs) return ""
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`
}

export function plural(n: number, one: string, many = `${one}s`) {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`
}
