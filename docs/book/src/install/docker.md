# Docker and Compose

The repository's `compose.yaml` runs delune next to an existing Navidrome.

```sh
git clone https://github.com/PndaMan/delune && cd delune
cp .env.example .env     # fill in Soulseek and Navidrome
docker compose up -d
```

```yaml
services:
  delune:
    build: .
    restart: unless-stopped
    env_file: .env
    environment:
      DELUNE_DATA_DIR: /data
      DELUNE_LIBRARY_DIR: /music
    ports:
      - "7474:7474"   # web app and API
      - "2234:2234"   # Soulseek, so peers can reach you
    volumes:
      - delune-data:/data
      - /path/to/music:/music     # the folder Navidrome scans
volumes:
  delune-data:
```

The image runs as an unprivileged user (`nonroot`, uid 65532). The music folder must
be writable by it. For a NAS share owned by a particular user, run the container as
that user (`user: "1024:100"`) and make sure `/data` belongs to them too.

## Reaching Navidrome from the container

`localhost` inside the container is the container, not your host. Point
`DELUNE_NAVIDROME_URL` at something the container can reach:

- another container on the same Compose network: `http://navidrome:4533`;
- a service on the host: `http://host.docker.internal:4533` (add
  `extra_hosts: ["host.docker.internal:host-gateway"]` on Linux);
- your reverse proxy's public name.

If Navidrome only listens on `127.0.0.1` on the host, the container can't reach it at
all. Let it listen on the bridge too, and keep the port closed to your LAN with the
firewall.

## Through a VPN

`compose.yaml` has a commented-out [gluetun](https://github.com/qdm12/gluetun) service.
See [Through a VPN](../connect/vpn.md).

## NAS shares

Mount the share on the host, then bind it in. If it's mounted on demand (autofs,
systemd automount), use `:rslave` so the container sees it once it appears:

```yaml
volumes:
  - /mnt/nas/music:/music:rslave
```
