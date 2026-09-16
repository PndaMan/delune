# Naming tokens

Use these in the naming template as `{token}`. Wrap parts that may be empty in `[ ]`:
the whole bracket disappears when a token inside it has no value. `{{` and `}}` are
literal braces.

| Token | |
|---|---|
| `{title}` | Track title |
| `{artist}` | Track artist(s) |
| `{album_artist}` | Album artist |
| `{album}` | Album title |
| `{edition}` | Edition or version, e.g. Deluxe |
| `{year}` | Original release year |
| `{track}` | Track number (with the padding and multi-disc settings) |
| `{disc}` | Disc number |
| `{disc_count}` | Number of discs |
| `{genre}` | Primary genre |
| `{composer}` | Composer |
| `{label}` | Record label |
| `{catalog}` | Catalogue number |
| `{isrc}` | ISRC |
| `{codec}` | Codec, e.g. FLAC |
| `{quality}` | Quality label, e.g. FLAC 24/96 |
| `{bit_depth}` | Bit depth, e.g. 24 |
| `{sample_rate}` | Sample rate in kHz, e.g. 96 |
| `{bitrate}` | Bitrate in kbps |
| `{source}` | Where it came from, e.g. Soulseek |
| `{mbid}` | MusicBrainz release ID |

## Examples

| Template | Result |
|---|---|
| `{album_artist}/[{year} - ]{album}/{track} - {title}` | `Radiohead/1997 - OK Computer/02 - Paranoid Android.flac` |
| `{album_artist}/{album}[ ({year})]/{track} {title}` | `Radiohead/OK Computer (1997)/02 Paranoid Android.flac` |
| `{album_artist}/{album}/{track} - {artist} - {title}` | `Radiohead/OK Computer/02 - Radiohead - Paranoid Android.flac` |
| `{album}/[[{year}]] {title}` | `OK Computer/[1997] Paranoid Android.flac` |

`[ ]` marks an optional part; write `[[` and `]]` for literal brackets. The preview in
Settings shows exactly what your template produces, including a second disc and
characters that aren't allowed in file names.

The extension is added for you. Characters that aren't allowed in file names are
replaced (Settings → Naming → options).
