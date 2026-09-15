# Web UI design

The design language for delune's web interface. Read this before changing how
anything looks.

## The idea

delune is named after *Clair de Lune*, and it's used the way people actually find
music: late, one album at a time, with care about how it sounds. The interface is a
**night sky you search from**. It's calm and dark by default, with one luminous
object, the moon, that shows what's happening.

## Principles

1. **One bold thing.** The moon is the only decorative element. It waxes as a search
   runs and shows the Soulseek connection. Everything around it stays quiet.
2. **Quality is colour.** Hi-res, lossless and lossy each have a colour, used for
   labels, row edges and filters. After a few searches you read quality before
   you read text.
3. **Lists, not cards.** Results are dense rows people can scan and move through
   with the keyboard. A release opens in a large modal with its artwork.
4. **Keyboard first.** `⌘K` or `/` focuses search, `↑` `↓` move through results,
   `Enter` opens one, `Esc` goes back. Every action is reachable without a mouse.
5. **Empty and error states give directions.** They say what to do next in plain
   words.

## Colour

Dark ("night") is the default; light ("blue hour") follows the OS setting.

| Token | Night | Blue hour | Use |
|---|---|---|---|
| `--background` ink | `#0F1226` | `#E9EBF4` | Page ground |
| `--card` dusk | `#161A33` | `#F5F6FA` | Rail, sheets, fields |
| `--accent` haze | `#212646` | `#DFE2F0` | Hover, selection |
| `--foreground` moon | `#EDEBE1` | `#1A1D38` | Text |
| `--muted-foreground` mist | `#959AC0` | `#5C6184` | Secondary text |
| `--primary` lilac | `#AB9DFF` | `#5646D0` | Focus, interactive |
| `--q-hires` gold | `#F2CF7E` | `#946508` | 24-bit or above 48 kHz lossless |
| `--q-lossless` sea | `#8FDCC6` | `#17785F` | CD-quality lossless |
| `--q-lossy` slate | `#8C95C2` | `#56608C` | MP3, AAC, Opus |

Colours are deliberately not a warm cream-and-clay palette, nor black with a single
neon accent. The night is blue-violet, and the moon is the warm white.

## Type

One family: **IBM Plex Sans** (400, 500, 600, 700). An engineered grotesque with a
hardware heritage that suits audio gear, without the look of this year's startup fonts.

- Headlines are 600 weight with tight tracking.
- Body text is 15px at 400.
- All numbers use tabular figures so columns of sizes and durations line up.
- No all-caps labels. Sentence case everywhere.

Scale: 12.5 · 14 · 15 · 18 · 24 · 36 · 56px.

## Layout

```text
idle                                   searching
┌────┬───────────────────────────┐     ┌────┬──────────────────────────────────┐
│ ☾  │                           │     │ ☾  │ [◐ search field            ⌘K]    │
│    │         ( moon )          │     │    │ 124 releases from 86 people      │
│ ⌕  │   Find something to       │     │ ⌕  │ [All Hi-res Lossless Lossy] [..] │
│ ↓  │   listen to               │     │ ↓  │ ▌FLAC 24/96  OK Computer   …     │
│ ✓  │   [ search field    ⌘K ]  │     │ ✓  │ ▌FLAC 16/44  OK Computer   …     │
│ ⚙  │   recent searches         │     │ ⚙  │ ▌MP3 320     OK Computer   …     │
│ ●  │                           │     │ ●  │                                  │
└────┴───────────────────────────┘     └────┴──────────────────────────────────┘
```

- A narrow icon rail on the left; labels appear on hover and at wide widths.
- Content is left-aligned, except the idle search screen, which is centred.
- At phone width the rail becomes a bottom bar and result rows stack.

## Motion

- The moon's terminator moves with search progress: the one continuous animation.
- Result rows appear without per-row entrance animations; sorting is instant.
- The release modal scales in from 97% over 200ms. Everything respects `prefers-reduced-motion`.
