#!/bin/sh
# delune installer — https://github.com/PndaMan/delune
#
#   curl -fsSL https://raw.githubusercontent.com/PndaMan/delune/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/PndaMan/delune/main/install.sh | sh -s -- --help
#
# Installs `delune` (the server) and `delune-tui` (the terminal client) from a GitHub
# release, verifies the download against its published SHA-256, and starts the setup
# wizard. Run it again to upgrade; `--uninstall` removes it.
#
# Every option has an environment variable, for scripts and CI:
#
#   --version <tag>     DELUNE_VERSION        a specific release (default: the latest)
#   --bin-dir <dir>     DELUNE_BIN_DIR        where to install (default: ~/.local/bin,
#                                             /usr/local/bin as root)
#   --no-setup          DELUNE_NO_SETUP=1     don't start `delune setup` afterwards
#   --force             DELUNE_FORCE=1        reinstall even if this version is present
#   --yes               DELUNE_YES=1          don't ask questions (use sudo if needed)
#   --mirror <url>      DELUNE_DOWNLOAD_BASE  fetch release files from here instead
#   --uninstall                               remove delune (your data is kept)
#   --dry-run                                 say what would happen, change nothing
#   --help
#
# NO_COLOR is respected. Nothing runs as root unless the install folder needs it, and
# then only after asking.

set -eu

REPO="PndaMan/delune"
PROGRAMS="delune delune-tui"
DOCS="https://pndaman.github.io/delune"

# ---------------------------------------------------------------- output -----

if [ -t 2 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
  ACCENT=$(printf '\033[38;2;174;184;255m')
  GREEN=$(printf '\033[38;2;111;211;155m')
  RED=$(printf '\033[38;2;255;122;133m')
  YELLOW=$(printf '\033[38;2;242;193;107m')
  MUTED=$(printf '\033[38;2;139;145;168m')
  BOLD=$(printf '\033[1m')
  RESET=$(printf '\033[0m')
else
  ACCENT="" GREEN="" RED="" YELLOW="" MUTED="" BOLD="" RESET=""
fi

say() { printf '%s\n' "$*" >&2; }
step() { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*" >&2; }
note() { printf '    %s%s%s\n' "$MUTED" "$*" "$RESET" >&2; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
die() {
  printf '  %s✗%s %s\n' "$RED" "$RESET" "$*" >&2
  exit 1
}

usage() {
  cat >&2 <<EOF
Install delune and delune-tui, then set delune up.

Usage: install.sh [options]

  --version <tag>   install this release (default: the latest)     DELUNE_VERSION
  --bin-dir <dir>   install here (default: ~/.local/bin, or
                    /usr/local/bin as root)                          DELUNE_BIN_DIR
  --no-setup        don't start 'delune setup' afterwards           DELUNE_NO_SETUP=1
  --force           reinstall even if this version is installed     DELUNE_FORCE=1
  -y, --yes         don't ask questions                             DELUNE_YES=1
  --mirror <url>    download release files from here                DELUNE_DOWNLOAD_BASE
  --uninstall       remove delune (your data is kept)
  --dry-run         show what would happen without changing anything
  -h, --help        show this help

Docs: $DOCS/install/script.html
EOF
}

# --------------------------------------------------------------- options -----

VERSION="${DELUNE_VERSION:-}"
BIN_DIR="${DELUNE_BIN_DIR:-}"
NO_SETUP="${DELUNE_NO_SETUP:-}"
FORCE="${DELUNE_FORCE:-}"
YES="${DELUNE_YES:-}"
MIRROR="${DELUNE_DOWNLOAD_BASE:-}"
UNINSTALL=""
DRY_RUN=""

while [ $# -gt 0 ]; do
  case "$1" in
    --version)
      [ $# -ge 2 ] || die "--version needs a tag, like v0.1.0"
      VERSION="$2"
      shift 2
      ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --bin-dir)
      [ $# -ge 2 ] || die "--bin-dir needs a folder"
      BIN_DIR="$2"
      shift 2
      ;;
    --bin-dir=*) BIN_DIR="${1#*=}"; shift ;;
    --mirror)
      [ $# -ge 2 ] || die "--mirror needs an address"
      MIRROR="$2"
      shift 2
      ;;
    --mirror=*) MIRROR="${1#*=}"; shift ;;
    --no-setup) NO_SETUP=1; shift ;;
    --force) FORCE=1; shift ;;
    -y | --yes) YES=1; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h | --help) usage; exit 0 ;;
    *) die "Unknown option: $1 (see --help)" ;;
  esac
done

# Releases are tagged "v0.1.0"; accept "0.1.0" too.
case "$VERSION" in
  "" | v*) ;;
  *) VERSION="v$VERSION" ;;
esac

# ----------------------------------------------------------------- tools -----

has() { command -v "$1" >/dev/null 2>&1; }

if has curl; then
  fetch() { curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 15 "$1"; }
  fetch_to() { curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 15 -o "$2" "$1" 2>/dev/null; }
elif has wget; then
  fetch() { wget -q --tries=3 --timeout=30 -O- "$1"; }
  fetch_to() { wget -q --tries=3 --timeout=30 -O "$2" "$1" 2>/dev/null; }
else
  die "Installing needs curl or wget."
fi

sha256() {
  if has sha256sum; then
    sha256sum "$1" | cut -d' ' -f1
  elif has shasum; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif has openssl; then
    openssl dgst -sha256 "$1" | sed 's/.*= //'
  else
    die "Checking the download needs sha256sum, shasum or openssl."
  fi
}

# Commands that change the install folder: through sudo when it needs root, and only
# described in a dry run.
SUDO=""
run() {
  if [ -n "$DRY_RUN" ]; then
    note "would run: ${SUDO:+$SUDO }$*"
    return 0
  fi
  if [ -n "$SUDO" ]; then
    sudo "$@"
  else
    "$@"
  fi
}

can_ask() { [ -z "$YES" ] && (: </dev/tty) 2>/dev/null; }

confirm() {
  can_ask || return 0
  printf '  %s?%s %s [Y/n] ' "$ACCENT" "$RESET" "$1" >&2
  read -r answer </dev/tty || answer=""
  case "$answer" in
    "" | y | Y | yes | Yes | YES) return 0 ;;
    *) return 1 ;;
  esac
}

# -------------------------------------------------------------- platform -----

detect_target() {
  os=$(uname -s)
  arch=$(uname -m)
  case "$os" in
    Linux) os="unknown-linux-musl" ;;
    Darwin)
      os="apple-darwin"
      # A shell under Rosetta reports x86_64 on Apple silicon; use the native build.
      if [ "$arch" = "x86_64" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
        arch="arm64"
      fi
      ;;
    *) die "There are delune builds for Linux and macOS, not $os. See $DOCS/install/source.html" ;;
  esac
  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) die "There's no delune build for $arch yet. See $DOCS/install/source.html" ;;
  esac
  TARGET="$arch-$os"
}

choose_bin_dir() {
  [ -n "$BIN_DIR" ] && return 0
  if [ "$(id -u)" = "0" ]; then
    BIN_DIR="/usr/local/bin"
  else
    BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
  fi
}

# Work out whether the install folder needs root, and ask before using sudo.
check_privileges() {
  probe="$BIN_DIR"
  while [ ! -d "$probe" ]; do probe=$(dirname "$probe"); done
  [ -w "$probe" ] && return 0
  has sudo || die "$BIN_DIR isn't writable and sudo isn't available. Choose another folder with --bin-dir."
  confirm "$BIN_DIR needs root to write to. Use sudo?" ||
    die "Nothing installed. Choose a folder you own with --bin-dir."
  SUDO="sudo"
}

installed_version() {
  [ -x "$BIN_DIR/delune" ] || return 0
  "$BIN_DIR/delune" --version 2>/dev/null | awk 'NR == 1 { print $2 }'
}

profile_file() {
  case "$(basename "${SHELL:-sh}")" in
    zsh) echo "${ZDOTDIR:-$HOME}/.zshrc" ;;
    bash) if [ "$(uname -s)" = "Darwin" ]; then echo "$HOME/.bash_profile"; else echo "$HOME/.bashrc"; fi ;;
    *) echo "$HOME/.profile" ;;
  esac
}

# ------------------------------------------------------------- uninstall -----

uninstall() {
  choose_bin_dir
  check_privileges
  say "  ${BOLD}Removing delune${RESET} from $BIN_DIR"
  unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/delune.service"
  if [ -f "$unit" ]; then
    if [ -n "$DRY_RUN" ]; then
      note "would stop and remove $unit"
    else
      systemctl --user disable --now delune.service >/dev/null 2>&1 || true
      rm -f "$unit"
      systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    step "Stopped and removed your delune service"
  fi
  removed=""
  for program in $PROGRAMS; do
    if [ -e "$BIN_DIR/$program" ]; then
      run rm -f "$BIN_DIR/$program"
      removed=1
      step "Removed $BIN_DIR/$program"
    fi
  done
  [ -n "$removed" ] || note "Nothing to remove in $BIN_DIR."
  say ""
  note "Your data is kept: ${XDG_DATA_HOME:-$HOME/.local/share}/delune (or /var/lib/delune)."
  if [ -f /etc/systemd/system/delune.service ]; then
    note "A system service is still installed. To remove it:"
    note "  sudo systemctl disable --now delune && sudo rm /etc/systemd/system/delune.service"
  fi
  exit 0
}

# ------------------------------------------------------------------ main -----

say ""
say "  ${ACCENT}☾${RESET} ${BOLD}delune${RESET}  ${MUTED}find music on Soulseek, review it, add it to Navidrome${RESET}"
say ""

[ -n "$UNINSTALL" ] && uninstall

has uname || die "Installing needs uname."
has tar || die "Installing needs tar."
detect_target
choose_bin_dir
step "This machine is $TARGET"

if [ -e /etc/NIXOS ]; then
  warn "This is NixOS. The delune module is usually the better fit:"
  note "a declarative service, secrets kept out of the store, optional VPN routing."
  note "$DOCS/install/nixos.html"
  note "Carrying on with a standalone install (these builds are static, so they run here)."
fi

if [ -z "$VERSION" ]; then
  VERSION=$(fetch "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1) || true
  if [ -z "$VERSION" ]; then
    warn "There's no published release to install yet."
    note "Build it from source instead (Rust 1.90+ and Bun): $DOCS/install/source.html"
    exit 1
  fi
  step "Latest release: $VERSION"
else
  step "Release: $VERSION"
fi

current=$(installed_version)
if [ -n "$current" ] && [ "v$current" = "$VERSION" ] && [ -z "$FORCE" ]; then
  step "delune $current is already installed in $BIN_DIR (--force reinstalls it)"
else
  check_privileges

  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t delune)
  trap 'rm -rf "$tmp"' EXIT
  trap 'rm -rf "$tmp"; exit 130' INT HUP TERM

  name="delune-$VERSION-$TARGET"
  base="${MIRROR:-https://github.com/$REPO/releases/download/$VERSION}"
  base="${base%/}"

  fetch_to "$base/$name.tar.gz" "$tmp/$name.tar.gz" ||
    die "Couldn't download $name.tar.gz. Does $VERSION have a build for $TARGET?"
  fetch_to "$base/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
    die "Couldn't download the checksum for $name.tar.gz."
  expected=$(cut -d' ' -f1 <"$tmp/$name.tar.gz.sha256")
  actual=$(sha256 "$tmp/$name.tar.gz")
  if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    die "The download doesn't match its published checksum, so nothing was installed."
  fi
  step "Downloaded and verified $name"

  tar -xzf "$tmp/$name.tar.gz" -C "$tmp" || die "Couldn't unpack the download."
  for program in $PROGRAMS; do
    [ -f "$tmp/$name/$program" ] || die "This release doesn't include $program."
    chmod 755 "$tmp/$name/$program"
    # macOS quarantines downloaded files; these were checked against their checksum.
    if [ "$(uname -s)" = "Darwin" ] && has xattr; then
      xattr -d com.apple.quarantine "$tmp/$name/$program" 2>/dev/null || true
    fi
    "$tmp/$name/$program" --version >/dev/null 2>&1 ||
      die "$program from this release doesn't run on this machine; nothing was replaced."
  done

  run mkdir -p "$BIN_DIR"
  for program in $PROGRAMS; do
    # Copy next to the target and rename over it: an interrupted install never leaves
    # half a program, and a running delune keeps its file until it restarts.
    run cp "$tmp/$name/$program" "$BIN_DIR/.$program.new"
    run chmod 755 "$BIN_DIR/.$program.new"
    run mv -f "$BIN_DIR/.$program.new" "$BIN_DIR/$program"
  done
  if [ -n "$DRY_RUN" ] && [ -n "$current" ]; then
    step "Would upgrade delune $current → ${VERSION#v} in $BIN_DIR"
  elif [ -n "$DRY_RUN" ]; then
    step "Would install delune and delune-tui in $BIN_DIR"
  elif [ -n "$current" ]; then
    step "Upgraded delune $current → ${VERSION#v} in $BIN_DIR"
  else
    step "Installed delune and delune-tui in $BIN_DIR"
  fi

  # A running user service picks the upgrade up now rather than at next reboot.
  if [ -n "$current" ] && [ -z "$DRY_RUN" ] && has systemctl &&
    systemctl --user is-active --quiet delune.service 2>/dev/null; then
    systemctl --user restart delune.service && step "Restarted your delune service"
  fi
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    warn "$BIN_DIR isn't on your PATH. Add it with:"
    if [ "$(basename "${SHELL:-sh}")" = "fish" ]; then
      note "fish_add_path $BIN_DIR"
    else
      note "echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> $(profile_file)"
    fi
    ;;
esac

if [ -n "$DRY_RUN" ]; then
  say ""
  note "Dry run: nothing was changed."
  exit 0
fi

say ""
if [ -n "$current" ]; then
  say "  ${BOLD}Done.${RESET} Your settings are unchanged; ${ACCENT}delune setup${RESET} changes them."
elif [ -n "$NO_SETUP" ]; then
  say "  ${BOLD}Done.${RESET} Run ${ACCENT}delune setup${RESET} when you're ready."
elif (: </dev/tty) 2>/dev/null; then
  # This script usually arrives through a pipe, so the wizard reads the terminal directly.
  exec "$BIN_DIR/delune" setup </dev/tty >/dev/tty 2>&1
else
  say "  ${BOLD}Installed.${RESET} Run ${ACCENT}delune setup${RESET} in a terminal to finish."
fi
say "  ${MUTED}Docs: $DOCS/${RESET}"
