# Contributing to nekotrail

Thanks for wanting to help. Bug reports, small fixes, docs and new agent support are all welcome. For anything bigger than a bug fix, open an issue first so we can agree on the shape before you spend time on it.

## Set up

You need:

- [Bun](https://bun.sh) (the frontend, its tests and the Tauri CLI run through it; CI uses the latest release).
- Rust stable, 1.85 or newer (the crate uses the 2024 edition), with clippy: `rustup component add clippy`.
- macOS 13 or newer with the Xcode command line tools (`xcode-select --install`), or Linux x86_64 with the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). On Debian or Ubuntu:

  ```sh
  sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

- To use the app itself: `git`, [`gh`](https://cli.github.com) signed in, and at least one agent CLI (`claude`, `codex`, `gemini`, `opencode`, ...).

Then:

```sh
git clone https://github.com/Harshil-Jani/nekotrail
cd nekotrail
bun install
bun run tauri dev
```

`bun run tauri dev` starts Vite on port 1420 and opens the app with hot reload for the UI. Rust changes rebuild and restart the app.

## Where things are

| Path | What |
|---|---|
| `src/` | React and TypeScript UI. `App.tsx` holds the app state and the command palette; `components/` the views. |
| `src/preview.tsx`, `preview.html` | The preview harness: the real UI with made-up sample data, no backend. |
| `src-tauri/src/` | Rust backend: worktrees (`git.rs`), the agent runner (`agent.rs`, `acp.rs`), terminals (`pty.rs`), the SQLite store (`store.rs`), the Conductor importer (`importer.rs`). Commands are registered in `lib.rs`. |
| `src-tauri/examples/` | Small command line tools for testing parts of the backend by hand. Each file says how to run it. |
| `tests/` | Bun unit tests for the UI logic. |
| `extensions/vscode/` | The VS Code and Cursor extension. It has its own `package.json` and tests. |
| `docs/` | Screenshots, guides and the [roadmap](docs/ROADMAP.md). |
| `DESIGN.md`, `PRODUCT.md` | The visual system and the product principles. Read them before changing UI. |

## The preview harness

You can work on most of the UI in a plain browser, without the Tauri backend. While `bun run tauri dev` (or just `bun run dev`) is running, open:

- `http://localhost:1420/preview.html?ws=w1` for a workspace, with `&tab=changes`, `terminal`, `preview`, `checks` or `notes`.
- `?view=sky` for the night sky, `?view=queue`, `?view=usage`, `?settings=integrations`, `?fresh` for the first run.

The full list of states is in the comment at the top of `src/preview.tsx`. All names and numbers there are made up. The README screenshots are taken from it, so if you change a screen in a visible way, please retake its screenshot from the preview at the same size (1600 by 1000) and keep the sample data fictional.

## Before you open a PR

Run all of these from the repo root. CI runs the same checks on macOS and Linux, and a PR needs them green.

```sh
bun run build                     # type-check and build the UI
bun test                          # UI unit tests
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
```

If you touched `extensions/vscode/`:

```sh
cd extensions/vscode && bun install && bun test test/ && bun run typecheck
```

Also:

- Add a line for anything users will notice under `## Unreleased` in [CHANGELOG.md](CHANGELOG.md), written for users, not for developers.
- Keep copy plain and short. Cat puns are fine in small doses, never in errors.
- Never put real tokens, keys or personal paths in code, tests or fixtures. Use obviously fake values (`/Users/sam`, `sk-test-...`).
- One topic per PR, with a description of what changed and how you checked it. Screenshots help for UI changes.

## Adding an agent

Every agent implements the `Agent` trait in `src-tauri/src/agent.rs` and is listed in `agent::ALL`. An agent that speaks the [Agent Client Protocol](https://agentclientprotocol.com) is one `AcpAgent` entry (`src-tauri/src/acp.rs`: command, arguments, name and the repo config it reads) in `agent::ALL`, plus its line in `src/agents.ts` (with the install command setup shows) and its models in `src-tauri/src/models.json`. Add it to the README's agent list too.

## How releases work

Maintainers cut releases; contributors don't need to do anything beyond the changelog line.

1. The maintainer turns `## Unreleased` into `## X.Y.Z (date)` in CHANGELOG.md.
2. `scripts/release.sh X.Y.Z` runs the build, clippy and the Rust tests, bumps the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, commits, tags `vX.Y.Z`, pushes and creates the release page with that section as its notes.
3. The `release` workflow builds the universal macOS `.dmg` and the Linux `.deb` and `.AppImage`, signs the update archives with the updater key (a repository secret only maintainers hold) and uploads them with `latest.json`. Installed apps pick the update up from there.

Local builds can skip the update signing: `bun run tauri build -c '{"bundle":{"createUpdaterArtifacts":false}}'`.

## Reporting bugs and security issues

- Bugs: open an [issue](https://github.com/Harshil-Jani/nekotrail/issues/new/choose) and paste the output of ⌘K, "Copy diagnostics for a bug report".
- Security problems: please don't open a public issue. See [SECURITY.md](SECURITY.md).

By contributing you agree that your work is released under the [MIT License](LICENSE), and to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
