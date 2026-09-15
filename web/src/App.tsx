import { useState } from "react"

import { MoonStatus } from "@/components/moon-status"
import { SearchBar } from "@/components/search-bar"
import { SourceLine } from "@/components/source-line"

export default function App() {
  const [submitted, setSubmitted] = useState<string | null>(null)

  return (
    <div className="min-h-dvh">
      <header className="flex h-12 items-center justify-between border-b px-4 sm:px-6">
        <span className="text-[15px] font-semibold tracking-tight">delune</span>
        <MoonStatus />
      </header>

      <main className="mx-auto w-full max-w-180 px-4 pt-[14vh] pb-16 sm:px-6">
        <h1 className="mb-6 text-[28px] leading-tight font-semibold tracking-tight text-balance">
          Find music for your library
        </h1>
        <SearchBar onSubmit={setSubmitted} />
        <div className="mt-4">
          <SourceLine />
        </div>

        <section className="mt-16 border-t pt-6 text-[15px] leading-relaxed text-muted-foreground">
          {submitted ? (
            <p>
              Searching isn't wired up yet. When it is, results for{" "}
              <span className="text-foreground">{submitted}</span> will stream in here from Soulseek, best quality first.
            </p>
          ) : (
            <p className="max-w-[60ch]">
              Paste an album, track or playlist link from Spotify, Apple Music, Tidal, Qobuz, Deezer, YouTube Music,
              SoundCloud or Bandcamp, or type an artist and title. Everything you pick is checked and waits for your
              review before it goes into Navidrome.
            </p>
          )}
        </section>
      </main>
    </div>
  )
}
