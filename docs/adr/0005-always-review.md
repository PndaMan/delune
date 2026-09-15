# 0005 — Every import is reviewed

- **Status:** Accepted
- **Date:** 2026-09-15

## Context

Automated music tools fail quietly: wrong editions, missing tracks, bad tags, fake
lossless files. Once those land in a shared library they're hard to notice and
tedious to clean up. delune is used by small groups (friends and family) sharing one
Navidrome library, where one person's mistake affects everyone.

## Decision

- **Nothing moves into the library without explicit approval.** There is no
  auto-import mode in v1.
- **The requester reviews their own downloads.** The review screen shows tracklist
  matching, quality per file, verification results, final tags, artwork, lyrics and
  resulting file paths.
- **Admins can require a second approval.** When enabled, approved jobs wait in the
  admin inbox as well.
- **Navidrome accounts are delune accounts.** Navidrome admins are delune admins, so
  there's no separate user management.

## Consequences

- Imports are slower than fully automatic tools, by design.
- The review screen is the most important screen in the product and must make
  problems obvious at a glance, in both the web UI and the TUI.
- Automation features (watchlist, followed artists, quality upgrades) produce review
  items, not imports, so they stay safe to leave running.
