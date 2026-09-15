import { EmptyState } from "@/components/empty-state"
import { useDownloads } from "@/lib/downloads"
import { JobCard } from "@/pages/downloads-page"

export function ReviewPage() {
  const downloads = useDownloads()
  const ready = (downloads.data ?? []).filter((job) => job.status === "ready")
  return (
    <PageFrame title="Review" wide>
      {ready.length === 0 ? (
        <EmptyState illumination={0.5} title="Nothing waiting for review">
          Every download stops here before it reaches your library. You'll see the tracklist, quality, artwork and file
          names, then approve it or send it back.
        </EmptyState>
      ) : (
        <>
          <p className="mt-2 max-w-[62ch] text-[15px] text-muted-foreground">
            These downloads are complete and staged. Checking, tagging and importing into your library are the next
            steps being built.
          </p>
          <ul className="mt-6 space-y-3 pb-24">
            {ready.map((job) => (
              <JobCard key={job.id} job={job} />
            ))}
          </ul>
        </>
      )}
    </PageFrame>
  )
}

export function PageFrame({ title, children, wide = false }: { title: string; children: React.ReactNode; wide?: boolean }) {
  return (
    <div className={wide ? "mx-auto w-full max-w-[1100px] px-5 sm:px-10" : "mx-auto w-full max-w-[900px] px-5 sm:px-10"}>
      <h1 className="type-display pt-10 pb-2 text-[44px] sm:pt-14">{title}</h1>
      {children}
    </div>
  )
}
