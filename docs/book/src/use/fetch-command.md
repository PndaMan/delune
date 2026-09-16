# Other sources (fetch command)

delune doesn't download from streaming services: that means getting around copy
protection and their terms. Instead, an admin can give delune a downloader they trust,
and delune runs it for a link. Whatever it fetches goes through the same
[review](review.md) as everything else.

Settings → Other sources:

- **Program** — for example `yt-dlp`, found on delune's `PATH`, or a full path.
- **Arguments** — one per line. `{url}` is replaced with the link and `{output}` with
  the folder to write into; both are required.

```text
-x
--audio-format
flac
-o
{output}/%(title)s.%(ext)s
{url}
```

The program is started directly, not through a shell, and only `{url}` and `{output}`
are substituted, so a link can't become a command. It runs as the delune service, with
an hour's limit, and can be stopped from Downloads.

Once it's set up, pasting a link delune can't search for offers **Fetch with …**.

The fetch command needs sign-in: it's switched off when delune runs in open mode.
