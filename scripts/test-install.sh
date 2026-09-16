#!/usr/bin/env bash
# Exercise install.sh against a fake release served from a temporary folder:
# fresh install, same version again, upgrade, dry run, bad option, missing build,
# bad checksum and uninstall. Run it with the shell to test: SHELL_UNDER_TEST=dash.
set -euo pipefail
cd "$(dirname "$0")/.."

sh_under_test=${SHELL_UNDER_TEST:-sh}
work=$(mktemp -d)
trap 'kill "$server" 2>/dev/null || true; rm -rf "$work"' EXIT

target="$(uname -m | sed 's/amd64/x86_64/; s/arm64/aarch64/')-unknown-linux-musl"
[ "$(uname -s)" = Darwin ] && target="$(uname -m | sed 's/arm64/aarch64/')-apple-darwin"
for version in v0.0.1 v0.0.2; do
  name="delune-$version-$target"
  mkdir -p "$work/release/$name"
  for program in delune delune-tui; do
    printf '#!/bin/sh\necho "%s %s"\n' "$program" "${version#v}" >"$work/release/$name/$program"
  done
  (cd "$work/release" && tar czf "$name.tar.gz" "$name" && { sha256sum "$name.tar.gz" 2>/dev/null || shasum -a 256 "$name.tar.gz"; } >"$name.tar.gz.sha256")
done

port=$((20000 + RANDOM % 20000))
(cd "$work/release" && exec python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1) &
server=$!
sleep 1

export DELUNE_BIN_DIR="$work/bin" DELUNE_NO_SETUP=1 NO_COLOR=1
mirror="http://127.0.0.1:$port"
failures=0
expect() {
  local want="$1" label="$2"
  shift 2
  local got=0
  "$sh_under_test" install.sh "$@" >"$work/out" 2>&1 || got=$?
  if [ "$got" -eq "$want" ]; then
    echo "ok   $label"
  else
    echo "FAIL $label (exit $got, wanted $want)"
    sed 's/^/     /' "$work/out"
    failures=$((failures + 1))
  fi
}
has() { grep -q "$1" "$work/out" || { echo "FAIL output lacks: $1"; failures=$((failures + 1)); }; }

expect 0 "fresh install" --version 0.0.1 --mirror "$mirror"
[ "$("$work/bin/delune" --version)" = "delune 0.0.1" ] || { echo "FAIL installed version"; failures=$((failures + 1)); }
expect 0 "same version again" --version v0.0.1 --mirror "$mirror"
has "already installed"
expect 0 "dry-run upgrade" --version v0.0.2 --mirror "$mirror" --dry-run
[ "$("$work/bin/delune" --version)" = "delune 0.0.1" ] || { echo "FAIL dry run changed something"; failures=$((failures + 1)); }
expect 0 "upgrade" --version v0.0.2 --mirror "$mirror"
[ "$("$work/bin/delune" --version)" = "delune 0.0.2" ] || { echo "FAIL upgraded version"; failures=$((failures + 1)); }
expect 1 "unknown option" --nope
expect 1 "missing build" --version v9.9.9 --mirror "$mirror"
echo "0000 x" >"$work/release/delune-v0.0.1-$target.tar.gz.sha256"
expect 1 "bad checksum" --version v0.0.1 --mirror "$mirror" --force
has "doesn't match"
expect 0 "uninstall" --uninstall
[ ! -e "$work/bin/delune" ] || { echo "FAIL uninstall left delune"; failures=$((failures + 1)); }
expect 0 "help" --help

[ "$failures" -eq 0 ] && echo "All install.sh checks passed ($sh_under_test)." || exit 1
