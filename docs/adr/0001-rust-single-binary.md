# 0001 — Rust, one binary, running beside Navidrome

- **Status:** Accepted
- **Date:** 2026-09-15

## Context

delune needs a long-running server (Soulseek connections, downloads, audio
decoding, file moves), a web UI and a terminal UI. It's self-hosted by people with
modest hardware, often on a NAS or small VM, and installed through Docker, NixOS,
plain binaries or package managers.

Navidrome, the target library server, has no upload endpoint (verified against the
v0.64 source). Files can only enter a library by being written into its music folder.

## Decision

- Write the server and the TUI in **Rust**, in one Cargo workspace, shipped as a
  **single `delune` binary** with the web UI embedded.
- Run delune **on the same host as Navidrome** (or with the music folder mounted),
  and trigger Navidrome rescans through the Subsonic API.
- Build the web UI with React and TypeScript, as a static app served by the binary.

## Consequences

- One artifact to install, update and package; no runtime to manage.
- Low memory use and no garbage-collection pauses during large transfers.
- ratatui gives the best terminal image support for album art.
- The TUI and server share types directly, without a generated client.
- Rust compiles slowly and is harder for casual contributors than Go or TypeScript.
  We offset this with small crates, pure core logic and thorough docs.
- Remote deployments where delune can't reach the music folder aren't supported.
  A small agent on the Navidrome host could be added later if people need it.
