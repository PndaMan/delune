# HTTP API

Everything the web app and the terminal client do goes through delune's HTTP API, so
anything they can do, a script can too.

- **Description:** OpenAPI 3.1 at `/api/v1/openapi.json`. A test makes sure every route is in it.
- **Base path:** `/api/v1`.
- **Errors:** JSON `{"code": "…", "message": "…"}` with a matching status.

## Signing in

```sh
curl -s -X POST https://delune.example.com/api/v1/session \
  -H 'content-type: application/json' \
  -d '{"username": "you", "password": "…", "token": true}'
# → { "username": "you", …, "token": "…" }

curl -s -H "Authorization: Bearer $TOKEN" https://delune.example.com/api/v1/downloads
```

The web app uses the `delune_session` cookie instead. `GET /api/v1/session` answers
`null` when nobody is signed in. Sessions end after 30 days without use, or with
`DELETE /api/v1/session`.

## Live updates

`GET /api/v1/events` is a Server-Sent Events stream. Each event's data is the name of
what changed (`downloads`, `progress`, `requests`, `notifications`, `wishlist`,
`follows`, `sharing`, `people`, `session`, `soulseek`, `chat`, `favourites`,
`bandcamp`, `soundcloud`, `automation`, `import-options`, `external`, or `all` after a
gap); fetch that part again.

## Some useful routes

| | |
|---|---|
| `GET /health` | Liveness, no sign-in needed |
| `GET /search?q=…` | Soulseek search, as a Server-Sent Events stream |
| `GET /classify?q=…` | What a pasted link points at |
| `POST /downloads` | Start a download |
| `GET /downloads/{id}/review` | The review report |
| `POST /downloads/{id}/import` | Import into the library |
| `GET/POST /wishlist` | The wishlist |
| `GET /music/artist`, `/music/album`, `/music/lyrics` | Artist, album and lyrics lookups |
| `GET /soulseek` | Soulseek connection status |

The TypeScript types for every request and response are generated from the Rust types
(`web/src/lib/api.generated.ts`).
