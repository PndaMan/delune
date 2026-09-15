<div align="center">

# delune

**Find music, check it, and file it into Navidrome. Soulseek first.**

A self-hosted server with a web UI and a terminal UI. Paste a link from any
service or type what you're looking for; delune finds the best copy on Soulseek,
lets you review it, and puts it in your library with the tags, artwork, lyrics and
file names you asked for.

[![CI](https://github.com/PndaMan/delune/actions/workflows/ci.yml/badge.svg)](https://github.com/PndaMan/delune/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-4f5bd5)](LICENSE)

<img src="docs/assets/results.png" alt="delune search results for Pink Floyd's The Dark Side of the Moon: album covers, colour-coded quality from FLAC 24/192 down, track counts, sizes and whether each person can send now" width="820">

</div>

> [!WARNING]
> **delune is in early development and can't download anything yet.** Live
> Soulseek search works in the web UI, the terminal UI and the API; the first working
> release (v0.1) is tracked in the [roadmap](docs/ROADMAP.md). Star or watch the
> repo to follow along.

<table>
  <tr>
    <td width="50%"><img src="docs/assets/search.png" alt="The search screen: tonight's moon above a large search field"></td>
    <td width="50%"><img src="docs/assets/release.png" alt="A release opened: large album cover, quality, availability and the full tracklist"></td>
  </tr>
  <tr>
    <td align="center">Search, under tonight's actual moon phase</td>
    <td align="center">Every release opens with artwork and a clean tracklist</td>
  </tr>
</table>

## Why

Getting an album into a self-hosted library usually means juggling a Soulseek
client, a link converter, a tagger and a file manager, then waiting for a scan.
delune is one tool for the whole path, built around a few rules:

- **Soulseek is always searched first.** Streaming services are only used if you
  switch them on.
- **Nothing reaches your library without review.** You see the tracklist,
  quality, artwork and any problems before you approve an import.
- **Your library's conventions win.** delune learns how your existing folders are
  named and follows them; every detail is adjustable.
- **It's a good Soulseek citizen.** It shares your library, rate-limits searches and
  respects other people's upload queues.

## How it works

```mermaid
flowchart LR
    A[Paste a link<br>or type a search] --> B[Resolve the release<br>MusicBrainz · UPC · ISRC]
    B --> C{Already in<br>Navidrome?}
    C -- no, or a better copy exists --> D[Search Soulseek]
    D -. only if enabled .-> E[Streaming sources]
    D --> F[Rank by match,<br>completeness, quality]
    E --> F
    F --> G[Download to staging<br>and verify]
    G --> H[Tag · artwork · lyrics]
    H --> I[You review]
    I -- approve --> J[Move into library<br>and rescan Navidrome]
```

The server runs on the same machine as Navidrome, because Navidrome has no upload
API: delune writes into the music folder and then asks Navidrome to rescan just
that folder. The web UI and the TUI are both clients of the server's HTTP API, so
you can run `delune tui` from your laptop.

## Features

**Find**
- Search Soulseek, or paste a link from Spotify, Apple Music, Tidal, Qobuz, Deezer,
  YouTube Music, SoundCloud, Bandcamp or MusicBrainz. Links resolve to the release,
  and results rank by how much of its tracklist each folder holds.
- Paste a Spotify or Deezer playlist to put its songs, or their albums, on the wishlist.
- Results ranked lossless first, complete before partial, then resolution and
  availability, with artwork, "in library" and "N missing" badges, and filters.
- A wishlist that keeps searching and downloads good copies for review.
  Automation is off by default: following artists and upgrading lossy albums.

**Soulseek, natively** (no slskd)
- Searching, downloads with queue position, resume and retry, stop and resume.
- Browse anyone's shares and profile; any folder opens like a search result.
- Private messages and chat rooms.
- Share your library (opt-in): uploads with slots, speed limits, per-person queues,
  blocking, a leecher policy, and the distributed search network, so people find you.
- Stats, with byte totals kept across restarts.

**Check and import**
- Every download waits for review. Each file is decoded, and transcodes posing as
  FLAC are flagged.
- Naming templates with live preview, matched to your library; imports land in the
  Navidrome folder and trigger a scan.
- Synced lyrics from LRCLIB (a sidecar `.lrc`, in the tags, or both) and embedded cover
  art on every import.

**People**
- Sign in with Navidrome accounts; Navidrome admins are admins.
- Per-person permissions and an optional admin approval step.
- Profile pictures, themes (Night, Blue hour, Midnight, Forest) and accents.

**Everywhere**
- A web app that installs as a PWA and is designed for phones, not just shrunk to fit.
- A TUI that does the whole flow: search, download, review, import.
- Runs as one Rust binary; a NixOS module and a Docker image are included.

Planned: transcoding options, and release binaries and packages. Streaming sources come last, and only if you opt in.

## Quick start (development)

You need Rust 1.90+ and [Bun](https://bun.sh).

```sh
git clone https://github.com/PndaMan/delune && cd delune

# Web UI (rebuild whenever you want it embedded in the binary)
(cd web && bun install && bun run build)

# Server on http://localhost:7474
cargo run -- serve

# In another terminal
cargo run -- tui
```

To search, give the server a Soulseek account (a new username is registered the
first time it logs in):

```sh
DELUNE_SLSK_USERNAME=you DELUNE_SLSK_PASSWORD=secret cargo run -- serve
```

For web development, `scripts/dev.sh` runs the API server and the hot-reloading
web UI together; open http://localhost:5173.

### Accounts

Point delune at Navidrome and everyone signs in with their Navidrome account;
Navidrome admins are delune admins ([ADR 0006](docs/adr/0006-navidrome-accounts.md)):

```sh
DELUNE_NAVIDROME_URL=http://localhost:4533 \
DELUNE_NAVIDROME_USERNAME=admin DELUNE_NAVIDROME_PASSWORD=secret \
cargo run -- serve
```

Without Navidrome, delune runs in **open mode**: no sign-in, and whoever can reach
it is an admin. Keep an open-mode server off the internet.

## Install

However you install it, open delune in a browser the first time: it asks for your
music folder, Navidrome and a Soulseek account, checks each one, and saves them to
`config.toml` in its data folder (readable only by delune). Flags and `DELUNE_*`
environment variables still work and take precedence; settings made that way show
as fixed in the web UI.

### NixOS

delune is a flake with a NixOS module:

```nix
{
  inputs.delune.url = "github:PndaMan/delune";

  outputs = { nixpkgs, delune, ... }: {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      modules = [
        delune.nixosModules.default
        {
          services.delune = {
            enable = true;
            libraryDir = "/srv/music";               # the folder Navidrome scans
            group = "navidrome";                     # so it can write there
            soulseek.username = "your-name";
            navidrome.url = "http://127.0.0.1:4533";
            navidrome.username = "admin";
            environmentFile = "/run/secrets/delune.env"; # DELUNE_SLSK_PASSWORD, DELUNE_NAVIDROME_PASSWORD
            openFirewall = true;                     # the Soulseek port
          };
        }
      ];
    };
  };
}
```

The service listens on `127.0.0.1:7474`; put your reverse proxy in front of it.
`nix build github:PndaMan/delune` builds just the binary.

### Docker

```sh
cp .env.example .env   # optional: or set everything up in the browser
docker compose up -d
```

[`compose.yaml`](compose.yaml) mounts your music folder at `/music` and keeps
delune's own data in a volume.

### Arch Linux

[`packaging/aur/PKGBUILD`](packaging/aur/PKGBUILD) builds delune with a hardened
systemd service:

```sh
sudoedit /etc/delune/delune.env   # library folder, Soulseek and Navidrome details
sudo systemctl enable --now delune
```

The service can write to `/srv/music`. For a library elsewhere, `systemctl edit delune`
and add `ReadWritePaths=/path/to/music`.

### Homebrew and release binaries

Tagged releases publish binaries for Linux and macOS (x86_64 and ARM) on the
[releases page](https://github.com/PndaMan/delune/releases), and
[`packaging/homebrew/delune.rb`](packaging/homebrew/delune.rb) installs them
(`brew services start delune` runs it in the background). On other Linux systems,
copy the binary to `/usr/bin` and use the unit in [`packaging/systemd/`](packaging/systemd/).

## Project layout

```text
crates/
  delune            the binary: `delune serve`, `delune tui`
  delune-core       domain types and pure logic: quality, providers, releases
  delune-soulseek   native Soulseek protocol client
  delune-resolve    link parsing and cross-service release matching
  delune-navidrome  Subsonic API client for Navidrome
  delune-library    naming templates, tagging, import into the library
  delune-server     HTTP API and embedded web UI
  delune-tui        ratatui terminal client
web/                React + TypeScript web UI (shadcn/ui on Base UI)
docs/               architecture, decisions (ADRs), research, roadmap
```

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit, and
[docs/adr/](docs/adr/) for why things are the way they are.

## Contributing

Issues and pull requests are welcome; start with
[CONTRIBUTING.md](CONTRIBUTING.md). Security problems go through
[SECURITY.md](SECURITY.md), not public issues.

## A note on use

delune is a tool for managing *your* music library. Soulseek is a file-sharing
network: share what you're comfortable sharing and respect the network's rules.
Streaming integrations are off by default, and using them may break those
services' terms. You're responsible for what you download and where you live.

## License

[AGPL-3.0-or-later](LICENSE). If you run a modified delune as a service for other
people, you must offer them your source.
