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
  their SoundCloud, and **Follow artist**.
- **Song:** where it's from, and its lyrics, verse by verse, with **Copy**.

Each has its own address (`/album/<artist>/<title>`, `/artist/<name>`,
`/song/<artist>/<title>`), so they can be shared and opened directly.

## Checking the library

**Settings → Library and imports → Library check** (for people who manage delune) looks through the music
folder for three things:

- **An album in several folders.** This happens when the artist's folder is spelled in
  different ways (`Fred again..`, `Fred again._`) or the folder names carry different
  years. **Merge** moves the tracks into the fullest folder, keeping the better copy
  where both folders have a track. It then makes the album one album, as below.
- **An album that shows up more than once.** Players group tracks by their tags, not
  their folder. Tracks from different sources often disagree on the album artist (or
  list it twice), the release date or the MusicBrainz release, and each variant shows as
  its own album. **Make it one album** gives every track the album title, album artist,
  dates (original dates too, in the same order) most of them already have, and drops
  release ids the tracks don't share. The folder is marked changed so Navidrome rescans
  it.
  Tracks tagged for a different album move to that album's folder. The album gets a
  cover from Deezer if it has none.
- **A track twice in one folder**, such as an MP3 beside the FLAC. **Keep the best**
  keeps the highest-quality copy of each track.

Tracks imported into an album you already have get the same treatment automatically.

These fixes never delete anything:

- Whatever a fix takes out goes to `.delune-trash` in the music folder, which Navidrome
  doesn't show. Anything in the trash is emptied after 30 days.
- **Recent changes** lists each fix and upgrade: what it was, and every file it moved,
  retagged or put away. **Undo** reverses all of it, tags included.
- Before each fix, delune checks that the files are still exactly as they were when you
  looked. If anything changed, it asks you to check again.
- A fix that touches more than 20 files needs a second tap. One that would touch more
  than 400 files is refused.

**Ignore** hides a finding for good: the same folders won't come up again, even as
their files change. **Show again**, below the list, brings every ignored finding back.

New tracks go into an album's existing folder even when that folder is spelled
differently, so new splits shouldn't appear.
