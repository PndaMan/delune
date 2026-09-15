import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api, type Candidate, type DownloadJob, type JobFile } from "@/lib/api"
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
    mutationFn: (candidate: Candidate) =>
      api.startDownload({
        username: candidate.username,
        folder: candidate.folder,
        title: candidate.title,
        parent: candidate.parent,
        // Audio plus artwork, cue sheets and logs: everything a review might need.
        files: candidate.files.map((f) => ({ path: f.path, size: f.size })),
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

/** One sentence describing where a job is, from the reader's point of view. */
export function describeJob(job: DownloadJob): string {
  const done = job.files.filter((f) => f.status === "done").length
  const active = job.files.find((f) => !["waiting", "done", "failed", "cancelled"].includes(f.status))
  switch (job.status) {
    case "ready":
      return `All ${plural(job.files.length, "file")} arrived. Ready for review`
    case "cancelled":
      return "Cancelled"
    case "failed": {
      const failed = job.files.filter((f) => f.status === "failed")
      return `${plural(failed.length, "file")} couldn't be downloaded${failed[0]?.error ? `: ${failed[0].error}` : ""}`
    }
    case "downloading":
      return `Downloading ${done + 1} of ${job.files.length} from ${job.username}`
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
