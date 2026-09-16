#!/bin/sh
# Install delune, then set it up.
#
#   curl -fsSL https://raw.githubusercontent.com/PndaMan/delune/main/install.sh | sh
#
# Installs `delune` (the server) and `delune-tui` (the terminal client) from the
# latest release into ~/.local/bin, or /usr/local/bin when run as root, checks the
# download against its published checksum, and starts `delune setup`.
#
#   DELUNE_VERSION=v0.1.0   install that release instead of the latest
#   DELUNE_BIN_DIR=/opt/bin install somewhere else
#   DELUNE_NO_SETUP=1       only install; run `delune setup` yourself later
#   DELUNE_DOWNLOAD_BASE=…  fetch release files from a mirror instead of GitHub
set -eu

repo="PndaMan/delune"

if [ -t 1 ]; then
  accent=$(printf '\033[38;2;174;184;255m'); ok=$(printf '\033[38;2;111;211;155m')
  bad=$(printf '\033[38;2;255;122;133m'); muted=$(printf '\033[38;2;139;145;168m')
  bold=$(printf '\033[1m'); reset=$(printf '\033[0m')
else
  accent=""; ok=""; bad=""; muted=""; bold=""; reset=""
fi

say() { printf '%s\n' "$*"; }
step() { printf '  %s✓%s %s\n' "$ok" "$reset" "$*"; }
fail() { printf '  %s✗%s %s\n' "$bad" "$reset" "$*" >&2; exit 1; }

say ""
say "  ${accent}☾${reset} ${bold}delune${reset} ${muted}— find music on Soulseek, review it, add it to Navidrome${reset}"
say ""

need() { command -v "$1" >/dev/null 2>&1 || fail "$1 is needed to install delune."; }
need uname
need tar
if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1"; }
  fetch_to() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO- "$1"; }
  fetch_to() { wget -qO "$2" "$1"; }
else
  fail "curl or wget is needed to install delune."
fi

case "$(uname -s)" in
  Linux) os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) fail "delune has builds for Linux and macOS. On $(uname -s), build it from source: cargo install --git https://github.com/$repo delune delune-tui" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  aarch64 | arm64) arch="aarch64" ;;
  *) fail "No build for $(uname -m) yet. Build from source: cargo install --git https://github.com/$repo delune delune-tui" ;;
esac
target="$arch-$os"
step "This machine is $target"

version="${DELUNE_VERSION:-}"
if [ -z "$version" ]; then
  version=$(fetch "https://api.github.com/repos/$repo/releases/latest" 2>/dev/null |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
fi
if [ -z "$version" ]; then
  say ""
  say "  ${muted}There's no release to download yet. Build it from source instead:${reset}"
  say "    cargo install --git https://github.com/$repo delune delune-tui"
  say "  ${muted}then run${reset} delune setup"
  exit 1
fi
step "Latest release is $version"

if [ -n "${DELUNE_BIN_DIR:-}" ]; then
  bin_dir="$DELUNE_BIN_DIR"
elif [ "$(id -u)" = "0" ]; then
  bin_dir="/usr/local/bin"
else
  bin_dir="$HOME/.local/bin"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
name="delune-$version-$target"
base="${DELUNE_DOWNLOAD_BASE:-https://github.com/$repo/releases/download/$version}"
fetch_to "$base/$name.tar.gz" "$tmp/$name.tar.gz" || fail "Couldn't download $name.tar.gz."
fetch_to "$base/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" || fail "Couldn't download its checksum."

expected=$(cut -d' ' -f1 <"$tmp/$name.tar.gz.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$name.tar.gz" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d' ' -f1)
fi
[ "$expected" = "$actual" ] || fail "The download doesn't match its checksum; not installing it."
step "Downloaded and verified"

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$bin_dir"
for program in delune delune-tui; do
  if [ -f "$tmp/$name/$program" ]; then
    install -m 755 "$tmp/$name/$program" "$bin_dir/$program" 2>/dev/null ||
      cp "$tmp/$name/$program" "$bin_dir/$program"
    chmod 755 "$bin_dir/$program"
  fi
done
step "Installed delune and delune-tui to $bin_dir"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) say "    ${muted}$bin_dir isn't on your PATH; add it in your shell's profile.${reset}" ;;
esac

if [ -n "${DELUNE_NO_SETUP:-}" ]; then
  say ""
  say "  Run ${accent}delune setup${reset} when you're ready."
  exit 0
fi

# The script itself usually arrives on stdin through a pipe; the wizard needs the terminal.
if [ -r /dev/tty ]; then
  say ""
  "$bin_dir/delune" setup </dev/tty >/dev/tty 2>&1
else
  say ""
  say "  Run ${accent}delune setup${reset} in a terminal to finish."
fi
