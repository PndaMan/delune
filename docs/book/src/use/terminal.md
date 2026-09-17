# Terminal client

`delune-tui` is delune in a terminal, for any machine that can reach the server. It does
the whole flow: search, download, watch progress, review, and import. It marks what you
already have: releases in your library, songs missing from a partial copy, and albums
already downloading or waiting for review. Everything updates live.

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

An `https://` address is never tried as plain `http://`. If the server redirects (say
from `http://` to `https://`), the client remembers where it ended up, since a redirect
would otherwise drop the sign-in.

## Signing in

If the server has sign-in on, it asks for your Navidrome username and password (or reads
`DELUNE_USERNAME` and `DELUNE_PASSWORD`). The password is never stored. The session
token and the server are kept in `~/.config/delune/tui.toml`, readable only by you, so
later starts go straight in. When the session ends (it expired, or you signed out of
that device from the web app), the client asks you to sign in again.

## Screens

- **Search** streams Soulseek releases, best quality first. Each is marked
  `✓ have`, `◐ 6/10` (some of its songs are in your library), `↓ 42%`, `… queued`,
  `● review`, or `↓ elsewhere` (another copy is downloading). Matching artists and albums
  from the music catalogue appear beside the results. Opening a release shows that
  person's files with the album cover: tick the tracks you want and download them, or
  press `i` for the album's tracklist (which can still download that copy). An artist
  opens with their picture and releases.

Covers and pictures are real images in terminals that can show them (kitty, WezTerm,
iTerm2, Ghostty, and others with Sixel support), and coloured blocks elsewhere.
- **Downloads** groups everything by where it's got to: downloading, waiting, needs
  attention, ready for review, and (press `i`) imported. It shows speed and time left.
- **Review** lists finished downloads with a verdict (clean, tracks to check, replaces
  files) and the full report, then the albums imported lately.

Downloading a release you already have asks first. Downloading one you have part of
gets only the missing songs. People without download permission are offered to ask an
admin instead.

## Keys

Press `?` for this list in the client.

| Where | Key | |
|---|---|---|
| Anywhere | `F1` `F2` `F3`, `Alt+1` `Alt+2` `Alt+3` | Search, Downloads, Review |
| | `Ctrl+C` | Quit |
| Outside the search box | `1` `2` `3` | Search, Downloads, Review |
| | `/` | Search box |
| | `?` | Keys |
| | `q` | Quit |
| Search box | type, `Enter` | Search (links work too); the results take the keys |
| | `Ctrl+W`, `Ctrl+U` | Delete a word, clear |
| | `↓` or `Tab` | Into the lists |
| | `Esc` | Clear, then back to the lists (it never quits) |
| Lists | `↑` `↓` (or `k` `j`), `PgUp` `PgDn`, `g` `G` | Move |
| | `Tab`, `←` `→` | Between artists and albums and releases |
| Releases | `Enter` or `o` | Open it: the person's files and the cover |
| | `d` | Download (or ask an admin) |
| | `i` | The album's tracklist |
| | `a` | Open the artist |
| An open release | `Space` | Tick or untick a track |
| | `t` | Tick all, or none |
| | `d` or `Enter` | Download the ticked tracks |
| | `i`, `a`, `s` | Album tracklist, artist, search again |
| | `Esc` | Back |
| Album or artist | `Enter` | Open the selected album |
| | `d` | Download the copy the album was opened from |
| | `s` | Search Soulseek for the album |
| | `a` | Open the album's artist |
| | `Esc` | Back |
| Downloads | `s` or `Space` | Stop or resume |
| | `p` | Download next (when waiting for a turn) |
| | `f` | Find another copy |
| | `x` | Remove (asks first) |
| | `Enter` | Review a finished download |
| | `i` | Show or hide imported albums |
| Review | `i` | Import (asks first) |
| | `x` | Discard (asks first) |
| | `f` | Find another copy |
| | `J` `K` | Scroll the report |
| | `R` | Reload the report |

Confirmations take `y` or `Enter`. Each screen shows its main keys at the bottom.
