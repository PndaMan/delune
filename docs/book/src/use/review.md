# Downloads and review

## Downloads

Every file in a folder is queued with that person at once. The Downloads page shows
progress, your place in their queue, and lets you:

- **Stop** (keeping what's arrived) and **Resume**;
- **Start next**, when only a few downloads may run at once (Settings → Sharing →
  *downloads at once*);
- **Find another copy**, when a download failed;
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

## Artwork and lyrics

On import (Settings → Library and imports):

- **Artwork:** the folder's cover image goes alongside, and is embedded in each file.
- **Lyrics** from [LRCLIB](https://lrclib.net): a `.lrc` file beside each track, in the
  tags, both, or off. Synced lyrics are used when they exist.

## History

The History page (in the profile menu) lists what you've downloaded: everything, what
was added to the library, requests, and what didn't work out. Admins can switch to
someone else's history, or everyone's.
