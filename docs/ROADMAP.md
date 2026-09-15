# Roadmap

Plans change; this file tracks the current intent. Product decisions behind it are
in [DECISIONS.md](DECISIONS.md) and [adr/](adr/).

## v0.1 — the whole path, once

Goal: paste a link or search, get an album from Soulseek, review it, and see it in
Navidrome, from both the web UI and the TUI.

**Foundations**
- [x] Workspace, CI, docs, ADRs
- [x] Domain model: quality ranking, providers, Soulseek-first source policy
- [x] Single binary serving API and embedded web UI
- [x] TUI and web UI shells connected to the server
- [ ] Configuration file and first-run setup (Navidrome URL, music folder, Soulseek account)
- [ ] SQLite persistence (settings, jobs, peer history)
- [ ] Login with Navidrome credentials; sessions
- [ ] OpenAPI generation and a generated TypeScript client

**Soulseek**
- [x] Wire primitives, server messages, compressed search responses
- [x] Server session: login, keepalive, reconnect with backoff
- [x] Listening port, peer connections, firewall piercing
- [x] Search with rate limiting; results streamed to clients (API + TUI)
- [x] Transfers: queueing, download, resume, retry, cancel
- [ ] NAT traversal help: UPnP/NAT-PMP port mapping, clear guidance when port 2234 isn't reachable
- [ ] Share index of the music folder; answering searches; uploads with slot limits

**Finding the right release**
- [x] Link parsing for 9 services
- [ ] MusicBrainz, UPC and ISRC resolution with caching
- [ ] Library check against Navidrome
- [x] Folder grouping and quality/availability ranking
- [ ] Tracklist matching against the resolved release
- [ ] Automatic retry with fewer words when the network returns nothing

**Downloads**
- [x] Download jobs: one folder, files in sequence, staged under the data directory
- [ ] Persist jobs across restarts (SQLite)

**Getting it into the library**
- [x] Naming template engine
- [ ] Layout detection from an existing library
- [ ] Decode verification and fake-FLAC detection
- [ ] Tags (lofty), artwork, synced lyrics (LRCLIB)
- [ ] Review inbox, admin approval gate
- [ ] Atomic import and targeted Navidrome scan

**Shipping**
- [ ] Docker image (multi-arch) and compose example
- [ ] NixOS module and flake
- [ ] Release binaries for Linux and macOS
- [ ] AUR and Homebrew packages

## Next: web UI overhaul

- [x] Complete visual redesign of the web UI (identity, typography, colour, motion)
- [x] Live search results with artwork, release view, settings with naming editor
- [x] Downloads screen with live progress; ready jobs listed in Review
- [ ] Review screen actions: approve, reject, import

## After v0.1

- Settings screens with full naming, lyrics, artwork and transcoding options
- Cover-art accent colours; theme presets; shared design tokens for web and TUI
- Playlist import and watchlist
- Followed artists and automatic quality upgrades (off by default)
- Opt-in streaming providers: Qobuz, Tidal, Deezer, YouTube Music, SoundCloud, Bandcamp
- Album art in the TUI (kitty, sixel, iTerm2)
