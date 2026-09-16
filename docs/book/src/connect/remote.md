# Remote access

## Tailscale

The simplest private way in. On the delune machine:

```sh
tailscale serve --bg --https=8446 http://127.0.0.1:7474
```

Then `https://<machine>.<tailnet>.ts.net:8446` works from your other devices, with a
real certificate — and the installed phone app works over it.

> **Pick a port nothing else uses.** `tailscale serve` really listens on the tailnet
> address. If another service (a reverse proxy on `:443`, an identity provider on
> `:8443`) binds all addresses on the same port, whichever starts first after a reboot
> wins, and the other fails. Check `ss -tln` and your configuration first.

## A public name

Behind a [reverse proxy](reverse-proxy.md), delune is fine on the internet as long as
**Navidrome sign-in is on**: every route except health, sign-in and the web app itself
needs a session. Failed sign-ins are limited (five, then a minute's wait). Don't expose
a delune running in open mode.

## The terminal client

`delune-tui` connects over whatever you've set up:

```sh
delune-tui https://delune.example.com
delune-tui https://machine.tailnet.ts.net:8446
delune-tui myserver           # finds delune on that host
```
