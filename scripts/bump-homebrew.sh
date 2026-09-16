#!/usr/bin/env bash
# Fill the Homebrew formula's version and checksums from a published release.
# Usage: scripts/bump-homebrew.sh 0.2.0
set -euo pipefail

version=${1:?usage: $0 <version, without the v>}
formula=packaging/homebrew/delune.rb
base="https://github.com/PndaMan/delune/releases/download/v$version"

sed -i.bak "s/^  version \".*\"/  version \"$version\"/" "$formula"
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  sum=$(curl -fsSL "$base/delune-v$version-$target.tar.gz.sha256" | cut -d' ' -f1)
  # The sha256 line follows the url line for each target.
  awk -v target="$target" -v sum="$sum" '
    index($0, target ".tar.gz\"") { print; getline; sub(/"[0-9a-f]+"/, "\"" sum "\""); }
    { print }
  ' "$formula" > "$formula.tmp" && mv "$formula.tmp" "$formula"
done
rm -f "$formula.bak"
echo "Updated $formula for v$version"
