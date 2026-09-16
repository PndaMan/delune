# SoundCloud

SoundCloud no longer hands out API keys, so delune only reads what anyone can: an
artist's public page, the RSS feed SoundCloud publishes for every account, and a
track's page. It never downloads audio from SoundCloud itself.

## On artist pages

If delune finds the artist's own SoundCloud (an account with the same name that's
verified or well followed), their page shows it, with their newest tracks. Open a
track for:

- **Soulseek** — search for it;
- **Free download** — when the artist gives it away, either their own download link or
  SoundCloud's download button;
- **Listen** — on SoundCloud.

When an admin has set up a [fetch command](fetch-command.md), a track with SoundCloud's
own download button can be fetched straight into Review.

## Following

**Follow new tracks** puts each track the artist posts from now on onto your wishlist
(as a single song), checked every few hours. SoundCloud follows are listed in the
wishlist, per person.
