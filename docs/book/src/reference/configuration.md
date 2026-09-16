# Configuration

delune reads its settings from three places. **Command-line flags and environment
variables win**; `config.toml` fills in the rest; everything else lives in delune's
database and is changed from the web app. Connections set by flags or variables show
as fixed in Settings → Connections.

## `delune serve`

| Flag | Environment variable | Default | |
|---|---|---|---|
| `--bind` | `DELUNE_BIND` | `0.0.0.0:7474` | Address for the web app and API |
| `--data-dir` | `DELUNE_DATA_DIR` | `delune-data` | Where delune keeps its data ([what's in it](data-folder.md)) |
| `--library-dir` | `DELUNE_LIBRARY_DIR` | — | The music folder Navidrome scans; imports go here |
| `--naming-template` | `DELUNE_NAMING_TEMPLATE` | `{album_artist}/[{year} - ]{album}/{track} - {title}` | Until naming is saved in the web app |
| `--slsk-username` | `DELUNE_SLSK_USERNAME` | — | Soulseek account (with the password) |
| `--slsk-password` | `DELUNE_SLSK_PASSWORD` | — | |
| `--slsk-port` | `DELUNE_SLSK_PORT` | `2234` | Port other Soulseek users connect to |
| `--navidrome-url` | `DELUNE_NAVIDROME_URL` | — | Turns on sign-in, library checks and rescans (with the next two) |
| `--navidrome-username` | `DELUNE_NAVIDROME_USERNAME` | — | A Navidrome **admin** |
| `--navidrome-password` | `DELUNE_NAVIDROME_PASSWORD` | — | |

| Environment variable | |
|---|---|
| `DELUNE_LOG` | Log filter, e.g. `debug` or `info,delune_soulseek=debug` (default `info`) |

Prefer environment variables (or `config.toml`) for passwords: flags end up in shell
history and process listings.

## `config.toml`

Written by `delune setup` and by Settings → Connections, in the data folder, readable
only by its owner:

```toml
library_dir = "/srv/music"

[soulseek]
username = "your-name"
password = "…"
port = 2234

[navidrome]
url = "http://127.0.0.1:4533"
username = "delune"
password = "…"
```

Every part is optional. Changing connections in the web app saves this file and
restarts delune in place.

## `delune setup`

| Flag | Environment variable | |
|---|---|---|
| `--data-dir` | `DELUNE_DATA_DIR` | Where the configuration goes (asked for too) |

## `delune-tui`

| Argument / flag | Environment variable | |
|---|---|---|
| `SERVER` | `DELUNE_SERVER` | delune's address, its host, or Navidrome's address |
| `--username` | `DELUNE_USERNAME` | Navidrome username |
| `--password` | `DELUNE_PASSWORD` | Navidrome password (prefer the prompt) |
| `--forget` | | Forget the saved server and session |

## Settings in the web app

Everything else is set in the web app and saved in the database: appearance, naming and
import options, automation, sharing and speed limits, people and permissions, the fetch
command, Bandcamp links, follows and favourites.
