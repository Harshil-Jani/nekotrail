#!/bin/sh
# Release a new version: ./scripts/release.sh 0.2.1
# 1. Write the "## 0.2.1 (date)" section in CHANGELOG.md first.
# 2. This bumps the version everywhere, commits, tags v0.2.1 and pushes.
# 3. GitHub Actions then builds the macOS app and publishes the release with those notes.
set -eu
v="${1:?usage: scripts/release.sh <version, e.g. 0.2.1>}"
cd "$(dirname "$0")/.."

case "$v" in v*) echo "Give the version without the v: ${v#v}" >&2; exit 1 ;; esac
grep -q "^## $v " CHANGELOG.md || { echo "Add a '## $v (date)' section to CHANGELOG.md first." >&2; exit 1; }
[ -z "$(git status --porcelain -- . ':!CHANGELOG.md')" ] || { echo "Commit or stash other changes first." >&2; exit 1; }
git rev-parse -q --verify "refs/tags/v$v" >/dev/null && { echo "v$v already exists." >&2; exit 1; }

# Refuse to tag anything that doesn't type-check, build and pass the tests.
echo "Checking: type-check and build..."
bun run build >/dev/null || { echo "The build or type check failed. Nothing was tagged." >&2; exit 1; }
rm -rf dist
echo "Checking: clippy (CI runs it with -D warnings)..."
(cd src-tauri && cargo clippy -q --all-targets -- -D warnings >/dev/null 2>&1) || { echo "Clippy found problems (run: cd src-tauri && cargo clippy --all-targets -- -D warnings). Nothing was tagged." >&2; exit 1; }
echo "Checking: Rust tests..."
(cd src-tauri && cargo test -q >/dev/null 2>&1) || { echo "Rust tests failed. Nothing was tagged." >&2; exit 1; }
# The real app, driven like a user: chats, turns, the queue, terminals, Stop, a crash (scripts/e2e.sh).
if [ "$(uname -s)" = "Darwin" ]; then
  echo "Checking: end-to-end journeys on the real app (about a minute)..."
  e2e_log="$(mktemp)"
  scripts/e2e.sh >"$e2e_log" 2>&1 || { tail -40 "$e2e_log" >&2; echo "The end-to-end journeys failed (full log: $e2e_log). Nothing was tagged." >&2; exit 1; }
  grep "^e2e: all" "$e2e_log"
  rm -f "$e2e_log"
  rm -rf dist
else
  echo "WARNING: the end-to-end journeys run on macOS only and were SKIPPED. This release was not tested end to end." >&2
fi
echo "Before you tag: the 5-minute manual pass in docs/RELEASE-CHECKLIST.md, on the installed app."

sed -i '' -E "s/(\"version\": \")[^\"]+/\1$v/" package.json src-tauri/tauri.conf.json
# Only the first `version =` in Cargo.toml (the package's own).
sed -i '' -E "1,/^version = /s/^version = \"[^\"]+\"/version = \"$v\"/" src-tauri/Cargo.toml
(cd src-tauri && cargo update -p nekotrail --offline -q 2>/dev/null || cargo metadata --format-version 1 >/dev/null)

git add CHANGELOG.md package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -qm "Release v$v"
git tag -a "v$v" -m "NekoTrail v$v"
git push -q origin HEAD:main "v$v"

# Create the release page now with our own gh login. Since v0.13.0 the workflow's token is refused
# when it tries to create a release ("Resource not accessible by integration"); tauri-action then
# finds this page by its tag and only uploads the app and latest.json to it.
notes="$(mktemp)"
awk -v v="$v" '$0 ~ "^## "v" " {on=1; next} /^## / {on=0} on' CHANGELOG.md > "$notes"
printf '\n### Install\nDownload the `.dmg` below, open it and drag NekoTrail to Applications. The app is not notarized yet, so macOS blocks it on first launch. Run `xattr -dr com.apple.quarantine /Applications/nekotrail.app` once in Terminal, then open it as usual.\n' >> "$notes"
gh release create "v$v" --title "NekoTrail v$v" --notes-file "$notes" --verify-tag >/dev/null \
  || echo "Couldn't create the release page; the workflow will try. Check https://github.com/Harshil-Jani/nekotrail/releases" >&2
rm -f "$notes"
echo "Pushed v$v. The macOS build takes about 15 minutes: https://github.com/Harshil-Jani/nekotrail/actions"
