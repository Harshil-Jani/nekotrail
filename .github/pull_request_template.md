## What and why

<!-- What this changes, and the problem it solves. Link the issue: "Fixes #123". -->

## How I checked it

<!-- What you ran or clicked. For UI changes, a screenshot from the app or the preview harness (preview.html). -->

## Checklist

- [ ] `bun run build` and `bun test` pass
- [ ] `cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test` passes
- [ ] User-visible changes have a line under `## Unreleased` in CHANGELOG.md
- [ ] Screenshots in `docs/screenshots/` retaken if a screen in the README changed
- [ ] No real tokens, keys or personal paths in code, tests or fixtures
