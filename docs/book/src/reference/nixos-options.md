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
