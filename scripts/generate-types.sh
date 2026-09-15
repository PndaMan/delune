#!/usr/bin/env bash
# Regenerate the web UI's TypeScript declarations from delune-core's API types.
# web/src/lib/api-contract.ts then checks the hand-written types against them.
set -euo pipefail
cd "$(dirname "$0")/.."

out=web/src/lib/api.generated.ts
cargo run -q -p delune-core --example typescript --features ts > "$out"
(cd web && bunx prettier@3.9.6 --no-semi --print-width 120 --write src/lib/api.generated.ts > /dev/null)
echo "wrote $out"
