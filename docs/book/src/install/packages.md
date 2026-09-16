# Arch Linux and Homebrew

## Arch Linux

`packaging/aur/PKGBUILD` builds delune from source and installs:

- `/usr/bin/delune` and `/usr/bin/delune-tui`
- a systemd service running as the `delune` user
- `/etc/delune/delune.env` for settings and passwords

```sh
sudoedit /etc/delune/delune.env
sudo systemctl enable --now delune
```

The service can write to `/srv/music`. For another music folder, run
`systemctl edit delune` and add it to `ReadWritePaths=`.

## Homebrew (macOS and Linux)

`packaging/homebrew/delune.rb` installs the release binaries, and
`brew services start delune` runs the server with its data in Homebrew's `var`.

> The AUR package and the Homebrew tap are published with the first tagged release.
> Until then, use the [install script](script.md) (once a release exists) or build
> [from source](source.md).
