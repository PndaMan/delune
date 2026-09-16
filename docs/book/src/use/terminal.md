# Terminal client

`delune-tui` is delune in a terminal, for any machine that can reach the server. It does
the whole flow: search, download, watch progress, review, and import.

```sh
delune-tui                               # the server you used last time
delune-tui myserver                      # finds delune on that host
delune-tui https://delune.example.com
delune-tui --forget                      # forget the server and sign-in
```

## Finding the server

Give it delune's address, just the host name, or your **Navidrome** address. It tries,
in order:

1. exactly what you typed;
2. the same host on delune's port, `7474`;
3. the same host at `/delune` (for a reverse proxy set up that way).

An `https://` address is never tried as plain `http://`.

## Signing in

If the server has sign-in on, it asks for your Navidrome username and password (or reads
`DELUNE_USERNAME` and `DELUNE_PASSWORD`). The password is never stored. The session
token and the server are kept in `~/.config/delune/tui.toml`, readable only by you, so
later starts go straight in until the session expires.

## Keys

| Where | Key | |
|---|---|---|
| Outside the search box | `1` `2` `3` | Search, Downloads, Review |
| | `q` | Quit |
| Anywhere | `Ctrl+C` | Quit |
| Search box | type, `Enter` | Search |
| | `↓` or `Tab` | Into the results |
| | `Esc` | Clear, then quit |
| Results | `↑` `↓` (or `k` `j`), `PgUp` `PgDn`, `g` `G` | Move |
| | `d` | Download this folder |
| | `/`, `Tab` or `Esc` | Back to the search box |
| Downloads | `↑` `↓` | Move |
| | `s` | Stop or resume |
| | `x` | Remove (asks first) |
| | `Enter` or `r` | Review a finished download |
| Review | `i` | Import (asks first) |
| | `x` | Discard (asks first) |

Confirmations take `y` or `Enter`. Each screen shows its keys at the bottom.
