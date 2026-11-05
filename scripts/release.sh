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
echo "Checking: Rust tests..."
(cd src-tauri && cargo test -q >/dev/null 2>&1) || { echo "Rust tests failed. Nothing was tagged." >&2; exit 1; }

sed -i '' -E "s/(\"version\": \")[^\"]+/\1$v/" package.json src-tauri/tauri.conf.json
# Only the first `version =` in Cargo.toml (the package's own).
sed -i '' -E "1,/^version = /s/^version = \"[^\"]+\"/version = \"$v\"/" src-tauri/Cargo.toml
(cd src-tauri && cargo update -p nekotrail --offline -q 2>/dev/null || cargo metadata --format-version 1 >/dev/null)

git add CHANGELOG.md package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -qm "Release v$v"
git tag -a "v$v" -m "nekotrail v$v"
git push -q origin HEAD:main "v$v"
echo "Pushed v$v. The macOS build takes about 15 minutes: https://github.com/Harshil-Jani/nekotrail/actions"
