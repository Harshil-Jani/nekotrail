# Launch plan

Progress (2026-11-06): P0-2, P0-3, P0-5, P0-6, P0-7, P0-8, the import preview part of P0-4, P1-1, P1-2, P1-3 (bake-off), P1-4 (spend today and 7 days, limit errors), P1-5 (second opinion into the diff), P1-7, problems 3 and 5, and most of the cut list are done. P0-1 (signing) needs the founder's Apple Developer ID. Moving imported worktrees (P0-4) is not done yet.

2026-11-06. Effort: S < 2h, M < 1 day, L > 1 day.

The core loop works and parity is real. What loses users is everything around it: first run, failures and scale. A Conductor refugee gives us 10 minutes.

## 1. Top 5 usability problems

1. **Missing tools fail silently or cryptically.** No `claude` gives a chat bubble saying `Could not start claude: No such file or directory (os error 2)` (`lib.rs:1008`). No `gh` login shows "Not signed in to gh" in the sidebar footer and a truncated `GitHub: ...` pill in the PR bar. **Fix:** see P0-2 and P0-3.
2. **Workspaces are named after stars, not tasks.** `displayName` = manual title, else the star. With 20 workspaces, "Mochi, Comet, Dash" tells you nothing. The branch gets renamed after the first message (`maybe_name_branch`) but the title does not. **Fix:** set the title from the first prompt, then from the PR title. The star moves to the subtitle.
3. **Four bars of chrome before any content.** Header (status select, Run, Open in, pin, archive), PR bar, tabs, chat tabs: about 165px. "Status: automatic" is a raw select sitting in prime space. **Fix:** put title, branch, PR pill and the primary action on one row. Status, pin, Open in and archive go into a single overflow menu and the palette.
4. **The agent starts before setup has run.** `newWorkspace` calls `api.send(prompt)` and only then starts the setup script. The agent's first `bun test` runs without `node_modules`, or it runs installs in parallel with setup. **Fix:** start setup first and queue the prompt until it exits.
5. **The review to merge path hides its reasons.** Merge is disabled with no explanation while checks run, it only does squash, and "Create PR" quietly spends agent tokens to run `git push` and `gh pr create`. **Fix:** show the reason on the disabled button ("3 checks running"), add "Merge when green" (`gh pr merge --auto`), make the merge method a repo setting, and turn archive on merge on by default with an Undo toast.

## 2. Launch blockers (P0, ranked)

| # | Item | Shape | Effort |
|---|---|---|---|
| 1 | **Signed, notarized build with auto-update.** "Right-click, Open" no longer works on current macOS; most users bounce at install. | Developer ID, notarize in GitHub Actions, `tauri-plugin-updater` with signed release JSON | L |
| 2 | **First-run setup check.** A brand-new user (no repos, no Conductor) sees a checklist: git, `gh` signed in, `claude` and/or `codex` found and logged in. Each missing item gets a copy button with the install command and "Check again". One agent is enough. Then two actions: "Open a local repo" and "Clone from GitHub". | New `doctor` command in `lib.rs` (after `load_login_env`) running `gh auth status` and `claude --version` / `codex --version`; `Setup.tsx` replaces `.welcome` in `App.tsx` | M |
| 3 | **Actionable runtime errors.** Missing CLI: a card with the install command and "Use Codex instead". Agent not logged in or session limit hit: "Sign in in the Terminal tab", with a button that opens a terminal running `claude /login`. `gh` logged out: a "Connect GitHub" button in the PR bar that runs `gh auth login` in a terminal. | Classify spawn and stderr errors in `agent.rs` into `kind: "setup"` events; render them in `Chat.tsx` and `PrBar.tsx` | M |
| 4 | **Conductor import as the hero, and safe.** Detect on first launch, preview counts, explain skips. Imported worktrees still live in Conductor's folder, so a Conductor uninstall or wind-down cleanup deletes the user's work. Offer "Move into nekotrail" (`git worktree move`) and recommend it. | `importer.rs` gains a `preview` and a `move_worktrees`; an import sheet in `App.tsx` | M |
| 5 | **Readable workspace names** (problem 2). | Set `title` next to `maybe_name_branch` in `lib.rs`; PR title on first `prStatus` | S |
| 6 | **Setup runs before the agent** (problem 4). | `App.tsx` `newWorkspace`: wait for the `pty-exit` of `:setup`, then send | S |
| 7 | **Clone from GitHub** for users who don't have the repo locally. | `clone_repo` command (`gh repo clone` into a chosen folder) plus a `gh repo list` picker | M |
| 8 | **Copy and docs truth pass.** README says ⌘J jumps to news, but ⌘J is Terminal. "v0.1" vs the v0.2 migration. Install steps. | README, `PRODUCT.md`, welcome copy | S |

## 3. Beyond Conductor (P1, by impact / effort)

| # | Feature | Why | Shape | Effort |
|---|---|---|---|---|
| 1 | **Project filter and "Needs you" group.** The filter is half built in `Sidebar.tsx`. Finish it: ⌘N defaults to the filtered repo, palette commands "Show only <repo>", and a pinned top group for unread, failing CI, plan awaiting approval and conflicts. | Power users live here | `Sidebar.tsx`, `App.tsx` commands, reuse `prStates` | S |
| 2 | **Start from a GitHub issue.** Uses the `gh` login, so no account needed. The issue body becomes the prompt and the PR links back with `Fixes #n`. | Backlog to agent in one step | New source tab in `NewWorkspace.tsx`, `repo_issues` command via `gh issue list` | S |
| 3 | **Claude vs Codex bake-off.** "Run on both" makes two sibling worktrees from one prompt. A compare view shows diff stat, tests, cost and time side by side, and "Keep this one" archives the other. | Only BYO-agent can do it; the launch story | `NewWorkspace.tsx` toggle, `create_workspace` twice with a shared `group_id` in `store.rs`, `Compare.tsx` | L |
| 4 | **Cost and limits you can trust.** Spend by day, repo and agent. Label it "API-equivalent" for subscription users. Detect "session limit reached" and show when it resets. Show Codex token counts. | Limits worry users more than dollars | `usage_summary` in `store.rs`, a meter in the sidebar foot | M |
| 5 | **Cross-agent review into the diff.** Codex reviews Claude's branch (or the reverse). `file:line` findings become inline comments in Changes, ready to send back. | Review is the parallel bottleneck | Parse the review output in `Diff.tsx` into the existing comment model; `PrBar` "Second opinion" action | M |
| 6 | **Guardrails.** Protected paths per repo (`.env`, migrations, infra), applied as Claude deny rules in the worktree and checked after Codex turns. A per-workspace log of commands the agent ran. A confirm step the first time Full auto is used. | Trust is why people stay local | `RepoSettings.tsx`, write `.claude/settings.local.json` in `create_workspace`, post-turn check in `lib.rs` | M |
| 7 | **Keyboard-only flow.** j/k in the sidebar, Enter opens, and every PR bar action in the palette with its disabled reason. | Speed | `App.tsx` key handler, `commands()` | S |

## 4. Cut list for launch

- **Notes tab:** hide; keep notes in the palette.
- **Status select in the header:** derived from the PR; override goes in the overflow.
- **Repo owner avatars and GitHub star counts** on sidebar repo heads: noise, one `gh` call per repo.
- **Pin button in the header:** move it to the overflow and the palette.
- **Drag grip on the new workspace panel:** nobody moves a modal.
- **Palette entries "Pet the cat" and "Reset zoom":** clicking the cat still pets it.
- **Star discovery toast on every new workspace:** show it once per constellation and on merges only.
- **Sidebar progress bar:** keep "10/18 stars", drop the bar.
- **Free-text model field:** replace it with a dropdown of known models plus "Other".
- **Read-only "Folder" row in Settings:** make it editable or hide it.

Delight stays in the sky and the cat. Every cut above is chrome on a working surface.
