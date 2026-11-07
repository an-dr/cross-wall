#!/bin/sh
# Installs the latest crossWall release for this user. Needs curl and unzip.
set -eu
base=https://github.com/an-dr/cross-wall/releases/latest/download
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL "$base/cross-wall-linux-x64.json" -o "$work/manifest.json"
url=$(sed -n 's/.*"url": *"\([^"]*\)".*/\1/p' "$work/manifest.json")
sha=$(sed -n 's/.*"sha256": *"\([^"]*\)".*/\1/p' "$work/manifest.json")
curl -fsSL "$url" -o "$work/cross-wall.zip"
echo "$sha  $work/cross-wall.zip" | sha256sum -c - >/dev/null
unzip -q "$work/cross-wall.zip" -d "$work/app"
"$work/app/cross-wall-app" --install
echo "crossWall is installed. Start it from your application list."
