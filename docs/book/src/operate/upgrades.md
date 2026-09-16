# Upgrades and backups

## Upgrading

| Installed with | Upgrade with |
|---|---|
| Install script | Run it again. It skips a version that's already installed and restarts your user service. |
| NixOS | `nix flake lock --update-input delune`, then `nixos-rebuild switch` |
| Docker Compose | `git pull && docker compose up -d --build` |
| Arch / Homebrew | your package manager |
| Source | `git pull`, rebuild the web app and the binaries, restart |

The database upgrades itself on start. Downloads in progress pick up where they left
off; a fetch (command or Bandcamp) that was running is marked failed, to be fetched
again.

## Backups

Back up the [data folder](../reference/data-folder.md). With restic, borg or similar,
include `delune.db*` and `config.toml`; `staging/` is optional.

Your music is in Navidrome's folder, not delune's, and your listening history is in
Navidrome's database.

## Logs

| | |
|---|---|
| systemd (user) | `journalctl --user -u delune -f` |
| systemd (system) | `journalctl -u delune -f` |
| NixOS container mode | `podman logs -f delune` |
| Docker | `docker compose logs -f delune` |

`DELUNE_LOG=debug` (or `info,delune_soulseek=debug`) shows more.

## Health

`GET /api/v1/health` answers without sign-in:

```sh
curl -s http://127.0.0.1:7474/api/v1/health
# {"name":"delune","version":"0.1.0","status":"ok"}
```
