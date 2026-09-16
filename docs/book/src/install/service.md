# Running as a service

`delune setup` writes a unit for you. If you'd rather do it by hand, this is what it
writes for a user service (`~/.config/systemd/user/delune.service`):

```ini
[Unit]
Description=delune
Wants=network-online.target
After=network-online.target

[Service]
ExecStart="/home/you/.local/bin/delune" serve
Environment="DELUNE_DATA_DIR=/home/you/.local/share/delune"
Environment="DELUNE_BIND=0.0.0.0:7474"
Restart=on-failure
RestartSec=5
UMask=0002

[Install]
WantedBy=default.target
```

```sh
systemctl --user daemon-reload
systemctl --user enable --now delune
loginctl enable-linger $USER       # keep it running when you're logged out
journalctl --user -u delune -f     # logs
```

A system-wide unit is the same with `User=` set and `WantedBy=multi-user.target`,
in `/etc/systemd/system/`. The packaged unit (`packaging/systemd/delune.service`) adds
sandboxing: a dedicated user, a read-only system, and write access only to the data
and music folders.

Passwords can live in `config.toml` (written by the wizard or the web app, mode 0600)
or in an `EnvironmentFile=` that only the service can read.

## macOS

`brew services start delune`, or a LaunchAgent that runs `delune serve` with
`DELUNE_DATA_DIR` set.
