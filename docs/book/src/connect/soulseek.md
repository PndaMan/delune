# Soulseek

delune has its own Soulseek client: it doesn't need slskd or Nicotine+.

## The account

Any username and password work: the Soulseek server registers a new name the first
time it signs in. **One name can only be signed in once.** Signing in somewhere else
(another delune, slskd, Nicotine+, or the setup wizard's check) signs the other one out.
If you're replacing slskd, stop it first and give delune the same account.

Settings → Connections shows the account, whether it's online, the address the
Soulseek server sees, and whether other users can reach you.

## The listening port

Other users connect to you on the listening port (`DELUNE_SLSK_PORT`, default `2234`).
When they can, you get more search results and downloads start straight away; when
they can't, some peers can't send to you at all.

- **At home:** forward the port on your router, or turn on *Forward the Soulseek port
  automatically* (UPnP) in Settings → Sharing.
- **Behind a VPN:** forward it with your VPN provider, and let the VPN container accept
  it (gluetun: `FIREWALL_VPN_INPUT_PORTS`). See [Through a VPN](vpn.md).

Settings → Connections says *reachable* once someone on the internet has connected to
that port.

## Sharing

Soulseek works on give and take, and many people only upload to those who share.
Settings → Sharing:

- **Share my library** — off until you turn it on. People browse it as `Music`, never
  the real path.
- **Upload slots**, **files per person**, and **speed limits**, with an optional
  **schedule** (for example, slower during the evening).
- **Only share with people who share**, and a **blocked** list.
- **Pass searches on to others** — take part in the distributed search network.

The share list is rebuilt after imports and every six hours.

## Searching well

- delune retries a search with fewer words when the network returns nothing.
- Pasting a link from Spotify, Apple Music, Tidal, Qobuz, Deezer, YouTube Music,
  SoundCloud, Bandcamp or MusicBrainz resolves the release first, then ranks folders by
  how much of its tracklist they hold.
- The Soulseek server limits how often you can search; wishlist searches take one turn
  per interval the server sets.
