#!/bin/sh
# Install or update NekoTrail on macOS from the latest GitHub release.
#
#   curl -fsSL https://harshil-jani.github.io/nekotrail/install.sh | sh
#
# What it does, and nothing else:
#   1. asks GitHub for the latest release and downloads its .dmg
#   2. copies NekoTrail into /Applications (replacing an older copy only after the new one is in place)
#   3. opens it
# A file downloaded with curl is not marked as "from the internet", so macOS opens the app without the
# xattr step the browser download needs. The app is not notarized yet; read this script before you run it.
set -eu

REPO="Harshil-Jani/nekotrail"
APP="/Applications/nekotrail.app"

[ "$(uname -s)" = "Darwin" ] || { echo "This installer is for macOS. On Linux, use the .deb from https://github.com/$REPO/releases/latest"; exit 1; }

echo "Finding the latest NekoTrail release..."
URL=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep -o '"browser_download_url": *"[^"]*_universal\.dmg"' | head -n 1 | sed 's/.*"\(https[^"]*\)"/\1/')
[ -n "$URL" ] || { echo "Could not find a .dmg in the latest release. Download it by hand: https://github.com/$REPO/releases/latest"; exit 1; }

TMP=$(mktemp -d)
MNT="$TMP/mnt"
cleanup() { hdiutil detach "$MNT" -quiet 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT

echo "Downloading $(basename "$URL")..."
curl -fL --progress-bar -o "$TMP/nekotrail.dmg" "$URL"

mkdir -p "$MNT"
hdiutil attach "$TMP/nekotrail.dmg" -nobrowse -readonly -mountpoint "$MNT" -quiet
SRC=$(find "$MNT" -maxdepth 1 -name '*.app' | head -n 1)
[ -n "$SRC" ] || { echo "The .dmg has no app in it. Download it by hand: https://github.com/$REPO/releases/latest"; exit 1; }

if pgrep -xq nekotrail; then
  echo "Quitting the running NekoTrail so it can be replaced..."
  osascript -e 'quit app "nekotrail"' 2>/dev/null || true
  sleep 2
fi

# Copy next to the old app first, then swap, so a failed copy never leaves you without NekoTrail.
NEW="/Applications/.nekotrail-new.app"
rm -rf "$NEW"
ditto "$SRC" "$NEW"
xattr -dr com.apple.quarantine "$NEW" 2>/dev/null || true
rm -rf "$APP"
mv "$NEW" "$APP"

VERSION=$(defaults read "$APP/Contents/Info" CFBundleShortVersionString 2>/dev/null || echo "?")
echo "NekoTrail $VERSION is in /Applications. Opening it..."
open "$APP"
