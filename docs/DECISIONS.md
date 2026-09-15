# delune — product decisions (2026-09-15)

Decided with Aidan after research (see [research/README.md](research/README.md)). Each will become a proper ADR in `docs/adr/` when implementation starts.

## Identity
- **Name:** `delune` (after *Clair de Lune*). Binary `delune`; `delune serve`, `delune tui`.
- **Repo:** `github.com/PndaMan/delune`, public, **AGPL-3.0**.

## Shape
- **Runs on the same host as Navidrome.** Server owns the library folder, Soulseek client, queue, and review inbox. Web UI and TUI are thin clients of the server API. Goal: replace slskd + antra with one more automated tool.
- **Works with any Navidrome instance** (generic self-hosting, not tied to one homelab).
- **Stack:** Rust core (axum, tokio, sqlx/SQLite, utoipa OpenAPI) · ratatui + ratatui-image TUI · React 19 + Vite + TanStack + shadcn/ui (Base UI) web, embedded in the binary. Shared DTCG design tokens → CSS vars + Rust theme.

## Sources
- **Soulseek is always searched first.** Native Soulseek protocol client **in Rust** (no slskd dependency). Must be a well-behaved client: shares the Navidrome library read-only, configurable upload slots/speed, search rate limiting to avoid server bans.
- **Streaming providers** exist as opt-in only (off by default, used only when the user enables them): Qobuz, Tidal, Deezer, YouTube Music, SoundCloud, Bandcamp. Implemented behind a provider trait.
- **Link resolution:** own resolver (Odesli is dead) — MusicBrainz URL lookup → ISRC/UPC bridge → provider metadata.

## Flow
- Search text **or** paste any provider link → canonical release → "already in library / upgrade available" via Navidrome → Soulseek results streamed live and ranked → optional streaming sources → default highest quality, user may pick format.
- **Always review.** Nothing enters the library without approval.
- **Approvals:** requester reviews their own download; admins can turn on a required admin approval gate.
- **Fake-FLAC detection:** flagged in review with spectrogram; next-best source suggested.

## Automation (v1)
| Feature | Default |
|---|---|
| Playlist import (Spotify/Apple/Deezer links → missing tracks) | on |
| Watchlist (keep retrying not-found items) | on |
| Artist follow + new releases | **off**, configurable |
| Quality upgrades (find better versions of owned albums) | **off**, configurable |

## Accounts
- **Log in with Navidrome credentials.** Navidrome admins = delune admins. Friends/family multi-user.

## Library
- **Default layout: detect and match the existing library's folder/file naming**, with very detailed settings to change it (template tokens, live preview, multi-disc, padding, illegal chars, conflicts). Full antra settings parity (lyrics synced/embedded/sidecar, artwork, MP3/AAC bitrate, sidecars, theming).

## Design
- Linear/Raycast-dense, keyboard-first, ⌘K palette; cover art sets accent color (Plexamp-style). Designed empty/loading/error states. Mobile-width correct.

## Distribution
- Docker image + compose · NixOS module + flake · static binaries on GitHub Releases · AUR + Homebrew (TUI client).

## v0.1 milestone — vertical slice
Paste link / search → Soulseek (native) → rank → download → verify → review → import → targeted Navidrome scan, working in **both** web and TUI.
