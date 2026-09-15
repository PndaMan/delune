# 0002 — Speak the Soulseek protocol natively

- **Status:** Accepted
- **Date:** 2026-09-15

## Context

Most self-hosted Soulseek automation (soularr, sockseek, Soulbeet, Lidarr plugins)
drives [slskd](https://github.com/slskd/slskd) over its REST API. slskd is mature,
and version 0.26 added batch downloads with client-supplied IDs, which would suit us.

But delune's goal is to *replace* the stack people currently run, not add another
service to it. Running slskd means a second container, a second UI people
shouldn't touch, a second config, and API keys passed between them. It also caps
how tightly delune can control search pacing, peer selection and transfer retries.

Writing a Soulseek client carries a real obligation. The network expects clients
to share files, answer searches, honour upload slots and queues, and not hammer the
server with searches: bursts get accounts temporarily banned.

## Decision

Implement the Soulseek protocol in Rust in `delune-soulseek`, as a full,
well-behaved client:

- Share the Navidrome library read-only by default, with configurable upload slots
  and speed limits.
- Answer incoming searches from the share index.
- Rate-limit outgoing searches (starting from sockseek's 34 per 220 seconds) and back
  off on server warnings.
- Build bottom-up and test each layer against fixtures: wire primitives, server
  messages, peer messages, connection state machine, transfers.

## Consequences

- One service to run; delune controls the whole search-to-transfer path.
- Search results can stream straight into both UIs.
- Much more work than calling slskd, and protocol quirks (firewall piercing,
  obfuscation, old clients) must be learned the hard way. The Nicotine+ protocol
  documentation is the primary reference.
- A bug here affects other people on the network, not just our users, so protocol
  changes need tests and careful review.
- If the native client stalls, an slskd-backed implementation of the same search and
  transfer interface remains a fallback.
