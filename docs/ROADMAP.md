# Roadmap after v0.2.0

2026-11-06. Effort: S about 2h, M half a day, L a day or more (split it). Ordered by impact on a Conductor refugee's first day, a power user with 20+ workspaces, then an investor demo.

## v0.3.0: Safe to switch today

Demo: import Conductor, move the worktrees out of its folder, every row named after its task, approve a tool call inline.

1. **Signed, notarized, auto-updating build** (L). Notarize in `.github/workflows/release.yml` (it still tells users to run `xattr`), add `tauri-plugin-updater`. Blocked on the founder's Apple ID.
2. **Move imported worktrees** (M). `importer.rs` `move_worktrees`: `git worktree move` into `~/nekotrail/workspaces`, update `workspaces.path`. Conductor's wind-down cleanup would otherwise delete user work.
3. **Task titles, not star names** (S). Set `title` from the first prompt beside `maybe_name_branch` in `lib.rs`; PR title wins later; star moves to the subtitle. Screenshots still read "Mochi, Comet, Dash".
4. **Tool approvals mid-turn** (L). Keep Claude alive per session (`--input-format stream-json`) with `--permission-prompt-tool` served by a stdio MCP shim; Approve / Deny / Always card in `Chat.tsx`. Last open PARITY item; Conductor has it.
5. **Screenshot fixes** (S). A workspace appears twice (Needs you and its repo) in `Sidebar.tsx`; orange Compare competes with green Merge; the plan card says "Plan" twice; free-text "Default model" and repo star counts survive the cut list.
6. **Small parity** (S). Discard a file and filter files in `Diff.tsx`; "Attach terminal output" from `Terminal.tsx`; `/compact` and a warning before Claude's 5 MB session limit.
7. **Copy diagnostics** (S). `doctor`, versions, last agent stderr, env redacted.

## v0.4.0: Fast at 30 workspaces

Demo: 30 workspaces across 6 repos, instant switching, Activity Monitor flat, no `gh` rate limits.

1. **One PR poller in Rust** (M). Replace `PrBar.tsx`'s 60s `pr_status` and `App.tsx`'s 5 min `pr_states` with one tokio task: one `gh api graphql` per repo for active branches, 20s while checks run, 5 min idle, paused when hidden, emitting `pr` events.
   - Done: `prpoll.rs` plus `pr_poll_loop` in `lib.rs` (a thread, since `gh` is a blocking process anyway). One query per repo covers active branches and the parents stacked workspaces sit on, 40 branches per call. Autopilot repos keep polling while hidden. The PR bar, sidebar, board, radar, stacked PRs and autopilot all read the cache; `pr_status` and `pr_states` are gone. Measured with a fake gh over 10 minutes, 20 workspaces in 4 repos: 18 gh runs before, 12 after when idle; 25 before, 66 after with CI running in one repo and autopilot in another (20 s polls, but each one refreshes every workspace in the repo).
2. **Incremental line counts** (M). `line_counts` spawns a thread and four git processes per workspace after every turn, reading untracked files whole. Recompute only the workspace whose turn ended or whose FSEvents fired (`notify`), cached by HEAD and index mtime. Fixes a bug: `git::changes_since` zips `--numstat` with `--name-status`, misaligned on renames.
3. **Cheap snapshot** (S). `workspace_costs` and `daily_costs` `json_extract` every message on each refresh. Write an indexed `turns` row on `done`.
4. **Unblock the backend** (M). One `Mutex<Store>` and blocking `Command::output()` in async commands let one slow `gh` stall everything. Use `tokio::process`, WAL and a read connection.
5. **Big chats** (S). `session_events` ships whole histories; add `before`/`limit` paging (`Chat.tsx` already renders a window).
6. **Startup budget** (S). Measure cold start; `React.lazy` NightSky, Board, Diff, xterm; defer `github_profile`, `repo_github`, `model_catalog`; `ps -A` every 15s, not 5s.
7. **Disk** (S). Size per workspace, "Free space from archived worktrees".

## v0.5.0: Agents that work as a team

Demo: paste a plan, fan it into four workspaces across Claude and Codex, autopilot fixes a red check, a scorecard shows which agent shipped more for less.

1. **Fan out a plan** (M). "Split into workspaces" on the plan card; each step becomes a workspace sharing the plan, grouped by the pair `group_id` in `store.rs`.
2. **Autopilot to merge-safe** (M). Poller events trigger: failing check sends `failing_logs`, new review comment goes to the agent. Max 3 rounds, then ting ting. Per-repo toggle.
3. **Limit handoff** (M). On a Claude usage limit (already classified), "Continue in Codex" in the same worktree with task, plan, todos and diff stat. Both directions. Only a bring-your-own-agents app can do this.
4. **Collision radar** (S). Flag workspaces editing the same files from cached file lists; suggest merge order.
5. **Agent scorecard** (M). From `turns` and bake-offs, per repo: merged PRs, first-try CI pass rate, cost per merged PR, by agent and model.
6. **Since you left** (S). After 8h away: merged, needs you, failing, in review. Local state, no model call.

## v0.6.0: Any agent, and Conductor's last gaps

Demo: one task to Claude, Codex and Gemini; click through the running app inside nekotrail, circle a bug, send it.

1. **Agent harness and ACP** (L). Turn `agent.rs` branches into an `Agent` trait, then add ACP agents (Gemini CLI, OpenCode, Cursor), found by `doctor`. Conductor lists all three.
   - Done: the `Agent` trait (`agent::ALL`, `src/agents.ts` on the UI side), and Gemini CLI as the third agent through its headless `--output-format stream-json` mode. Each turn is still one process, so queueing, Stop and resume work the same for all three.
   - Done: a generic ACP client (`acp.rs`) and OpenCode (`opencode acp`) as its first agent. Each turn is still one process: `initialize`, `session/resume` (or `session/load`, or `session/new` in the worktree), model and mode through `configOptions`, `session/prompt`, then stdin closes. The `Parser` writes its own replies to stdin, Stop sends `session/cancel`, and `session/request_permission` is the "Ask before changes" card. nekotrail offers no fs or terminal capability; the agent uses its own tools.
   - Next: Cursor and Gemini over ACP, each one `AcpAgent` entry (command, args, name, plus a repo-config check). Gemini over ACP (`gemini --acp`) would give it "Ask before changes". Then `session/fork` for forking ACP chats, and the agent's live model list (`configOptions`) in the model picker.
2. **In-app preview with annotate** (M). Web view tab for detected `localUrls`; Annotate saves a screenshot via `save_attachment` into the composer.
3. **Repo actions and prompt snippets** (M). Named commands (test, lint) in `RepoSettings.tsx` and `nekotrail.json`, in the palette, optionally after setup; snippets from `/`.
4. **Share MCP and skills across agents** (S). `McpSheet.tsx` copies between `~/.claude` and `~/.codex/config.toml`.
5. **Stacked PRs** (M). Workspace from another's branch; `PrBar.tsx` shows the stack; `gh pr edit --base` when the parent merges.
6. **Appearance** (S). Code and terminal fonts, app zoom, Mermaid in chat.

## Do not build

- **Cloud, teams, routines, remote control, mobile.** Contradicts local-first; it is what Conductor is winding down.
- **Provider and API key settings.** The CLIs own this (principle 3).
- **Linear.** Needs an account integration; `gh` issues cover it.
- **Dictation.** macOS dictation works in the composer.
- **Spotlight into the main checkout.** Overwrites user work; preview and ports cover it.
- **A real editor, extra themes, artifact marketplaces.** "Open in" hands off; hours here leave the agent loop.
- **More night-sky mechanics** until retention data says the quest keeps anyone.

## Three risks to test first

1. **Install and import trust.** Five refugees from download to first PR: past Gatekeeper, worktrees moved, back on day three?
2. **Headless turns.** Without approvals, do people accept auto-accept edits or return to the terminal? Count turns stopped to steer.
3. **Multi-agent is worth it.** Are bake-off, second opinion and fan-out reused after the first try, or dropped over doubled cost and review load?
