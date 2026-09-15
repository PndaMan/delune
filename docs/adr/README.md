# Architecture decision records

Each record captures one decision: the context, what we chose, and what it costs.
Records are immutable once accepted; a changed decision gets a new record that
supersedes the old one.

| # | Decision | Status |
|---|---|---|
| [0001](0001-rust-single-binary.md) | Rust, one binary, running beside Navidrome | Accepted |
| [0002](0002-native-soulseek.md) | Speak the Soulseek protocol natively instead of driving slskd | Accepted |
| [0003](0003-link-resolution.md) | Resolve links through MusicBrainz, UPC and ISRC, not Odesli | Accepted |
| [0004](0004-quality-ranking.md) | One ordered quality key; match before quality | Accepted |
| [0005](0005-always-review.md) | Every import is reviewed | Accepted |
| [0006](0006-navidrome-accounts.md) | Accounts come from Navidrome | Accepted |

New record: copy the most recent one, take the next number, and open a pull request.
Background research for these decisions is in [`../research/`](../research/).
