import { Link } from "@tanstack/react-router"

import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"

export function DownloadsPage() {
  return (
    <PageFrame title="Downloads">
      <EmptyState
        illumination={0.25}
        title="Nothing downloading"
        action={<Button render={<Link to="/" search={{}} />}>Find an album</Button>}
      >
        Releases you choose from search will download here, one folder at a time, and move to Review when they've been
        checked. Downloading arrives in the next release.
      </EmptyState>
    </PageFrame>
  )
}

export function ReviewPage() {
  return (
    <PageFrame title="Review">
      <EmptyState illumination={0.5} title="Nothing waiting for review">
        Every download stops here before it reaches your library. You'll see the tracklist, quality, artwork and file
        names, then approve it or send it back.
      </EmptyState>
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
