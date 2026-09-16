# NixOS

delune is a flake with a NixOS module. It runs delune as a hardened systemd service,
reads secrets from a file outside the store, and can route everything through a VPN.

```nix
{
  inputs.delune.url = "github:PndaMan/delune";

  outputs = { nixpkgs, delune, ... }: {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        delune.nixosModules.default
        ({ config, ... }: {
          services.delune = {
            enable = true;
            libraryDir = "/srv/music";        # the folder Navidrome scans
            group = "navidrome";              # so delune can write there
            soulseek.username = "your-name";
            navidrome.url = "http://127.0.0.1:4533";
            navidrome.username = "delune";    # a Navidrome admin
            environmentFile = "/run/secrets/delune.env";
            openFirewall = true;              # the Soulseek port
          };
        })
      ];
    };
  };
}
```

The environment file holds the passwords:

```sh
DELUNE_SLSK_PASSWORD=…
DELUNE_NAVIDROME_PASSWORD=…
```

With [sops-nix](https://github.com/Mic92/sops-nix), render it from secrets:

```nix
sops.templates."delune.env".content = ''
  DELUNE_SLSK_PASSWORD=${config.sops.placeholder."soulseek-password"}
  DELUNE_NAVIDROME_PASSWORD=${config.sops.placeholder."navidrome-delune-password"}
'';
services.delune.environmentFile = config.sops.templates."delune.env".path;
```

The service listens on `127.0.0.1:7474`; put your reverse proxy in front of it (see
[Reverse proxy and HTTPS](../connect/reverse-proxy.md)).

## Keep delune's own nixpkgs

Don't make `delune` follow an older `nixpkgs` (a stable release, or an older unstable
pin). delune needs a recent Rust and Bun, and the web dependencies are pinned by a
hash that matches the Bun in delune's own lock file. Following something older fails
with `Unknown lockfile version` or a hash mismatch.

```nix
inputs.delune.url = "github:PndaMan/delune";   # no inputs.nixpkgs.follows
```

## Through a VPN

See [Through a VPN](../connect/vpn.md) for `services.delune.vpn.container` (gluetun
and friends) and `services.delune.vpn.namespace`.

## Just the programs

```sh
nix run github:PndaMan/delune -- serve        # the server
nix profile install github:PndaMan/delune     # delune and delune-tui on your PATH
```

All options are listed in [NixOS module options](../reference/nixos-options.md).

## Updating

```sh
nix flake lock --update-input delune
sudo nixos-rebuild switch --flake .#your-host
```

### Updating on its own, with rollback

`nixosModules.autodeploy` keeps a machine on the latest delune (and on the latest of
its own config repo). Every couple of minutes it looks for a new commit. Once that
commit's GitHub checks pass, it builds and switches, then checks the machine is
healthy. If anything is wrong, it switches back to the system that was running.

```nix
imports = [ inputs.delune.nixosModules.autodeploy ];

services.autodeploy = {
  enable = true;
  repo = "/etc/nixos";                # your flake, as a git checkout
  inputs = [ "delune" ];              # inputs to follow
  units = [ "delune.service" ];       # must be running afterwards
  checks = [ "curl -fsS http://127.0.0.1:7474/api/v1/health" ];
  ntfyUrlFile = "/run/secrets/autodeploy-ntfy";   # optional: hear about it
};
```

A deploy counts as healthy when all of these hold:

- every unit in `units` is active;
- no unit has failed that wasn't already failed before the switch;
- every command in `checks` succeeds.

It checks six times, 20 seconds apart (`healthTries`, `settleSeconds`). If the
switch fails, or the checks still fail after the last try, it switches back.

- **Pushes to your config repo** are pulled when its checkout is clean. With local
  changes it deploys the tree as it stands and leaves pulling to you.
- **A commit that failed** is recorded in `/var/lib/autodeploy/bad` and isn't tried
  again. Whether it failed its checks, the build, or the health checks, the next push
  is tried as normal. Delete a line to retry that commit.
- **The lock file** is restored after a failure. After a success it's committed
  (only `flake.lock`); set `push = true` to push that commit too.
- **A deploy interrupted** by a crash or a reboot is checked on the next run, and kept
  or rolled back the same way.
- **The previous generations** stay in the boot menu, as with any switch.

```sh
systemctl start autodeploy          # look now
journalctl -u autodeploy -f         # what it did
```
