# Install script

```sh
curl -fsSL https://raw.githubusercontent.com/PndaMan/delune/main/install.sh | sh
```

`install.sh` is a POSIX shell script (tested under `dash` in CI) that:

1. detects the platform: Linux or macOS, `x86_64` or `aarch64` (and the native
   build under Rosetta);
2. finds the latest release, or the one you ask for;
3. downloads it with retries and checks it against the release's published SHA-256
   (it refuses to install anything that doesn't match);
4. makes sure the programs run on this machine **before** replacing anything;
5. installs `delune` and `delune-tui` by copying next to the target and renaming, so an
   interrupted install never leaves half a program;
6. starts [`delune setup`](../setup-wizard.md) on a first install.

Running it again **upgrades**: it skips a version that's already installed, restarts a
running user service after an upgrade, and leaves your settings alone.

## Options

Pass options after `sh -s --`:

```sh
curl -fsSL https://raw.githubusercontent.com/PndaMan/delune/main/install.sh | sh -s -- --version v0.1.0 --no-setup
```

| Option | Environment variable | What it does |
|---|---|---|
| `--version <tag>` | `DELUNE_VERSION` | Install this release (`v0.1.0` or `0.1.0`). Default: the latest. |
| `--bin-dir <dir>` | `DELUNE_BIN_DIR` | Install here. Default: `~/.local/bin` (or `$XDG_BIN_HOME`), `/usr/local/bin` as root. |
| `--no-setup` | `DELUNE_NO_SETUP=1` | Don't start `delune setup` afterwards. |
| `--force` | `DELUNE_FORCE=1` | Reinstall even if this version is already there. |
| `-y`, `--yes` | `DELUNE_YES=1` | Don't ask questions (use sudo without asking if the folder needs it). |
| `--mirror <url>` | `DELUNE_DOWNLOAD_BASE` | Download the release files from here instead of GitHub. |
| `--uninstall` | | Remove the programs and your user service. Your data is kept. |
| `--dry-run` | | Show what would happen without changing anything. |
| `-h`, `--help` | | Show the options. |

`NO_COLOR` turns colours off.

## Root and sudo

The script never needs root for the default folder. If you choose a folder you can't
write to (like `/usr/local/bin` as a normal user), it asks before using `sudo`, and
only uses it for creating, copying and renaming the programs.

## Reading it first

Piping to a shell runs whatever the URL serves. To look before you run:

```sh
curl -fsSLO https://raw.githubusercontent.com/PndaMan/delune/main/install.sh
less install.sh
sh install.sh
```

## Where things go

| | |
|---|---|
| Programs | `~/.local/bin/delune`, `~/.local/bin/delune-tui` |
| Data | `~/.local/share/delune` (or `/var/lib/delune` as root) |
| Service | `~/.config/systemd/user/delune.service` |
| Client settings | `~/.config/delune/tui.toml` |

If `~/.local/bin` isn't on your `PATH`, the script prints the line to add to your shell's
profile.

## When there's no release yet

Until the first release is tagged, the script says so and points to
[building from source](source.md).
