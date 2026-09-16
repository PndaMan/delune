# Notifications

delune's bell (top of the app) lists what happened: downloads ready for review or
failed, albums added to the library, requests decided, new requests (for people who
manage delune), and new releases from artists you follow. **Settings → Notifications**
sends them on, so you hear about them when delune isn't open. Each person sets their
own.

## On your phone or browser

Turn on **Notifications on this device** and allow them when the browser asks. It works
in Chrome, Edge and Firefox on desktop and Android, and in Safari on macOS. On an iPhone
or iPad, add delune to the home screen first (Share → Add to Home Screen) and turn it on
from the app opened there; Apple only allows it for installed apps.

Every device you turn on is listed, and can be removed from any other. Push needs delune
to be reached over HTTPS (a reverse proxy or Tailscale), as browsers require.

## ntfy

Paste an [ntfy](https://ntfy.sh) topic address, like `https://ntfy.sh/some-long-name`,
and subscribe to the same topic in the ntfy app. Anyone who knows a topic on ntfy.sh can
read it, so pick a name nobody would guess, or run your own ntfy server. People who
manage delune may also use a plain `http://` address, for an ntfy on the local network.

## Discord

In a Discord channel's settings, **Integrations → Webhooks → New Webhook → Copy Webhook
URL**, and paste it here. Messages arrive as the webhook, with a link back to delune.

## Choosing what's sent

**What to send** mutes kinds of notification everywhere except the bell. **Send a test**
tries every channel you've set up and says how each went.

Links in ntfy and Discord messages point at the address you last saved these settings
from.
