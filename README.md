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

<p align="center"><a href="https://github.com/sponsors/Harshil-Jani"><img src="docs/sponsor/github-sponsors.svg" height="40" alt="Sponsor on GitHub"></a> <a href="https://buymeacoffee.com/harshiljani"><img src="docs/sponsor/buy-me-a-coffee.svg" height="40" alt="Buy me a coffee"></a></p>

---

nekotrail is an open-source, local-first desktop app for running coding agents in parallel: Claude Code, Codex, Gemini CLI, OpenCode and 12 more, sixteen in all. Every task gets a **workspace**: a git worktree on its own branch, a chat with the agent, a diff view, a live preview of your app, a real terminal, notes, and a PR bar with live GitHub status.

**Moving from Conductor?** nekotrail imports your repos, active workspaces and chats from the Conductor app on your Mac, then moves your worktrees out of Conductor's folder into its own with `git worktree move`, and checks each one. It uses the same keyboard shortcuts as Conductor, and imported chats continue the same agent session. See [Moving from Conductor](docs/MOVING-FROM-CONDUCTOR.md) for what comes over, what doesn't, and Conductor Cloud.

**The night sky.** Every workspace is named after a star in one of eight cat constellations. Starting a workspace lights its star; merging its PR makes it shine, and lines appear between neighbouring stars once both have shipped. Ship enough and a whole cat appears in your sky. One of them, Felis, is a real constellation: astronomer Jérôme Lalande drew a cat among the stars in 1799.

## Tour

Screenshots use sample data from `preview.html`. Light and dark follow your Mac, or pick one in Settings.

**Start a workspace.** Type the task and press Enter. Pick any of the sixteen agents, or Claude + Codex to race them. Start from a GitHub or Linear issue, an open PR, an existing branch, or on top of another workspace.
<img src="docs/screenshots/new-workspace.png" width="900" alt="New workspace panel with the task box, the Claude, Codex, Gemini, OpenCode and Claude + Codex picker, and sources">

**Choose the model and how hard it thinks.** One picker for the model and its intelligence level (⌘/).
<img src="docs/screenshots/model-picker.png" width="900" alt="Model and intelligence picker in the chat box">

**Approve the plan, or split it.** Approve with one click (⌘⇧↵), or split a numbered plan into parallel workspaces.
<img src="docs/screenshots/plan.png" width="900" alt="Plan card with Approve and build, Split into 3 workspaces, a todo list and queued messages">

**Or approve each change.** In "Ask before changes" mode, every edit and command waits for your Allow (↵), Deny (⌫) or Allow for this chat. Here it's OpenCode, which nekotrail drives over the Agent Client Protocol.
<img src="docs/screenshots/opencode.png" width="900" alt="An OpenCode chat in Ask before changes mode, waiting on an edit with Deny, Allow for this chat and Allow">

**Review the agent like a pull request.** Click a line number (shift-click or drag for more lines) to comment. Send review hands every comment to the agent in one message, with the file, line and code. Comments on lines the agent has since changed are marked Outdated. GitHub review comments show inline too.
<img src="docs/screenshots/changes.png" width="900" alt="Changes tab with draft review comments on lines, an Outdated comment, GitHub comments inline and the Send review button">

**Rewind to here.** Every message you send saves your files first. Rewind puts them back, says how many files change before it does anything, and can be undone. `.env`, `node_modules` and other ignored files are never touched.
<img src="docs/screenshots/rewind.png" width="900" alt="Rewind sheet: 2 files go back to how they were before the message, 1 commit comes off the branch">

**See your app, and mark it up.** The Preview tab (⌘4) shows your running app inside the workspace. Annotate freezes the page, lets you draw boxes and arrows and add a note, and drops it into your chat as a screenshot the agent can see. Preview only loads local addresses.
<img src="docs/screenshots/preview.png" width="900" alt="Preview tab in Annotate mode with a box and an arrow drawn on a sample app and a note for the agent">

**Stack work on work.** Start a workspace on top of another one: its PR builds on the unmerged branch, and the PR bar links the parent. When the parent merges, one click moves it onto main, rebases it and points its PR there.
<img src="docs/screenshots/stacked.png" width="900" alt="PR bar of a stacked workspace showing On Mochi #482 merged, with a Move onto main banner">

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

**Check your MCP servers.** Type `/mcp` in any chat to see every server Claude, Codex, Gemini and OpenCode have, and copy one from one agent to another in one click. Skills share the same way.
<img src="docs/screenshots/mcp.png" width="900" alt="MCP servers and skills sheet with connection status and which agents have each one">

**Light up the night sky.** Every workspace lights a star; every merged PR makes it shine.
<img src="docs/screenshots/night-sky.png" width="900" alt="Night sky with constellations">

**First launch checks your setup.** It shows the command for anything missing and offers the Conductor import, which moves your worktrees out of Conductor's folder.
<img src="docs/screenshots/welcome.png" width="900" alt="Welcome screen in light mode with the setup check and Conductor import">

## Why

- **Open source and local.** MIT licensed. It runs on your machine and talks only to the tools you already use. Your workspaces, branches and chats stay on your disk, in folders you own.
- **Bring any agent.** It drives sixteen agents through their official CLIs, with your own logins, settings and subscriptions: Claude Code, Codex, Gemini CLI, OpenCode, Cursor, GitHub Copilot, Goose, Qwen Code, Kimi, Auggie, Grok, Kiro, Droid, Mistral Vibe, Cline and Kilo. Setup shows the install command for any you don't have. Agents that speak the Agent Client Protocol (ACP) are one registry entry.
- **Hand off without starting over.** When one agent runs out of usage or goes in circles, hand the task to another in the same workspace and branch, with a brief nekotrail writes from the chat and you read first.
- **Your Conductor work, kept safe.** It imports your repos, active workspaces and chats from the Conductor app on your Mac, and moves your worktrees out of Conductor's folder with `git worktree move`, checking each one after. Nothing is copied or deleted: git renames each folder. See [Moving from Conductor](docs/MOVING-FROM-CONDUCTOR.md).

## What it does

| | |
|---|---|
| Start | Open a local repo, clone one of your GitHub repos, or import from Conductor (it shows what it will bring over and move first). Start a workspace from a task, a GitHub or Linear issue, an open PR or a branch. |
| Workspaces | One git worktree and branch per task, named after the task. Archive removes the folder and keeps the branch; restore brings it back. |
| Chat | Any of the sixteen agents, several chats per workspace. Queue messages while the agent works, stop a turn, and pick plan / auto-accept / full-auto. Type `@` to mention a file; paste a screenshot to attach it. |
| Bake-off | Pick "Claude + Codex" and each gets the same task in its own workspace. Compare files, turns, time and cost side by side, keep the better one. |
| Hand off | Another agent carries on the task in the same workspace and branch (a chat's + menu, ⌘K or /handoff). You read and edit the brief before it is sent, and the new chat keeps this chat's mode. |
| Linear | Connect Linear in Settings, Integrations, and start a workspace from one of your issues: Linear's branch name, the issue in the first message, and "Fixes ENG-123" in the PR. |
| Second opinion | One click asks the other agent to review the branch read-only. Its `file:line` findings become review comments in Changes, ready to send back. |
| Checkpoints | Every message you send saves your files first. Hover it and click **Rewind to here** to put them back, including untracked and staged files. It tells you how many files change first, and Undo brings the newer state back. Each workspace keeps its last 50. |
| Changes | Everything the agent changed versus the target branch, including uncommitted and new files, with line numbers. Comment on any line or range and send all your comments to the agent as one review; comments on lines it has since changed are marked Outdated. Discard one file (the old copy goes to the Trash) and filter long file lists. |
| Preview | Your running app inside the workspace (⌘4), localhost only. Annotate draws boxes, arrows and a note on a frozen page and attaches it to the chat. Nothing running? Start the Run script from Preview. |
| Stacks | Start a workspace on top of another. The PR bar links the parent, and when the parent merges, one click moves it onto main and retargets its PR. The sidebar marks workspaces that change the same files and suggests which to merge first. |
| Terminal | A real login shell in the workspace folder. |
| PRs | One button for the next step: the agent opens the PR, fixes failing checks from their logs, or resolves conflicts; then squash and merge. Also commit and push, pull latest, agent review, and GitHub review comments shown in the diff. Start a workspace from any open PR. |
| Scripts | Setup, run and archive scripts per repo, each in its own terminal tab. Commit a `nekotrail.json` to share them with your team. Every workspace gets its own 10 ports (`NEKOTRAIL_PORT`, `NEKOTRAIL_PORT_1` to `_9`, and `PORT`), and the Ports popover shows what listens on them. |
| Repo settings | Instructions for every chat, review and PR prompts, `.env` files copied into new workspaces, environment variables. |
| Appearance | Light, Dark or Follow system, zoom from 80% to 150%, and your choice of monospace font and size for code and the terminal. |
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

Download the `.dmg` from the [latest release](https://github.com/Harshil-Jani/nekotrail/releases), open it and drag nekotrail to Applications. The app isn't notarized yet, so macOS blocks it on first launch. Run this once in Terminal to clear the download quarantine flag, then open it as usual:

```sh
xattr -dr com.apple.quarantine /Applications/nekotrail.app
```

**Updates.** nekotrail checks for a new release on launch and every 6 hours, and offers to update itself (⌘K → Check for updates, or Settings). It waits for your agents before restarting. If you're on 0.10.1 or earlier, install the latest release from the download link once by hand: those versions can't update themselves yet.

### Linux (new, experimental)

Since 0.14.0, each release also carries an x86_64 `.deb` and `.AppImage`. This is new and has not been tested on real Linux machines yet, so expect rough edges and please [report them](https://github.com/Harshil-Jani/nekotrail/issues/new/choose).

- **Debian, Ubuntu and similar:** download the `.deb` and run `sudo apt install ./nekotrail_*_amd64.deb`.
- **Other distros:** download the `.AppImage`, make it executable (`chmod +x nekotrail_*.AppImage`) and run it. It needs glibc 2.35 or newer (Ubuntu 22.04 era).
- Both update themselves from inside the app. A `.deb` update asks for your password, since it installs with `pkexec`.
- Shortcuts use Ctrl where the Mac uses ⌘. Two moves: switching workspace is Alt+1 to Alt+9 (Ctrl+1 to Ctrl+4 switch tabs), and Open in editor is Ctrl+Shift+O (Ctrl+O shows tool output). In the terminal, Ctrl keys go to your shell; copy and paste are Ctrl+Shift+C and Ctrl+Shift+V. Settings, Shortcuts lists them all.
- Not on Linux yet: the Conductor import (Conductor is Mac-only), vibrancy, and the Dock badge (most Linux desktops don't show one). Annotate in Preview uses WebKitGTK's own snapshot and is the least tested part.
- Requirements as below, plus WebKitGTK 4.1 (`libwebkit2gtk-4.1-0`, pulled in by the `.deb`).

nekotrail checks your setup on first launch and gives you the exact command for anything missing. Requirements: macOS (or Linux, see above), [`git`](https://git-scm.com), [`gh`](https://cli.github.com) signed in, and at least one agent CLI, such as [`claude`](https://docs.anthropic.com/claude-code), [`codex`](https://github.com/openai/codex), [`gemini`](https://github.com/google-gemini/gemini-cli) or [`opencode`](https://opencode.ai). Setup lists all sixteen with their install commands.

To build it yourself:

```sh
git clone https://github.com/Harshil-Jani/nekotrail
cd nekotrail
bun install
bun run tauri build -c '{"bundle":{"createUpdaterArtifacts":false}}'   # app bundle in src-tauri/target/release/bundle
# (release builds also sign an update archive; that needs the maintainers' updater key)
# or, while hacking on it:
bun run tauri dev
```

## Keyboard

Conductor's shortcuts still work, so muscle memory carries over. The full list is in Settings (⌘,), under Shortcuts, and [Moving from Conductor](docs/MOVING-FROM-CONDUCTOR.md) maps Conductor's concepts to nekotrail's.

| Keys | Action |
|---|---|
| ⌘K | Command palette: jump to any workspace, run any action |
| ⌘N / ⌘⇧N | New workspace / from a pull request or branch |
| ⌃1 to ⌃9, ⌘⌥↑ / ⌘⌥↓ | Switch workspace |
| ⌘⌥L | Next workspace with news |
| ⌘1 / ⌘2 / ⌘3 / ⌘4 | Chat / Changes / Terminal / Preview (⌘L, ⌘⇧D and ⌘J also work) |
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

Committed values win over the ones saved in the app. Because anyone who can push to the repo or open a PR
can change this file, nekotrail shows you its commands and runs them only after you approve them. It asks
again whenever they change. An archive command you never approved is skipped.

The same goes for agent config a repo commits that would run commands as soon as a chat starts: hooks,
`apiKeyHelper` and `env` in `.claude/settings.json`, servers in `.mcp.json`, and `mcp_servers`,
`shell_environment_policy.set` and `.codex/rules` for Codex, hooks, MCP servers and risky `.env` variables for
Gemini, and MCP servers, plugins, custom tools, formatters and `allow` permissions for OpenCode. Claude and
Codex skip their own trust prompt when run headless, so nekotrail lists what would run before the first message.
Until you trust it, Claude, Codex and OpenCode run with that repo config off (`claude --setting-sources user`,
Codex with the folder marked untrusted), and Gemini asks before it runs, while your own settings and MCP
servers stay on.

## Ports for each workspace

Five workspaces running the same app no longer fight over port 3000. Each active workspace holds its own
block of 10 ports (4100 to 4109 for the first, 4110 to 4119 for the next, and so on). The block stays the same
across restarts, skips ports something already listens on, and is given back when you archive the workspace.

Terminals, setup and run scripts and agent turns get `NEKOTRAIL_PORT` (the first port), `NEKOTRAIL_PORT_1` to
`NEKOTRAIL_PORT_9`, and `PORT`. A `PORT` you export in your shell or set in Repo settings is never replaced.
To give ports names, add them to `nekotrail.json` in the order you want them:

```json
{ "ports": { "web": "PORT", "api": "API_PORT" } }
```

Here web gets the first port as `PORT` and api the second as `API_PORT`. Only variables whose name contains
`PORT` can be set this way, so a repo can't use it to change anything else.

The Preview tab opens the workspace's port until the Run script prints another address. The Ports button in
the toolbar lists every workspace's ports and what listens on them, opens one in your browser or the Preview
tab, and frees a port by stopping its server, after you confirm, only if that workspace's own terminal, script
or agent started it. If a dev server ignores `PORT` and lands on its default (or finds another workspace
already there), the Preview tab and the popover say so and show the fix for that server.

## Develop

- `src-tauri/src`: Rust backend (worktrees, agent runner, PTY terminals, SQLite store, Conductor importer). Run `cargo test` in `src-tauri`.
- `src`: React UI. Open `http://localhost:1420/preview.html?ws=w1` (or `?view=sky`) during `bun run tauri dev` to see the UI with sample data in any browser.
- `PRODUCT.md`: who it's for and the product principles. `DESIGN.md`: the visual system.
- [docs/ROADMAP.md](docs/ROADMAP.md): what's next, and what we're not building.
- [CONTRIBUTING.md](CONTRIBUTING.md): setup, the checks a PR needs, and how releases work.

## Releases

Every batch of work ships as a numbered version. See [CHANGELOG.md](CHANGELOG.md) for what changed in each one, and [Releases](https://github.com/Harshil-Jani/nekotrail/releases) for the app downloads. Maintainers release with `scripts/release.sh <version>` after adding the version's section to the changelog.

## Status

Early (0.21). Built and used daily on macOS. Linux x86_64 builds are new and experimental. Feedback and PRs welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

## Sponsor

nekotrail is free, open source and local-first, with no account and no paid tier. If it saves you time, you can support its development on [GitHub Sponsors](https://github.com/sponsors/Harshil-Jani) or with a one-off coffee on [Buy Me a Coffee](https://buymeacoffee.com/harshiljani). In the app: Settings, next to the version, or "Sponsor nekotrail" and "Buy me a coffee" in ⌘K.

<p><a href="https://github.com/sponsors/Harshil-Jani"><img src="docs/sponsor/github-sponsors.svg" height="40" alt="Sponsor on GitHub"></a> <a href="https://buymeacoffee.com/harshiljani"><img src="docs/sponsor/buy-me-a-coffee.svg" height="40" alt="Buy me a coffee"></a></p>

## License

MIT. The open-source components it ships with, and their licenses, are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). nekotrail is an independent project and is not affiliated with Conductor, Anthropic, OpenAI, Google or OpenCode.
