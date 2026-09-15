import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api, type Candidate, type CandidateFile, type DownloadJob, type JobFile } from "@/lib/api"
import { plural } from "@/lib/format"

/** All download jobs, polled while anything is still moving. */
export function useDownloads() {
  return useQuery({
    queryKey: ["downloads"],
    queryFn: ({ signal }) => api.downloads(signal),
    refetchInterval: (query) =>
      query.state.data?.some((job) => job.status === "queued" || job.status === "downloading") ? 1_000 : 5_000,
  })
}

export function useStartDownload() {
  const client = useQueryClient()
  return useMutation({
    /** `files` narrows the download, e.g. to one track; by default it's the whole folder. */
    mutationFn: ({ candidate, files }: { candidate: Candidate; files?: CandidateFile[] }) =>
      api.startDownload({
        username: candidate.username,
        folder: candidate.folder,
        title: candidate.title,
        parent: candidate.parent,
        // Audio plus artwork, cue sheets and logs: everything a review might need.
        files: (files ?? candidate.files).map((f) => ({ path: f.path, size: f.size })),
      }),
    onSuccess: () => client.invalidateQueries({ queryKey: ["downloads"] }),
  })
}

export function useRemoveDownload() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api.removeDownload(id),
    onSuccess: () => client.invalidateQueries({ queryKey: ["downloads"] }),
  })
}

/** The job downloading this exact folder from this person, if any. Cancelled jobs don't count. */
export function jobForCandidate(jobs: DownloadJob[], candidate: Candidate): DownloadJob | undefined {
  return jobs.find(
    (job) => job.username === candidate.username && job.folder === candidate.folder && job.status !== "cancelled",
  )
}

const normalise = (s: string | null | undefined) =>
  (s ?? "")
    .toLowerCase()
    .replace(/[([{][^)\]}]*[)\]}]/g, "")
    .replace(/[^a-z0-9]/g, "")

/** A job for the same album from anyone, matched loosely by title and artist folder. */
export function jobForAlbum(jobs: DownloadJob[], candidate: Candidate): DownloadJob | undefined {
  const title = normalise(candidate.title)
  if (title.length < 3) return undefined
  return jobs.find((job) => job.status !== "cancelled" && normalise(job.title).includes(title))
}

/** One sentence describing where a job is, from the reader's point of view. */
const AUDIO_FILE = /\.(flac|alac|wav|aiff?|mp3|m4a|aac|opus|ogg|oga|wv|ape|dsf)$/i

export function describeJob(job: DownloadJob): string {
  const tracks = job.files.filter((f) => AUDIO_FILE.test(f.name))
  const done = tracks.filter((f) => f.status === "done").length
  const active = job.files.find((f) => !["waiting", "done", "failed", "cancelled"].includes(f.status))
  switch (job.status) {
    case "ready":
      return job.review === "checking" || job.review === "waiting"
        ? "All files arrived. Checking them now"
        : `All ${plural(job.files.length, "file")} arrived. Ready for review`
    case "cancelled":
      return "Cancelled"
    case "imported":
      return "Imported into your library"
    case "failed": {
      const failed = job.files.filter((f) => f.status === "failed")
      return `${plural(failed.length, "file")} couldn't be downloaded${failed[0]?.error ? `: ${failed[0].error}` : ""}`
    }
    case "downloading":
      return `Downloading track ${Math.min(done + 1, tracks.length)} of ${tracks.length} from ${job.username}`
    case "queued":
      return describeWaiting(job.username, active)
  }
}

function describeWaiting(username: string, file: JobFile | undefined): string {
  if (!file || file.status === "connecting" || file.status === "waiting") return `Connecting to ${username}`
  if (file.status === "queued") {
    return file.place_in_queue
      ? `Waiting in ${username}'s queue, number ${file.place_in_queue}`
      : `Waiting in ${username}'s queue`
  }
  return `${username} is about to start sending`
}
