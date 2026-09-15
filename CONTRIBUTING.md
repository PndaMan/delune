# Contributing to delune

Thanks for helping. This guide covers setup, how the code is organised, and what a
good pull request looks like.

## Setup

Requirements: Rust 1.90 or newer, [Bun](https://bun.sh) 1.3+, and optionally
Chromium for UI screenshots.

```sh
(cd web && bun install && bun run build)   # web UI, embedded into the binary
cargo test --workspace                    # all Rust tests
cargo run -- serve                        # server on :7474
cargo run -- tui                          # terminal UI
(cd web && bun run dev)                   # web UI with hot reload, proxies /api to :7474
```

## Before you start

- **Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — ten minutes that save hours.
- **Check the [roadmap](docs/ROADMAP.md) and open issues.** For anything bigger than
  a bug fix, open an issue first so we can agree on the approach.
- **Decisions that change the architecture need an ADR** in [docs/adr/](docs/adr/).

## Rules that aren't negotiable

These come from the product's core promises. A pull request that breaks one won't
be merged, however good it is otherwise.

1. **Soulseek is searched first.** Source ordering goes through
   `SourcePolicy::search_order()`; never build a source list anywhere else.
2. **Streaming providers stay opt-in.** Nothing may enable one by default.
3. **Nothing enters the library without review.**
4. **Be a good network citizen.** Soulseek changes must keep search rate limiting,
   sharing and slot limits intact, and need tests.
5. **Both clients use the API.** No feature exists only in the web UI or only in the
   TUI because of a shortcut around the server.

## Code style

**Rust**
- `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings` must pass.
  Clippy's `pedantic` group is on; allow a lint locally with a comment saying why.
- Keep pure logic separate from I/O so it can be unit-tested (see
  `delune-soulseek::wire` and `delune-library::naming`).
- Every public item gets a doc comment that says what it's for, not just what it is.
- Errors use `thiserror` in libraries and `anyhow` only in the binary and TUI.
- No `unsafe`. The workspace forbids it.

**TypeScript**
- `bun run typecheck`, `bun run lint` and `bun run build` must pass.
- Components from `src/components/ui` come from shadcn; add new ones with
  `bunx shadcn@latest add <name>` rather than hand-writing primitives.
- Use theme tokens (`bg-card`, `text-muted-foreground`, `border`), never raw colours.
- Every async view needs designed loading, empty and error states.
- Check your change at 400px wide and with keyboard only.

**Writing in the UI**
- Sentence case, plain verbs, name things the way a user would.
- Errors say what happened and what to do next.

## Commits and pull requests

- Conventional commit messages: `feat(soulseek): …`, `fix(web): …`, `docs: …`.
- One logical change per pull request, with tests for behaviour changes.
- UI changes include before/after screenshots.
- CI must be green.

## Licensing

delune is AGPL-3.0-or-later. By contributing you agree that your contribution is
licensed the same way. Don't copy code from projects with incompatible licences;
in particular, antra's source is under the Elastic License 2.0 and can't be used.
