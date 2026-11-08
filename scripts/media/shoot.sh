#!/usr/bin/env bash
# Re-shoot every screenshot, site clip, README card and GIF from the preview harness, headless.
#   scripts/media/shoot.sh            # everything (about 8 minutes)
#   scripts/media/shoot.sh shots      # just docs/screenshots + site/public/shots (and the cards)
#   scripts/media/shoot.sh clips      # just the site clips and README GIFs
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PORT="${PORT:-1441}"
export PYTHONDONTWRITEBYTECODE=1
export BASE="http://localhost:$PORT/preview.html"
export AVATARS="${AVATARS:-${TMPDIR:-/tmp}/nekotrail-avatars}"
what="${1:-all}"

for tool in ffmpeg gifski cwebp python3 bun; do
  command -v "$tool" >/dev/null || { echo "missing: $tool (brew install $tool)"; exit 1; }
done
python3 -c "import playwright, PIL" 2>/dev/null || { echo "needs: pip install playwright pillow && playwright install chromium"; exit 1; }

cd "$ROOT"
started=""
if ! curl -sf -o /dev/null "$BASE"; then
  [ -d node_modules ] || bun install
  bun run dev -- --port "$PORT" --strictPort >/dev/null 2>&1 &
  started=$!
  trap '[ -n "$started" ] && kill "$started" 2>/dev/null' EXIT
  for _ in $(seq 60); do curl -sf -o /dev/null "$BASE" && break; command sleep 0.5; done
fi

if [ "$what" = all ] || [ "$what" = shots ]; then
  python3 scripts/media/shots.py
  python3 docs/readme/build_assets.py                # cards, buttons, icons from the new screenshots
fi
if [ "$what" = all ] || [ "$what" = clips ]; then
  python3 docs/readme/clips.py                       # GIFs, plus site clips: compare, handoff, conductor, parallel, sky
fi
