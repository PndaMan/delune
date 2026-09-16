# Security

## What delune protects

- **Passwords** are never stored for sign-in: Navidrome checks them. Session tokens are
  stored hashed, and sessions end after 30 days without use. Failed sign-ins are
  rate-limited.
- **Connection passwords** (Soulseek, Navidrome) live in `config.toml` or your
  environment/secrets, both readable only by delune. The database is too.
- **Bandcamp logins** are stored only in the database, redacted from logs, never sent to
  the browser, and only ever sent to Bandcamp.
- **The session cookie** is `HttpOnly`, `SameSite`, and `Secure` behind HTTPS.
- **Imports** can't write outside the music folder, and downloaded zips can't unpack
  outside their staging folder.
- **The fetch command** runs a program directly, without a shell, substituting only the
  link and the folder, and is disabled without sign-in.

## What you should do

- Keep Navidrome sign-in on for anything reachable beyond your machine. **Open mode**
  (no Navidrome) makes everyone who can reach delune an admin.
- Put delune behind HTTPS, and bind it to `127.0.0.1` behind your proxy.
- Give delune its own Navidrome admin account, so you can change or remove it
  independently.
- Share your library knowingly: it makes those files downloadable by anyone on
  Soulseek.
- Consider a [VPN](../connect/vpn.md): Soulseek shows peers your IP address.

## Reporting a problem

See [SECURITY.md](https://github.com/PndaMan/delune/blob/main/SECURITY.md). Please
report vulnerabilities privately, not in public issues.
