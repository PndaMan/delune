#!/usr/bin/env bash
# Run the API server and the hot-reloading web UI together.
#
#   scripts/dev.sh            # API on :7474, web UI on :5173 (open this one)
#
# The web UI reloads instantly on save. The API server rebuilds and restarts when
# Rust sources change if `watchexec` is installed; otherwise restart this script.
set -euo pipefail
cd "$(dirname "$0")/.."

cleanup() { kill 0 2>/dev/null || true; }
trap cleanup EXIT INT TERM

if command -v watchexec >/dev/null; then
  watchexec --restart --watch crates --exts rs,toml -- cargo run -p delune -- serve &
else
  cargo run -p delune -- serve &
fi

(cd web && bun run dev --host) &
wait
