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

