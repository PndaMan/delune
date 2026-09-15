# 0006 — Accounts come from Navidrome

- **Status:** Accepted
- **Date:** 2026-09-15

## Context

delune is shared by the people who already share a Navidrome library. They each
have a Navidrome account, and one of them runs the server. A second set of
usernames and passwords would be one more thing to set up, forget and leak.

People also need different rights. The person who runs the server decides what goes
into the library; a friend might be trusted to search and download, but not to
import without a look from an admin.

## Decision

1. **Sign in with Navidrome.** delune asks Navidrome's Subsonic API (`getUser`)
   whether a username and password work and whether the person is an admin. It never
   stores the password.
2. **delune issues its own sessions.** A random 256-bit token, sent to browsers as an
   `HttpOnly`, `SameSite=Lax` cookie and to the TUI as a bearer token. Only its
   SHA-256 hash is saved, in `accounts.json` (mode 0600). Sessions end after 30 days
   unused, or on sign-out.
3. **Navidrome admins are delune admins,** re-read at every sign-in, and can do
   everything. Everyone else starts with search and download, and an admin can change
   that per person: search, download, skip approval, manage.
4. **Approval is a switch.** Everyone reviews their own downloads (ADR 0005). With
   "Imports need an admin's approval" on, only admins and people allowed to skip
   approval can move a download into the library.
5. **Downloads belong to whoever started them.** People see their own; managers see
   everyone's.
6. **No Navidrome, no accounts.** Without a Navidrome to ask, delune runs in open
   mode: whoever can reach it is the admin, and the server logs a warning at startup.

## Consequences

- Nothing to set up beyond pointing delune at Navidrome, and removing someone from
  Navidrome stops them signing in (existing sessions last until they expire or an
  admin clears them; a revoke button is planned).
- Sign-in depends on Navidrome being up. delune's own sessions keep working while
  it's down.
- Five wrong passwords for one username pause sign-in for that name for a minute,
  so delune can't be used to guess Navidrome passwords quickly.
- Open mode must never be exposed to the internet; the docs say so.
