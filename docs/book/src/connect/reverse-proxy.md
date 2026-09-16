# Reverse proxy and HTTPS

delune serves plain HTTP. For anything beyond your LAN, put a reverse proxy with HTTPS
in front of it, and keep delune itself on `127.0.0.1` (the NixOS module's default).

Two things matter:

- **Live updates use Server-Sent Events** (`/api/v1/events`, and search results stream
  the same way). The proxy must not buffer those responses.
- **Tell delune the page came over HTTPS** with `X-Forwarded-Proto: https`, so its
  sign-in cookie is marked `Secure`. Most proxies send it already.

## Caddy

```caddyfile
delune.example.com {
    reverse_proxy 127.0.0.1:7474
}
```

Caddy streams responses and sets the forwarding headers by default.

## Traefik

```nix
services.traefik.dynamicConfigOptions.http = {
  routers.delune = {
    rule = "Host(`delune.example.com`)";
    service = "delune";
    tls.certResolver = "letsencrypt";
  };
  services.delune.loadBalancer.servers = [ { url = "http://127.0.0.1:7474"; } ];
};
```

Traefik doesn't buffer by default.

## nginx

```nginx
server {
    server_name delune.example.com;
    listen 443 ssl http2;

    location / {
        proxy_pass http://127.0.0.1:7474;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        # Live updates and streamed search results
        proxy_buffering off;
        proxy_read_timeout 1h;
    }
}
```

## Uploads

Profile pictures are small; nginx's default `client_max_body_size` (1 MB) is fine for
them, but raise it if pictures fail to save.

## A sub-path

delune expects to be at the root of its host (`https://delune.example.com/`). Give it
its own (sub)domain rather than a path like `/delune`.
