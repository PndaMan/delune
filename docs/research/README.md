# Research summary (2026-09-15)

Summary of the research behind delune's design, done before the first line of code.
Detailed working notes stay out of the repository; this page keeps the findings that
shaped decisions. Decisions themselves are recorded in [../adr/](../adr/).

## 1. What antra is (and isn't)

- **Link resolver, not search.** Text queries fail (`Not a recognised Spotify URL`). 6 providers: Spotify, Apple Music, Amazon, Tidal, Qobuz, Deezer.
- Formats: FLAC 24/192, FLAC 16, ALAC, AAC 256, MP3 320.
- Downloads come from author's **private relay servers** + streaming mirrors. Soulseek exists in desktop source only (via slskd), not prioritized.
- License **Elastic License 2.0** — source-available, forbids hosted-service use. **Do not copy code.** Ideas only.
- Stack: desktop = Go/Wails + Svelte 3 (7.8k-line `App.svelte`) + Python engine. Web = private FastAPI + vanilla JS.

### Worth taking (as ideas)
- **Settings-as-schema**: each setting one declarative object (visibility/disabled rules, local vs server scope); same schema powers Ctrl+K settings search.
- **Clickable template tokens** with live filename preview.
- **Theme engine**: each theme = a few hue/chroma parameters + lightness ramp → ~200 derived CSS vars. Contrast stays consistent. "Artwork" theme derives hue from cover.
- Command palette (Ctrl+K), availability checker, resolver scoring with a minimum-confidence floor (0.60) and penalties for radio edits/clean versions.

### Where to beat it
- No text search; Soulseek not first; mobile toolbar overflows; giant single files; multi-disc token bugs; inconsistent defaults; `{bitrate}`=`{quality}`.

### antra settings (web) — parity checklist

| Group | Key | antra default | Ours |
|---|---|---|---|
| Naming | single track template | `{artist} - {title}` | ✅ |
| Naming | album track template | `{track} - {title}` | ✅ |
| Naming | folder structure | `{album_artist}/{year} - {album}/` | ✅ |
| Naming | tokens | title, artist, album_artist, album, year, track, disc, genre, composer, isrc, codec, bitrate, quality | ✅ + `bit_depth`, `sample_rate`, `mbid`, `label`, `catalog`, `source` |
| Formatting | multi-disc handling | `prefix` | ✅ (fixed semantics) |
| Formatting | track number padding | 2 | ✅ |
| Formatting | illegal char replacement | `_` | ✅ |
| Formatting | whitespace handling | preserve | ✅ |
| Formatting | conflict behavior | skip | ✅ + `upgrade-if-better` |
| Lyrics | fetch lyrics | on | ✅ |
| Lyrics | mode | embedded | ✅ embedded / `.lrc` sidecar / both (Navidrome prefers sidecar) |
| Artwork | source | max | ✅ |
| Artwork | max size / quality | 0 / 92 | ✅ |
| Artwork | embed / save cover file | on / off | ✅ |
| Transcode | MP3 bitrate | 320 | ✅ + V0 |
| Transcode | AAC bitrate | 256 | ✅ |
| Matching | prefer explicit | on | ✅ |
| Matching | cross-platform match / 16-bit fallback | off | ✅ (always on for Soulseek) |
| Sidecars | m3u / cue / nfo / json | off | ✅ |
| Theming | 11 themes: antra, nebula, abyss, verdant, ember, rose, solar, crimson, slate, void, artwork | — | ✅ own theme set, same engine idea |

## 2. Ecosystem facts that shape design

1. **Odesli/song.link API is dead** (401 `PUBLIC_API_ACCESS_DEPRECATED`, shutdown 2026-07-31). → Own resolver: MusicBrainz URL lookup → ISRC/UPC bridge (Deezer `isrc:`/`upc:`, MB `/isrc/`, iTunes `lookup?upc=`) → Spotify embed page scrape for metadata.
2. **Spotify Web API heavily restricted since Feb 2026** (Premium owner, 5 users, search limit 10). Don't depend on it.
3. **slskd, not native Soulseek, for v1.** slskd 0.26 has batch downloads w/ client job ID, auto-retry, per-job destination. Native Rust client (`soulseek-rs-lib`) possible later behind a trait.
4. **Soulseek search bans** (30-min) on burst — rate limit ~34 searches / 220s (sockseek default).
5. **Navidrome has no upload API** (verified v0.64 source). Files must land in its music folder: same host, shared mount, or a tiny agent. Then `startScan` (admin, supports targeted path) + poll `getScanStatus`.
6. **Navidrome `search3` matches MBIDs** → "Already in library" / "Upgrade available" (songs expose suffix, bitrate, bitDepth, samplingRate).
7. Subsonic auth = md5(password+salt) token; no API-key extension → store password encrypted.

### Ranking (summary)
1. Hard filters (format allowlist, min bitrate, banned users, file count sanity).
2. Album: map files → tracklist by title/number/duration against every release in the release group.
3. Sort key: match confidence → completeness → quality tier (24/192 > 24/96 > 24/48 > 24/44.1 > 16/44.1 > MP3 320 > V0; infer from size×duration when missing) → mixed-quality penalty → availability (free slot, speed, queue, historical peer reliability).
4. Patience: abort + fall through if remote-queued/slow too long.
5. Auto-grab only when confident; otherwise hold for review.
6. Post-download verification: decode check (symphonia), spectral fake-FLAC check, tag/art/lyrics, then review → import.

### Prior art
sockseek/sldl (best ranking), soularr, **Soulbeet** (Rust, slskd+beets+Navidrome — closest), DroppedNeedle (confidence-gated auto vs review), Lidarr.Plugin.Slskd (edition-fit rejection), seakarr (peer reliability memory).

## 3. Stack recommendation

- **Core:** Rust — `axum` API + SSE events, `tokio`, `sqlx` (SQLite), `utoipa` OpenAPI.
- **TUI:** `ratatui` + `ratatui-image` (kitty/sixel/iTerm album art) — same binary: `delune serve`, `delune tui`.
- **Web:** React 19 + Vite + TanStack Router/Query/Table/Virtual, shadcn/ui (Base UI), cmdk, sonner, Lucide, Inter/Geist + Geist Mono. Embedded into binary via `memory-serve`. TS client generated from OpenAPI (`@hey-api/openapi-ts`).
- **Media:** `lofty` (tags), `symphonia` (decode/verify), `ffmpeg-sidecar` (transcode), `musicbrainz_rs`, `rusty-chromaprint`, `ebur128`.
- **Design tokens:** one DTCG token file → Style Dictionary → CSS vars + Rust theme module, so TUI and web themes match.

## 4. Proposed flow

```
paste link / type query
  → resolve to canonical (MB release-group / recording, ISRC/UPC)
  → check Navidrome: owned? upgrade available?
  → Soulseek search (always first), results stream in live, ranked
  → [only if user enabled] streaming providers
  → pick best (default) or choose format/source
  → download to staging → verify (decode, spectral, track count)
  → tag + art + lyrics per settings → REVIEW screen (diff, tracklist, quality)
  → approve → move into Navidrome library → targeted startScan
```

## 5. Name candidates

| Name | Status |
|---|---|
| **delune** | clean on GitHub, crates.io, npm |
| sourdine | clean (mute on an instrument) |
| sillage | clean GH/crates, npm taken |
| lunette | mostly clean (dead 207★ mac player) |
| berceuse | clean, hard to spell |
| sostenuto | clean, long |
| aubade / gloaming / moonrake | soft collisions |
| nocturne / fermata / cadenza | **avoid** (existing music apps) |
