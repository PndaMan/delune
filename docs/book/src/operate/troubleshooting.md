# Troubleshooting

## Signing in

**"Wrong username or password", but Navidrome accepts it.** delune signs you in against
Navidrome at `DELUNE_NAVIDROME_URL`. Check that address works *from where delune runs*
(see [Can delune reach it?](../connect/navidrome.md#can-delune-reach-it)).

**Too many attempts.** After five failed sign-ins, delune waits a minute.

**Someone's admin rights didn't change.** They follow Navidrome within about 20
seconds; the page updates on its own.

## Navidrome

**Albums don't show up after import.** delune asks Navidrome to scan and retries for a
few minutes. If Navidrome was down longer, its own scheduled scan picks them up.
Check the delune account is an **admin** in Navidrome.

**delune in a container can't reach Navidrome.** `127.0.0.1` inside a container is the
container. Use the bridge address, and let Navidrome listen on it.

## Soulseek

**Soulseek keeps going offline.** One account can only be signed in once. Stop the
other client (slskd, Nicotine+, another delune), or use another name. When another
client takes the account, delune signs back in by itself after ten minutes.

**An artist never finds anything.** The Soulseek server gives every client a list of
phrases (usually at a rights holder's request) that they shouldn't answer searches for,
so nobody replies. delune tells you when a search is one of them, and doesn't answer
such searches from its own shares either. Look for the music where the artist sells it.

**Search stops working after the VPN restarts.** With `services.delune.vpn.container`,
delune restarts along with the VPN container, since it can't use the old container's
network. Elsewhere, restart delune after recreating its VPN container.

**Few results, downloads that never start.** Other users can't reach you. Forward the
listening port (or turn on UPnP), and check Settings → Connections says *reachable*.
Behind a VPN, the VPN has to forward it too.

**Results but downloads fail.** Try **Find another copy**; peers go offline. The
release view shows how downloads from each person have gone before.

## Imports

**"Import stopped: permission denied".** delune can't write to the music folder. Run it
as the folder's owner or group (for a NAS share, the share's uid and gid).

**A NAS folder is empty inside a container, or delune fails to start after a reboot.**
The share wasn't mounted yet. Mount it with `rslave` propagation, and start delune after
the mount (the NixOS module does both).

**Conflicts.** A file with the same destination already exists. Change the naming
template, or remove the old copy in Navidrome first.

## NixOS

**`Unknown lockfile version` or a hash mismatch building delune.** Your flake makes
delune follow an older `nixpkgs`. Remove `inputs.delune.inputs.nixpkgs.follows`.

**`secret … cannot be found`.** A sops secret the configuration references isn't in
your secrets file yet. Add it, then rebuild.

## The web app

**Something went wrong.** Reload. If it keeps happening, open the browser console and
[open an issue](https://github.com/PndaMan/delune/issues) with the message.

**Nothing updates live behind a proxy.** The proxy is buffering the event stream; see
[Reverse proxy](../connect/reverse-proxy.md).

**On iPhone, the bottom bar floats after typing.** Update delune: current versions hide
the bar while the keyboard is up and re-lay out the page when it closes.

## Lyrics

**"No words for this one".** LRCLIB doesn't have them, or the song's length doesn't
match any entry within ten seconds. delune also searches by title alone when the artist
isn't known.

## Still stuck

Run with `DELUNE_LOG=debug` and [open an issue](https://github.com/PndaMan/delune/issues)
with what you did, what happened, and the log around it (passwords are never logged).
