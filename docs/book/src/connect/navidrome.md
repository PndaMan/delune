# Navidrome

delune uses Navidrome for three things:

- **Sign-in.** Everyone signs in with their Navidrome account. Navidrome admins are
  delune admins, and that follows Navidrome within about 20 seconds when it changes.
- **Library checks.** Search results and album pages show what you already have.
- **Rescans.** After an import, delune asks Navidrome to scan, and retries for a few
  minutes if Navidrome is down.

## The account delune uses

Give delune its own **admin** account in Navidrome (Settings → Users → add, with
*Is admin* on), for example `delune`. Admin rights are needed to look people up and
start scans. Its password goes in `DELUNE_NAVIDROME_PASSWORD`, `config.toml`, or your
secrets manager — never in the Nix store.

Check it before you point delune at it:

```sh
# Subsonic's token auth: md5(password + salt)
salt=$(head -c6 /dev/urandom | od -An -tx1 | tr -d ' \n')
token=$(printf '%s%s' "$PASSWORD" "$salt" | md5sum | cut -d' ' -f1)
curl -s "http://127.0.0.1:4533/rest/getUser.view?u=delune&t=$token&s=$salt&v=1.16.1&c=check&f=json&username=delune"
# … "status":"ok" … "adminRole":true
```

## Can delune reach it?

`DELUNE_NAVIDROME_URL` must be an address **delune** can reach, which isn't always the
one you use in a browser:

| delune runs… | Navidrome at | Use |
|---|---|---|
| on the same host | `127.0.0.1:4533` | `http://127.0.0.1:4533` |
| in a container, Navidrome on the host | host loopback only | Not reachable. Make Navidrome listen on the container bridge too (see below). |
| in the same Compose project | a service called `navidrome` | `http://navidrome:4533` |
| inside a VPN container's network | host loopback only | The bridge's gateway, e.g. `http://10.88.0.1:4533` (podman) or `http://172.17.0.1:4533` (Docker) |

Inside a container, `127.0.0.1` is the container itself. If Navidrome only listens on
the host's loopback, let it listen more widely and use the firewall to keep it off your
LAN. On NixOS:

```nix
services.navidrome.settings.Address = "0.0.0.0";
networking.firewall.interfaces.podman0.allowedTCPPorts = [ 4533 ];  # the bridge only
```

## The music folder

delune moves approved albums into the folder Navidrome scans (`DELUNE_LIBRARY_DIR`), so
it needs write access there:

- on one machine, run delune in Navidrome's group (the NixOS module's `group` option)
  — files are written group-writable (`UMask=0002`);
- on a NAS share, run delune as the share's owner (`services.delune.vpn.user =
  "1024:100"`, or `user:` in Compose).

Downloads are staged in delune's data folder first. If that's on a different disk
from the library, imports copy and then delete instead of renaming, which is slower
but safe.

## Without Navidrome

With no Navidrome configured, delune runs in **open mode**: no sign-in, and anyone who
can reach it is an admin. The [fetch command](../use/fetch-command.md) is disabled in
open mode. Keep an open-mode delune off the internet.
