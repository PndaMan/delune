# Data folder

Everything delune keeps is in its data folder (`DELUNE_DATA_DIR`).

| | |
|---|---|
| `delune.db` (+ `-wal`, `-shm`) | SQLite: accounts and sessions, downloads, reviews, wishlist, follows, requests, notifications, settings, chat, favourites and their saved share lists, Bandcamp links, peer history. Readable only by delune. |
| `config.toml` | Connections from `delune setup` or the web app. Readable only by delune. |
| `staging/<job>/` | Downloads waiting for review. Removed after import or discard; finished jobs are forgotten after 60 days and leftover folders are cleared at startup. |
| `avatars/` | Profile pictures |
| `share-cache.json` | The index of what you share, so a restart doesn't rescan the library |

Older versions kept `*.json` files instead of `delune.db`. They're imported the first
time the database opens and renamed to `*.json.imported`.

## Backing up

Back up the whole folder. For a consistent copy of the database while delune runs:

```sh
sqlite3 /var/lib/delune/delune.db ".backup '/backups/delune.db'"
```

`staging/` can be left out: it only holds downloads nobody has reviewed yet.
