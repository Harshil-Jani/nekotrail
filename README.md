<p align="center"><img src="public/mascot-happy.svg" width="140" alt="nekotrail cat"></p>

<h1 align="center">nekotrail</h1>

<p align="center">
  <a href="https://github.com/Harshil-Jani/nekotrail/releases/latest"><img src="https://img.shields.io/github/v/release/Harshil-Jani/nekotrail?label=release&color=ff9a4d" alt="Latest release"></a>
  <a href="https://github.com/Harshil-Jani/nekotrail/actions/workflows/ci.yml"><img src="https://github.com/Harshil-Jani/nekotrail/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="CHANGELOG.md"><img src="https://img.shields.io/badge/changelog-what's%20new-555" alt="Changelog"></a>
  <img src="https://img.shields.io/badge/macOS-13%2B-555" alt="macOS 13+">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-555" alt="MIT"></a>
</p>

<p align="center">Run many coding agents at once on your Mac. Each one gets its own branch and folder, and each one ends as a pull request.</p>

<p align="center"><img src="docs/screenshots/workspace.png" width="900" alt="nekotrail: a workspace with a Claude chat, PR status, CI checks and the sidebar"></p>

<p align="center"><a href="https://github.com/Harshil-Jani/nekotrail/releases/latest"><b>Download for Mac</b></a> · <a href="#tour">Take the tour</a> · <a href="CHANGELOG.md">What's new</a></p>

---

nekotrail is an open-source, local-first desktop app for running Claude Code and Codex in parallel. Every task gets a **workspace**: a git worktree on its own branch, a chat with the agent, a diff view, a real terminal, notes, and a PR bar with live GitHub status.

**The night sky.** Every workspace is named after a star in one of eight cat constellations. Starting a workspace lights its star; merging its PR makes it shine, and lines appear between neighbouring stars once both have shipped. Ship enough and a whole cat appears in your sky. One of them, Felis, is a real constellation: astronomer Jérôme Lalande drew a cat among the stars in 1799.

## Tour

Screenshots use sample data from `preview.html`. Light and dark follow your Mac.

**Start a workspace.** Type the task and press Enter. Pick Claude, Codex, or Both to race them. Start from an issue, an open PR or an existing branch.
<img src="docs/screenshots/new-workspace.png" width="900" alt="New workspace panel with the task box, agent picker and sources">

**Choose the model and how hard it thinks.** One picker for the model and its intelligence level (⌘/).
<img src="docs/screenshots/model-picker.png" width="900" alt="Model and intelligence picker in the chat box">

**Approve the plan, or split it.** Approve with one click (⌘⇧↵), or split a numbered plan into parallel workspaces. In "Ask before changes" mode, every edit and command waits for your Allow (↵) or Deny (⌫).
<img src="docs/screenshots/plan.png" width="900" alt="Plan card with Approve and build, a live todo list and queued messages">

**Review the changes.** Line numbers, comments on any line, GitHub review comments inline, side by side.
<img src="docs/screenshots/changes.png" width="900" alt="Changes tab with a diff, a review comment and GitHub comments">

**See everything on a board.** Backlog, In progress, In review, Done. Drag to change status (⌘⇧K).
<img src="docs/screenshots/board.png" width="900" alt="Kanban board of workspaces">

**Race Claude and Codex.** Same task, two workspaces. Compare files, time and cost; keep the winner.
<img src="docs/screenshots/compare.png" width="900" alt="Bake-off comparison of a Claude and a Codex workspace">

**See which agent works for you.** Clean-finish rate, turn time, spend and merged PRs per agent, from your own history.
<img src="docs/screenshots/scorecard.png" width="900" alt="Agent scorecard comparing Claude and Codex">

**Get disk space back.** See each workspace's size and clean the build folders git ignores, in one click.
<img src="docs/screenshots/storage.png" width="900" alt="Storage sheet with per-workspace sizes and Clean buttons">

**Do anything from the keyboard.** ⌘K jumps to any workspace or action.
<img src="docs/screenshots/palette.png" width="900" alt="Command palette">

**Check your MCP servers.** Type `/mcp` in any chat.
<img src="docs/screenshots/mcp.png" width="900" alt="MCP servers sheet with connection status">

**Light up the night sky.** Every workspace lights a star; every merged PR makes it shine.
<img src="docs/screenshots/night-sky.png" width="900" alt="Night sky with constellations">

**First launch checks your setup.** It shows the command for anything missing and offers the Conductor import.
<img src="docs/screenshots/welcome.png" width="900" alt="Welcome screen in light mode with the setup check and Conductor import">

## Why

- **Open source and local.** MIT licensed. It runs on your machine and talks only to the tools you already use. It can't be shut down or acquired away.
- **Bring your own agent.** It drives the official `claude` and `codex` CLIs with your own logins, settings and subscriptions.
- **One click from Conductor.** It imports your repos, active workspaces and chats from a local Conductor install. It only reads Conductor's data and moves nothing, and imported chats continue the same agent session.

## What it does

| | |
|---|---|
| Start | Open a local repo, clone one of your GitHub repos, or import from Conductor (it shows what it will bring over first). Start a workspace from a task, a GitHub issue, an open PR or a branch. |
| Workspaces | One git worktree and branch per task, named after the task. Archive removes the folder and keeps the branch; restore brings it back. |
| Chat | Claude Code or Codex, several chats per workspace. Queue messages while the agent works, stop a turn, and pick plan / auto-accept / full-auto. Type `@` to mention a file; paste a screenshot to attach it. |
| Bake-off | Pick "Both" and Claude and Codex each get the same task in their own workspace. Compare files, turns, time and cost side by side, keep the better one. |
| Second opinion | One click asks the other agent to review the branch read-only. Its `file:line` findings become review comments in Changes, ready to send back. |
| Checkpoints | Every message snapshots the workspace first. Hover a message and click **Rewind files to here** to undo everything the agent did from that point, commits included. |
| Changes | Everything the agent changed versus the target branch, including uncommitted and new files, with line numbers. Comment on any line and send all your comments to the agent as one review. |
| Terminal | A real login shell in the workspace folder. |
| PRs | One button for the next step: the agent opens the PR, fixes failing checks from their logs, or resolves conflicts; then squash and merge. Also commit and push, pull latest, agent review, and GitHub review comments shown in the diff. Start a workspace from any open PR. |
| Scripts | Setup, run and archive scripts per repo, each in its own terminal tab. Commit a `nekotrail.json` to share them with your team. Every workspace gets its own ports (`NEKOTRAIL_PORT` and the next 9). |
| Repo settings | Instructions for every chat, review and PR prompts, `.env` files copied into new workspaces, environment variables. |
| GitHub | Your avatar and name, repo owner avatars and stars, PR authors and reviewers, all through your `gh` login. |
| Night sky | 50 stars in 8 cat constellations. Workspaces light stars, merged PRs make them shine and draw the lines. |

### The cat

The cat reflects the real state of your work, so its moods carry information:

- **Tail wagging:** an agent is working.
- **Asleep (z z):** nothing has happened for five minutes.
- **Worried:** CI is failing or a turn ended in an error.
- **Happy:** a PR was merged. Its star starts to shine, and the cat offers to archive the workspace.
- **Brings you something:** when an agent finishes while you're elsewhere, you get a macOS notification and a Dock badge. Press ⌘⌥L to jump to the next workspace with news.
- Its eyes follow your cursor. Click it and it purrs the story of one of your constellations.
- Archiving a workspace knocks it off the table.

## Install

Download the `.dmg` from the [latest release](https://github.com/Harshil-Jani/nekotrail/releases), open it and drag nekotrail to Applications. The app isn't notarized yet, so on first launch right-click it and choose **Open**.

nekotrail checks your setup on first launch and gives you the exact command for anything missing. Requirements: macOS, [`git`](https://git-scm.com), [`gh`](https://cli.github.com) signed in, and at least one of [`claude`](https://docs.anthropic.com/claude-code) or [`codex`](https://github.com/openai/codex).

To build it yourself:

```sh
git clone https://github.com/Harshil-Jani/nekotrail
cd nekotrail
bun install
bun run tauri build      # app bundle in src-tauri/target/release/bundle
# or, while hacking on it:
bun run tauri dev
```

## Keyboard

Conductor's shortcuts still work, so muscle memory carries over. The full list is in Settings (⌘,), under Shortcuts.

| Keys | Action |
|---|---|
| ⌘K | Command palette: jump to any workspace, run any action |
| ⌘N / ⌘⇧N | New workspace / from a pull request or branch |
| ⌃1 to ⌃9, ⌘⌥↑ / ⌘⌥↓ | Switch workspace |
| ⌘⌥L | Next workspace with news |
| ⌘1 / ⌘2 / ⌘3 | Chat / Changes / Terminal (⌘L, ⌘⇧D and ⌘J also work) |
| ⌘⇧P / ⌘⇧Y / ⌘⇧M | Create PR / commit and push / merge |
| ⌘⇧R / ⌘⇧X / ⌘⇧L | Agent review / fix failing checks / pull latest |
| ⌘R / ⌘⇧B | Run or stop the app / open localhost |
| ⌘P | Open any file (and edit it) |
| ⌘F | Search the chat |
| ⌘⇧↵ | Approve the agent's plan |

## Sharing scripts with `nekotrail.json`

```json
{ "scripts": { "setup": "bun install", "run": "bun dev", "archive": "docker compose down" } }
```

Committed values win over the ones saved in the app.

## Develop

- `src-tauri/src`: Rust backend (worktrees, agent runner, PTY terminals, SQLite store, Conductor importer). Run `cargo test` in `src-tauri`.
- `src`: React UI. Open `http://localhost:1420/preview.html?ws=w1` (or `?view=sky`) during `bun run tauri dev` to see the UI with sample data in any browser.
- `PRODUCT.md`: who it's for and the product principles.
- `docs/FEATURES.md`: what's in, what's next, and what we're skipping.

## Releases

Every batch of work ships as a numbered version. See [CHANGELOG.md](CHANGELOG.md) for what changed in each one, and [Releases](https://github.com/Harshil-Jani/nekotrail/releases) for the app downloads. Maintainers release with `scripts/release.sh <version>` after adding the version's section to the changelog.

## Status

Early (v0.2). Built and used daily on macOS. Feedback and PRs welcome.

## License

MIT. nekotrail is an independent project and is not affiliated with Conductor, Anthropic or OpenAI.
