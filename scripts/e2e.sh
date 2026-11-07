#!/bin/sh
# End-to-end user journeys on the real app: builds a debug app, starts it with no window in a
# throwaway home folder with fake agent CLIs, and drives it like a user (tests/e2e/run.ts).
#   scripts/e2e.sh            build, then run
#   E2E_NO_BUILD=1 scripts/e2e.sh   run the last build
#   E2E_KEEP=1 scripts/e2e.sh       keep the temp folder (the app's database, its output)
# macOS only for now. The run stops itself after 4 minutes whatever happens.
set -eu
cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Darwin" ]; then
  echo "e2e: SKIPPED. The end-to-end journeys only run on macOS. Nothing was tested end to end." >&2
  exit 0
fi

target="${CARGO_TARGET_DIR:-src-tauri/target}"
if [ -z "${E2E_NO_BUILD:-}" ]; then
  echo "e2e: building the app (debug)..."
  bun run build >/dev/null 2>&1 || { echo "e2e: the frontend build failed:" >&2; bun run build >&2; exit 1; }
  (cd src-tauri && cargo build -q --features tauri/custom-protocol)
fi
exec bun tests/e2e/run.ts "$target/debug/nekotrail"
