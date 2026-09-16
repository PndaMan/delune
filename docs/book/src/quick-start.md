# Quick start

On Linux or macOS, one command installs delune and walks you through setting it up:

```sh
curl -fsSL https://raw.githubusercontent.com/PndaMan/delune/main/install.sh | sh
```

The script:

1. works out your system (Linux or macOS, Intel or ARM),
2. downloads the latest release and checks it against its published SHA-256,
3. installs `delune` and `delune-tui` (to `~/.local/bin`, or `/usr/local/bin` as root),
4. starts [`delune setup`](setup-wizard.md).

The wizard asks for your music folder, Navidrome and a Soulseek account, tries each
one, saves the settings and starts delune as a service. When it's done it prints the
address to open, usually `http://<this machine>:7474`.

> **On NixOS**, use the [module](install/nixos.md) instead: it keeps secrets out of
> the store and can route delune through a VPN.
>
> **With Docker**, see [Docker and Compose](install/docker.md).

## What you need

| | Why |
|---|---|
| A Navidrome server and an **admin** account on it | Sign-in, library checks and rescans |
| Write access to Navidrome's music folder | Imports move files there |
| A Soulseek account | Searching and downloading (a new name is registered on first sign-in) |
| Optionally, a forwarded port | Other Soulseek users can reach you: more results, faster downloads |

## After installing

- Open the web app and sign in with your Navidrome account. Navidrome admins are
  delune admins.
- Search for an album, open a result and press **Download**. It arrives in **Review**.
- Approve it, and it's in Navidrome a moment later.
- On your phone, use **Add to Home Screen** for the app version. See
  [On your phone](use/phone.md).
