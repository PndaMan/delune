# Searching and pasting links

The search box takes words (**artist and album** works best) or a link.

## Links

Paste a link from **Spotify, Apple Music, Tidal, Qobuz, Deezer, YouTube Music,
SoundCloud, Bandcamp** or **MusicBrainz**. delune works out the release from the
service's public data, matches it across services by barcode (UPC) and ISRC through
MusicBrainz, then searches Soulseek for it. Results are ranked by how much of that
release's tracklist each folder has.

On your phone, **Share → delune** from those apps does the same.

A playlist link (Spotify, Deezer) offers to put its songs, or their albums, on the
[wishlist](wishlist.md).

## Results

Results stream in as peers answer (about 20 seconds). Each is one **folder** from one
person, ranked:

1. lossless before lossy, hi-res first;
2. complete before partial;
3. then by quality, a free upload slot, and speed.

Badges show **In library**, **N missing**, and the quality. Filters narrow by quality
and "ready now" (a free slot). People you've hidden don't show up.

If nothing comes back, delune tries again with fewer words, and offers **Keep looking
for it** (the wishlist).

### Artists, albums and songs

When the search names an artist ("gorillaz", or "radiohead creep"), their picture
appears at the top. Tap it for their page: albums from Deezer, and which of them you
already have. **All · Artists · Albums · Tracks** switch what's shown:

- **All**: the artist, their albums on Deezer (tap one for its tracklist), a few
  matching songs, then album folders on Soulseek.
- **Artists**: everyone the search could mean.
- **Albums**: album folders on Soulseek, as above.
- **Tracks**: single songs whose title is in the search, one row per person sharing
  them, with the album's cover. Opening one downloads just that song. Name the artist
  and the song ("fred again jungle") for the best matches.

Scene release names (`Hum-Youd_Prefer_An_Astronaut-1995-FLAC`) are read like any
other, so they get their cover and proper album name.

### Searches Soulseek won't run

The Soulseek server tells every client to ignore searches containing some phrases,
usually at a rights holder's request, so nobody answers them. delune says so instead
of reporting "nothing found". The artist's page still shows what they've released.

## A release

Opening a result shows its tracklist, quality per file, how downloads from that person
have gone before, and whether the album is already in your library. Choose **Download**
(the whole folder, or tick tracks), or **Request** if your account asks for albums
instead.

The album and artist names open [album and artist pages](library.md#album-and-artist-pages).
