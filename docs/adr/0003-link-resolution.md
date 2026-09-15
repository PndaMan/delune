# 0003 — Resolve links through MusicBrainz, UPC and ISRC

- **Status:** Accepted
- **Date:** 2026-09-15

## Context

Users paste links from any service and expect delune to understand which release
they mean. Most tools have used Odesli (song.link) for this. Its public API now
returns `401 PUBLIC_API_ACCESS_DEPRECATED` and was shut down on 2026-07-31.

Spotify's Web API has been heavily restricted since February 2026 (Premium-owner
apps, five users, reduced search), so it can't be a dependency either.

## Decision

1. **Parse offline first.** `delune_resolve::link::parse` turns URLs into
   `(provider, kind, id)` without network access.
2. **MusicBrainz URL relationships** map provider URLs to MusicBrainz releases where
   the community has linked them.
3. **UPC and ISRC bridge the rest.** Fetch the barcode or ISRC from a service that
   exposes it without authentication (Deezer, iTunes lookup), then look it up
   elsewhere (MusicBrainz, Deezer `upc:`/`isrc:`).
4. **Public page metadata** is the last resort for services without open APIs
   (for example Spotify's embed page for title, artists and duration).
5. **Cache every identifier we learn**, so each release is resolved once.

The canonical identity of an album is its MusicBrainz release group; a specific
edition and tracklist is a MusicBrainz release.

## Consequences

- No dependency on a third-party aggregator that can disappear.
- MusicBrainz rate limits (1 request per second) mean resolution needs a queue and
  a cache.
- Obscure releases missing from MusicBrainz fall back to text search, which the UI
  must make clear.
