# Naming and the library

## The naming template

Settings → Library and imports → Naming decides where imported files go, relative to
the music folder. The default:

```text
{album_artist}/[{year} - ]{album}/{track} - {title}
```

- `{token}` is replaced with a value; see [Naming tokens](../reference/naming-tokens.md).
- `[ … ]` is an optional section: it disappears when a token inside it is empty. Above,
  albums without a year become `Artist/Album/…`. `[[` and `]]` are literal brackets.

**Match my library** looks at the files already in the music folder and suggests the
template they follow. The preview shows real examples as you type.

Options: track number padding, how multi-disc albums are laid out, what replaces
characters filesystems don't allow, whitespace handling, and a limit on the length of
each part of the path.

These settings are for admins; they shape everyone's library.

## Home and stats

The home screen, under the search box, shows what's downloading, what's waiting for
review, the albums added to Navidrome lately, and the library at a glance. **All stats**
opens the full picture: albums, artists, songs, listening time and size; how much is
hi-res, CD quality or lossy; the artists with the most albums; releases by decade;
genres; what came in through delune each month and who added it; and (for people who
manage delune) the Soulseek users you've downloaded most from. The counts come from
Navidrome and are kept for an hour; **Count again** refreshes them.

## Album and artist pages

Album, artist and song names are links throughout the app.

- **Album:** cover, tracklist and running time, whether it's in your library, **Find on
  Soulseek**, **Keep looking for it**, the Bandcamp price, and following the artist.
- **Artist:** what your library has by them, their albums and singles (from Deezer),
  their SoundCloud, and **Follow**.
- **Song:** where it's from, and its lyrics, verse by verse, with **Copy**.

Each has its own address (`/album/<artist>/<title>`, `/artist/<name>`,
`/song/<artist>/<title>`), so they can be shared and opened directly.
