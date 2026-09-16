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

## Album and artist pages

Album, artist and song names are links throughout the app.

- **Album:** cover, tracklist and running time, whether it's in your library, **Find on
  Soulseek**, **Keep looking for it**, the Bandcamp price, and following the artist.
- **Artist:** what your library has by them, their albums and singles (from Deezer),
  their SoundCloud, and **Follow**.
- **Song:** where it's from, and its lyrics, verse by verse, with **Copy**.

Each has its own address (`/album/<artist>/<title>`, `/artist/<name>`,
`/song/<artist>/<title>`), so they can be shared and opened directly.
