# Wishlist, following and automation

## Wishlist

The wishlist keeps searching for things that aren't on Soulseek yet, or not in a good
enough copy. Open it from the search page.

- Search for an artist or album inside the sheet and add it, or **Keep looking for it**
  after a search that found nothing.
- Each item has a **minimum quality** (any, lossless, hi-res) and can **download
  automatically** when a good copy turns up, or just tell you.
- Items are searched one per interval the Soulseek server allows (usually about twelve
  minutes), oldest first.
- A download that fails or is removed puts its item back in line.
- Pause or remove items any time. Up to 500 items can be waiting per server.

## Following artists

**Follow artist** (on their page, an album, or in the wishlist) puts their new albums
and EPs on your wishlist as they're released. delune checks each followed artist
once a day, using Deezer's catalogue. Following is per person, and artists you follow
are listed under **Artists you follow** in the wishlist.

An album view has two buttons, and they do different things:

| Button | Watches | Puts on the wishlist |
|---|---|---|
| **Follow artist** | the artist | their next albums and EPs |
| **Keep album complete** | this album only | its missing tracks, and any added later |

SoundCloud artists can be followed too; see [SoundCloud](soundcloud.md).

## New releases

**New releases** (from the home screen, or `/radar`) lists what the artists you follow
released in the last four months, and what's announced, with whether each is already
in your library, on your wishlist or downloading. **Wish for it** adds one; upcoming
releases can be wished for before they're out. You're also notified when a followed
artist's new album or EP goes on your wishlist.

## Keeping albums complete

**Keep album complete** (in a release or album view) watches that one album. Once a day
delune compares the album's current tracklist with your library and puts each missing
track on the wishlist, once. That covers tracks you skipped, and tracks the artist adds
after release, as with albums that keep growing. Each is found in a copy of the album and
imported into the album you already have, numbered in the album's current order (see
[Filling gaps](review.md#filling-gaps-in-an-album-you-have)). An album you don't have
yet goes on the wishlist whole.

Followed albums are listed under **Albums kept complete** in the wishlist.

## Quality upgrades

Settings → Automation → **Quality upgrades** (off by default) slowly walks through the
library, a few dozen albums an hour, and puts every lossy album on the wishlist with the
quality you choose. Upgrades download like anything else and still go through review.

When an import brings a better copy of a track the album already has, the new copy
takes its place and the old one (with its lyrics file) moves to `.delune-trash` in your
library folder. Navidrome doesn't show that folder, and delune empties anything in it
after 30 days. A copy that isn't better than what you have is left out, and Review says
so.

## Downloading what it finds

Settings → Automation → **Download what it finds** (on by default) lets items that
following and upgrades add start downloading as soon as a match turns up; otherwise they
wait on the wishlist for someone to download. Everything still stops at review, and admins' clean downloads import
themselves (see [Review](review.md#who-reviews-what)).
