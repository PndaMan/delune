# Downloads and review

## Downloads

Every file in a folder is queued with that person at once. The Downloads page shows
progress, your place in their queue, and lets you:

- **Stop** (keeping what's arrived) and **Resume**;
- **Start next**, when only a few downloads may run at once (Settings → Sharing →
  *downloads at once*);
- **Try someone else**, when a download failed: delune searches again and takes the best
  copy from anyone this album hasn't already failed with;
- **Find another copy**, to search for it yourself;
- **Remove**, which deletes what was downloaded.

Downloads survive restarts: delune picks up where it left off.

## Review

When a download finishes, delune checks it before anything touches your library:

- every file is **decoded** end to end;
- lossless files are checked for a **brick-wall cutoff** that gives away a transcode
  (an MP3 dressed up as FLAC);
- tags are read and the **destination path** is planned with your naming template;
- **conflicts** with files already in the library are found.

The Review page shows all of that per track, with the planned paths. Then:

- **Import** moves the files into the library, embeds artwork, fetches lyrics, and asks
  Navidrome to rescan. The album shows up in Navidrome a moment later.
- **Discard** deletes the download.

### Who reviews what

| | |
|---|---|
| **Admins** (and people who manage delune) | A clean download — nothing blocked, no conflicts, no suspected transcodes or unreadable files — **imports itself**, with a notification. Anything flagged waits in Review. |
| **Members** | Their downloads wait in Review for them to import. |
| **Members, with approval on** | They can't import; an admin approves from Review. People can be allowed to skip approval. |

The Review tab shows how many albums are waiting.

### Adding music you have

**Add music** (the button at the top of Downloads) takes tracks, a whole folder (choose
it, or drag it in) or a zip. Only music and pictures are kept. The upload becomes a
download like any other: its files are played through and checked, named from their
tags (the album and artist fields are only needed for untagged files), joined to an
album you already have part of, and wait here for review, or go straight in for people
who may import their own downloads. **Keep this album complete** also follows the album,
so its missing tracks are looked for.

### Listening first

Every track in Review has a play button: the file streams from the server, so it starts
at once and you can skip around. The length and frequency figure beside a track opens
its **spectrogram**, the track's frequencies over time. A file converted from MP3 shows
a flat ceiling (often around 16 or 19 kHz) well below the top, and the ceiling delune
detected is marked.

### Filling gaps in an album you have

When the library already has part of an album, the release view picks only the missing
tracks. On import they go into the album's existing folder and get its album, artist
and year tags, so Navidrome shows one album rather than two. Albums that change after
release (tracks added, order changed, like Fred again..'s *USB*) are numbered by their
current tracklist: new tracks get their place in it, and tracks already in the library
are renamed and renumbered to match. The review lists what moves. On albums with more
than one disc, numbers are left as they are.

## Artwork and lyrics

On import (Settings → Library and imports):

- **Artwork:** the folder's cover image goes alongside, and is embedded in each file.
- **Lyrics** from [LRCLIB](https://lrclib.net): a `.lrc` file beside each track, in the
  tags, both, or off. Synced lyrics are used when they exist.

## History

The History page (in the profile menu) lists what you've downloaded: everything, what
was added to the library, requests, and what didn't work out. Admins can switch to
someone else's history, or everyone's.
