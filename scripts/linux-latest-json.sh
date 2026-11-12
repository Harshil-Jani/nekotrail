#!/bin/sh
# Adds the Linux packages to the updater's latest.json and checks the result. Used by the publish job in
# .github/workflows/release.yml, and by its dry run against a stand-in latest.json.
#
#   scripts/linux-latest-json.sh <latest.json> <notes.md> <tag> <dir with the .deb, .rpm and .sig files>
#
# Writes the new latest.json to stdout. The updater looks up {os}-{arch}-{bundle} first, and the app knows
# whether it was installed from the .deb or the .rpm, so each package gets its own key:
#   linux-x86_64-deb, linux-x86_64-rpm, linux-aarch64-deb, linux-aarch64-rpm
# The bare linux-x86_64 / linux-aarch64 keys are left out on purpose. Only an install that is neither a
# .deb nor an .rpm falls back to them (an old AppImage, a source build), and handing that a .deb would
# fail or, with older updaters, write a package over the AppImage. Without the key it finds no update
# and keeps running. AppImage keys are dropped for the same reason.
set -eu
in="${1:?latest.json}"
notes="${2:?notes.md}"
tag="${3:?tag, e.g. v0.27.0}"
dir="${4:?directory with the Linux packages}"
v="${tag#v}"
base="https://github.com/${GITHUB_REPOSITORY:-Harshil-Jani/nekotrail}/releases/download/$tag"

# key and file name, one per line. The names are what tauri's bundler writes (rpm release is 1).
entries="linux-x86_64-deb nekotrail_${v}_amd64.deb
linux-x86_64-rpm nekotrail-${v}-1.x86_64.rpm
linux-aarch64-deb nekotrail_${v}_arm64.deb
linux-aarch64-rpm nekotrail-${v}-1.aarch64.rpm"

json="$(jq --rawfile notes "$notes" '.notes = ($notes | sub("\\s+$"; ""))
  | .platforms |= with_entries(select(.key | startswith("linux") | not))' "$in")"

echo "$entries" | {
  while read -r key file; do
    [ -s "$dir/$file" ] || { echo "missing $dir/$file" >&2; exit 1; }
    [ -s "$dir/$file.sig" ] || { echo "missing $dir/$file.sig" >&2; exit 1; }
    json="$(printf '%s' "$json" | jq --arg k "$key" --arg url "$base/$file" --rawfile sig "$dir/$file.sig" \
      '.platforms[$k] = {signature: $sig, url: $url}')"
  done
  printf '%s\n' "$json"
} > "$in.linux"

# The checks. macOS stays as the macOS job left it, every Linux key is signed and points at its own kind of
# package, and there are no other Linux keys.
for p in darwin-aarch64 darwin-x86_64 linux-x86_64-deb linux-x86_64-rpm linux-aarch64-deb linux-aarch64-rpm; do
  jq -e --arg p "$p" '.platforms[$p].url and (.platforms[$p].signature | length > 0)' "$in.linux" >/dev/null \
    || { echo "latest.json has no signed $p entry" >&2; cat "$in.linux" >&2; exit 1; }
done
jq -e '([.platforms | to_entries[] | select(.key | startswith("linux"))
          | (.key | split("-") | last) as $kind | .value.url | endswith("." + $kind)] | all)
       and ([.platforms | keys[] | select(startswith("linux"))]
          == ["linux-aarch64-deb", "linux-aarch64-rpm", "linux-x86_64-deb", "linux-x86_64-rpm"])' "$in.linux" >/dev/null \
  || { echo "latest.json points Linux at the wrong files" >&2; cat "$in.linux" >&2; exit 1; }
jq -e --arg v "$v" '.version == $v or .version == ("v" + $v)' "$in.linux" >/dev/null \
  || { echo "latest.json version does not match $tag" >&2; exit 1; }

cat "$in.linux"
rm -f "$in.linux"
