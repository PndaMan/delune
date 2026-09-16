# The setup wizard

`delune setup` is a guided first run in the terminal. The install script starts it for
you; run it again at any time to change the answers.

```sh
delune setup                       # data in ~/.local/share/delune (or /var/lib/delune as root)
delune setup --data-dir /srv/delune
```

It needs a real terminal. Everything it sets can also be changed later in the web app
under **Settings → Connections**.

## The screens

| Screen | What it asks | What it checks |
|---|---|---|
| **Folders** | delune's data folder, and your music folder | The data folder can be created; the music folder exists and is writable |
| **Navidrome** | Address, admin username and password (empty skips it) | Signs in, and confirms the account is an admin |
| **Soulseek** | Username, password, listening port (empty skips it) | Signs in to the Soulseek network |
| **Network** | The web address to listen on, and whether to open the port with UPnP | — |
| **Keep it running** | A user service, a system service, or nothing; whether to start now | — |
| **Finish** | A summary | — |

Each screen with a connection tries it when you press **Enter** and only moves on once
it works. Keys: **Enter** next, **Esc** back, **↑↓** move, **Space** choose,
**Ctrl+C** quit without changing anything.

## What it writes

- `<data folder>/config.toml` — the connections, readable only by you (mode 0600).
- The UPnP choice, in delune's database.
- A systemd unit:
  - **User service:** `~/.config/systemd/user/delune.service`, enabled and started
    with `systemctl --user`. To keep it running while you're logged out:
    `loginctl enable-linger $USER`.
  - **System service** (as root): `/etc/systemd/system/delune.service`. Under `sudo`,
    it runs as the user who ran sudo, not root.
  - **On NixOS** it prints the module configuration instead, since `/etc` is managed
    by your configuration.

> **Checking a Soulseek account signs it in.** If the same account is already signed
> in somewhere else (another delune, slskd, Nicotine+), that session is signed out.
