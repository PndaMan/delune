# Through a VPN

Soulseek shows the people you trade with your IP address. To keep delune's traffic on a
VPN, run delune inside a network that **only** has the VPN: if the VPN drops, delune
loses the network instead of falling back to your home connection.

There are three ways, from most common to least.

## 1. Inside a VPN container (gluetun)

[gluetun](https://github.com/qdm12/gluetun) runs a VPN client in a container, and other
containers can share its network. delune then:

- sends and receives everything through the VPN;
- is reached through ports that **gluetun** publishes;
- sees `127.0.0.1` as gluetun, not your host (so point it at Navidrome by another address).

### NixOS

```nix
virtualisation.oci-containers.containers.gluetun.ports = [
  "127.0.0.1:7474:7474"   # delune's web app, for your reverse proxy
];

services.delune = {
  enable = true;
  listen = "0.0.0.0:7474";                    # inside gluetun's network
  libraryDir = "/mnt/nas/media/music";
  soulseek.port = 50300;                      # the port your VPN forwards
  navidrome = { url = "http://10.88.0.1:4533"; username = "delune"; };
  environmentFile = config.sops.templates."delune.env".path;
  vpn = {
    container = "gluetun";
    user = "1024:100";                        # owner of the NAS share
  };
};
```

The module builds delune into a container image from the package, runs it with
`--network=container:gluetun`, mounts the data and music folders (the music folder with
`rslave`, so an on-demand NAS mount shows up), and waits for the music folder's mount
before starting.

### Docker Compose

```yaml
services:
  gluetun:
    image: qmcgaw/gluetun
    cap_add: [NET_ADMIN]
    devices: [/dev/net/tun]
    env_file: gluetun.env           # provider, keys, FIREWALL_VPN_INPUT_PORTS=2234
    ports:
      - "7474:7474"
      - "2234:2234"
  delune:
    build: .
    network_mode: service:gluetun
    env_file: .env
    environment:
      DELUNE_DATA_DIR: /data
      DELUNE_LIBRARY_DIR: /music
    volumes:
      - delune-data:/data
      - /path/to/music:/music
```

### The listening port

Other Soulseek users can only reach you if the VPN forwards a port to gluetun, and
gluetun lets it in:

1. Forward a port with your VPN provider (or, on your own VPS running WireGuard, DNAT
   it to the tunnel address:
   `iptables -t nat -A PREROUTING -p tcp --dport 50300 -j DNAT --to 10.100.0.2:50300`).
2. Tell gluetun: `FIREWALL_VPN_INPUT_PORTS=50300`.
3. Set delune's `soulseek.port` / `DELUNE_SLSK_PORT` to the same number.

> **Test from outside the VPN.** A check run from inside gluetun leaves through the
> VPN and comes back to itself, so it can say *closed* when the port is open. Test from
> another network, or trust delune's own *reachable* indicator.

## 2. Inside a network namespace

If you run WireGuard in its own network namespace (namespaced-wireguard, vopono, or your
own `ip netns` setup), point the module at it:

```nix
services.delune.vpn.namespace = "wg";   # /run/netns/wg
```

delune runs as the normal hardened service, with `NetworkNamespacePath` set, so it only
has the namespace's interfaces.

## 3. Your own setup

Anything that puts delune's process in a network where the only route out is the VPN
works: a systemd unit with `NetworkNamespacePath=`, a container with its network
attached to a VPN container, and so on.

## Check it

Settings → Connections shows **the address the Soulseek server sees**. It should be
your VPN's address, not your home's.
