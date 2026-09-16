# Bandcamp

## Buying

Album pages show **Buy on Bandcamp** with the digital price (and "or more" when the
artist lets you pay what you like). It opens the album on Bandcamp: payment always
happens there, never through delune.

## Linking your account

Each person can link their own Bandcamp account (Settings → Bandcamp). Then:

- your **purchases** are listed (the **Purchases** button on the search page),
  refreshed when you link, when you ask, and every twelve hours;
- albums you've bought say **Bought on Bandcamp** instead of showing a price;
- **Get the files** downloads a purchase in the format you choose (FLAC, ALAC, MP3 320,
  MP3 V0), unpacks it, and sends it to [Review](review.md) like any other download.

Bandcamp has no public API for this, so delune uses your browser's sign-in cookie,
called `identity`:

1. Sign in at [bandcamp.com](https://bandcamp.com) on a computer.
2. Open the developer tools (**F12**) → **Application** (Chrome) or **Storage**
   (Firefox) → **Cookies** → `https://bandcamp.com`.
3. Copy the value of `identity` and paste it into Settings → Bandcamp.

A whole `Cookie:` header, or a `cookies.txt` export, works too. delune checks it with
Bandcamp before keeping it.

The cookie is a login: delune stores it only in its database (readable only by delune),
never logs it, never sends it back to the browser, and only ever sends it to Bandcamp.
**Unlink** forgets it. If Bandcamp signs delune out, Settings says so and asks you to
link again.
