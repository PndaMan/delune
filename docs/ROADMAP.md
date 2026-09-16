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
- [x] TUI vertical slice: search, download for review, downloads (stop, resume, remove), review and import
- [x] Configuration file and first-run setup (Navidrome URL, music folder, Soulseek account), checked before saving
- [x] SQLite persistence (settings, jobs, peer history), importing the JSON files from before
- [x] Login with Navidrome credentials; sessions
- [x] TypeScript types generated from the API types, checked against the web UI at compile time and in CI
- [x] OpenAPI description of the routes, served at `/api/v1/openapi.json` and checked for completeness

**Soulseek**
- [x] Wire primitives, server messages, compressed search responses
- [x] Server session: login, keepalive, reconnect with backoff
- [x] Listening port, peer connections, firewall piercing
- [x] Search with rate limiting; results streamed to clients (API + TUI)
- [x] Transfers: queueing, download, resume, retry, cancel
- [x] NAT traversal help: UPnP port forwarding (opt-in), and whether the port is actually reachable
- [x] Share index of the music folder; answering searches; uploads with slot limits

**Finding the right release**
- [x] Link parsing for 9 services
- [x] Link resolution from each service's public metadata (Deezer, iTunes, Spotify embed, oEmbed, JSON-LD, MusicBrainz), cached
- [x] Cross-service matching by UPC and ISRC through MusicBrainz (exact name as a fallback), without holding up the search
- [x] Library check against Navidrome
- [x] Folder grouping and quality/availability ranking
- [x] Tracklist matching against the resolved release
- [x] Automatic retry with fewer words when the network returns nothing (noise, filler words, then for links the title alone)

**Downloads**
- [x] Download jobs: one folder, every file queued at once, staged under the data directory
- [x] Persist jobs across restarts

**Getting it into the library**
- [x] Naming template engine
- [x] Layout detection from an existing library; naming settings saved from the web UI
- [x] Decode verification and fake-FLAC detection
- [x] Tags (lofty), embedded artwork, synced lyrics (LRCLIB)
- [x] Review inbox
- [x] Admin approval gate
- [x] Import with conflict checks, then a Navidrome scan

**Shipping**
- [x] Docker image (multi-arch) and compose example
- [x] NixOS module and flake, with a VM test
- [x] Release binaries for Linux and macOS
- [~] AUR and Homebrew packages: PKGBUILD, formula and systemd unit written; publishing them waits for the first release

## Next: web UI overhaul

- [x] Complete visual redesign of the web UI (identity, typography, colour, motion)
- [x] Live search results with artwork, release view, settings with naming editor
- [x] Downloads screen with live progress; ready jobs listed in Review
- [x] Review screen: verification results, planned paths, import and discard

## Requested next (from Aidan, 2026-09-15)

**Accounts and requests: "Overseerr for Soulseek and streaming"**
- [x] Sign in with Navidrome credentials; sessions; sign out (web and TUI)
- [x] Roles and permissions: admin (from Navidrome admin), and per-person rights to search,
      download, skip approval and manage; an "imports need approval" switch
- [x] Revoke sessions: your devices, sign out elsewhere; admins sign someone out everywhere
- [x] Other sources: a downloader of your own that delune runs for a link (opt-in, admins only).
      delune has no built-in streaming downloads: those need getting around copy protection
      and terms, so they're out (decided with Aidan, 2026-09-16).
- [x] Requests: people ask for an album; admins or permitted users approve; request status
      visible to the requester (pending, approved, downloading, in library, declined)
- [x] Profile menu at the bottom of the rail (avatar, role, connection status, sign out)
      replacing the moon indicator
- [x] Per-user history and notifications (requests, reviews ready, failed downloads, imports)

**Soulseek: everything slskd does, simpler (before any streaming service)**
- [x] Share the music library: index, answer searches, upload with slot and speed limits (opt-in)
- [x] Uploads page: who's downloading from you, queue, speeds, cancel, block, per-user limits, refuse people who share nothing
- [x] Browse a user's shared folders; download any folder from there
- [x] User profiles: info, picture, shared counts, free slots, speed
- [x] Private messages and chat rooms (for people who manage delune; rooms rejoin on reconnect)
- [x] Wishlist: saved searches re-run on a schedule, with auto-download (for review) when a good enough match appears
- [x] Search filters (words and -exclusions, format, quality tier, free slot) and hidden people
- [x] Transfer controls: stop, resume, retry elsewhere, global upload and download speed limits
- [x] Speed schedule: different upload and download limits between two times of day
- [x] Limit albums downloading at once; the rest wait in line and can be moved to the front
- [x] Distributed search network: joins so shared files are found, and (opt-in) relays searches to child clients
- [x] Statistics: shared, running, transferred (kept across restarts)

**Links**
- [x] Resolve pasted Spotify, Apple Music, YouTube Music, Deezer, Tidal, Qobuz, SoundCloud, Bandcamp and MusicBrainz links to a release, then search Soulseek for it
- [x] Rank folders by how much of the linked tracklist they hold; track links download just that track
- [x] Playlist links from Spotify and Deezer: pick songs, add them (or their albums) to the wishlist, searched straight away

**Profiles and look**
- [x] Profile pictures: upload your own (rail, profile menu, people list); Soulseek users' pictures on their pages
- [x] Themes: System, Night, Blue hour, Midnight (OLED), Forest (green); six accents; saved to each account

**Mobile**
- [x] Phone layouts for search results, filters, release view (sticky action), downloads
- [x] Phone layouts for review (sticky import bar) and settings (section links)
- [x] Installable web app (manifest, icons, offline shell)

**Search and release view**
- [x] Release view fits the screen with no scrollbars (adaptive, multi-column tracklist)
- [x] Results show "Downloading" / "In review" / "Imported" when that folder or album is already in progress
- [x] "In library" and "N missing" from Navidrome, per-track marks in the release view, a warning before downloading an album you already have
- [x] Cover overlays: greyed with a progress ring while downloading, tinted once in the library

## After v0.1

- Settings screens with full naming, lyrics, artwork and transcoding options
- Cover-art accent colours; theme presets; shared design tokens for web and TUI
- Playlist import and watchlist
- [x] Followed artists and automatic quality upgrades (off by default)
- Opt-in streaming providers: Qobuz, Tidal, Deezer, YouTube Music, SoundCloud, Bandcamp
- Album art in the TUI (kitty, sixel, iTerm2)

## Ideas (2026-09-16)

Proposed after an app-wide review; not yet scheduled.

**Listening and discovery**
- Listen before you import: play any track from Review (and from a peer's folder) in the browser.
- Release radar: a calendar of upcoming and new releases from followed artists.
- Complete an artist: see which albums the library is missing and want them all in one tap.
- Recommendations from what people actually play (Navidrome play counts, ListenBrainz).
- Keep playlists in sync: a Spotify, Deezer or YouTube playlist that's re-checked for new songs.

**Library care**
- Library health: duplicates, lossy albums, missing artwork or lyrics, odd tags — each with a fix.
- Quality ladder: upgrade an album automatically when a better copy turns up, with a spectrogram in review.
- Tagging against MusicBrainz releases, choosing the edition, before import.

**Staying in touch**
- Push notifications for the installed app (Web Push), plus ntfy, Discord and email.
- An activity log: who asked for, approved and imported what.
- A diagnostics page: Soulseek, Navidrome, port reachability and VPN address, with what to do about each.

**Polish**
- A command palette (⌘K / long-press) and keyboard shortcuts across the app.
- Request comments, and per-person request limits.
- Backup and restore of delune's database from Settings.
- SOCKS5 proxy support for Soulseek, for VPNs that aren't a container or namespace.
- Several libraries (Navidrome music folders) with a choice at import.
