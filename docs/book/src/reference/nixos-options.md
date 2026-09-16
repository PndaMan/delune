# NixOS module options

All options live under `services.delune`.

| Option | Type | Default | |
|---|---|---|---|
| `enable` | bool | `false` | Run delune |
| `package` | package | the flake's `delune` | |
| `listen` | string | `"127.0.0.1:7474"` | Address for the web app and API. Use `0.0.0.0:<port>` in container mode. |
| `dataDir` | path | `"/var/lib/delune"` | Downloads under review, accounts and settings |
| `libraryDir` | null or path | `null` | The music folder Navidrome scans |
| `namingTemplate` | null or string | `null` | Import naming, until saved in the web app |
| `user` | string | `"delune"` | User for the service (created if `delune`) |
| `group` | string | `"delune"` | Group for the service; use Navidrome's to write to its folder |
| `soulseek.username` | null or string | `null` | Soulseek account |
| `soulseek.port` | port | `2234` | Soulseek listening port |
| `navidrome.url` | null or string | `null` | Navidrome's address (set with `navidrome.username`) |
| `navidrome.username` | null or string | `null` | A Navidrome admin |
| `environmentFile` | null or path | `null` | File with `DELUNE_SLSK_PASSWORD=` and `DELUNE_NAVIDROME_PASSWORD=` (any `DELUNE_*` works) |
| `openFirewall` | bool | `false` | Open `soulseek.port` |
| `vpn.container` | null or string | `null` | Run delune as a container in this container's network (e.g. `"gluetun"`) |
| `vpn.namespace` | null or string | `null` | Run delune in `/run/netns/<name>` |
| `vpn.user` | string | `"0:0"` | `uid:gid` in container mode; match the music folder's owner |
| `vpn.volumes` | list of strings | `[ ]` | Extra mounts in container mode |

## What it sets up

- **Normal mode:** a `delune` systemd service, sandboxed (read-only system, no new
  privileges, a system-call filter), with write access only to `dataDir` and
  `libraryDir`, restarting on failure.
- **`vpn.namespace`:** the same service with `NetworkNamespacePath`.
- **`vpn.container`:** an `oci-containers` container built from the package (with CA
  certificates), started after the VPN container, mounting `dataDir` and `libraryDir`
  (with `rslave`), waiting for `libraryDir`'s mount and restarting every 30 s until it
  works.

## `services.autodeploy`

From `nixosModules.autodeploy`; see [Updating on its own](../install/nixos.md#updating-on-its-own-with-rollback).

| Option | Type | Default | |
|---|---|---|---|
| `enable` | bool | `false` | Deploy this machine's flake when it changes, and roll back if it breaks |
| `repo` | string | | The flake checkout on this machine |
| `host` | string | `networking.hostName` | The `nixosConfigurations` entry to deploy |
| `inputs` | list of strings | `[ ]` | `github:` inputs to follow |
| `pull` | bool | `true` | Fast-forward the repo to its upstream when it's clean |
| `requireChecks` | bool | `true` | Wait for a commit's GitHub checks to pass |
| `githubTokenFile` | null or string | `null` | Token for reading checks (private repos, rate limits) |
| `interval` | string | `"2min"` | How often to look |
| `units` | list of strings | `[ ]` | Units that must be active after a deploy |
| `checks` | list of strings | `[ ]` | Commands that must succeed after a deploy |
| `settleSeconds` | positive int | `20` | Wait between health checks |
| `healthTries` | positive int | `6` | Checks before rolling back |
| `commitLock` | bool | `true` | Commit `flake.lock` after a good deploy |
| `push` | bool | `false` | Push that commit |
| `ntfyUrlFile` | null or string | `null` | File with an ntfy topic URL for deploy and rollback messages |
