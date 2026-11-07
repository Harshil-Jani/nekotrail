<p align="center"><img src="public/mascot-happy.svg" width="140" alt="NekoTrail cat"></p>

<h1 align="center">NekoTrail</h1>

<p align="center">
  <a href="https://github.com/Harshil-Jani/nekotrail/releases/latest"><img src="https://img.shields.io/github/v/release/Harshil-Jani/nekotrail?label=release&color=ff9a4d" alt="Latest release"></a>
  <a href="https://github.com/Harshil-Jani/nekotrail/actions/workflows/ci.yml"><img src="https://github.com/Harshil-Jani/nekotrail/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="CHANGELOG.md"><img src="https://img.shields.io/badge/changelog-what's%20new-555" alt="Changelog"></a>
  <img src="https://img.shields.io/badge/macOS-13%2B-555" alt="macOS 13+">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-555" alt="MIT"></a>
</p>

<p align="center"><b>Your coding agents, all at once.</b><br>Claude Code, Codex and 14 more on your logins, each in its own worktree. Free, MIT, no account.</p>

<p align="center"><img src="docs/screenshots/workspace.png" width="900" alt="NekoTrail: a workspace with a Claude chat, PR status, CI checks and the sidebar"></p>

<p align="center"><a href="https://github.com/Harshil-Jani/nekotrail/releases/latest"><b>Download for Mac</b></a> · <a href="#install">Install</a> · <a href="#tour">Take the tour</a> · <a href="CHANGELOG.md">What's new</a></p>

<p align="center"><a href="https://github.com/sponsors/Harshil-Jani"><img src="docs/sponsor/github-sponsors.svg" height="40" alt="Sponsor on GitHub"></a> <a href="https://buymeacoffee.com/harshiljani"><img src="docs/sponsor/buy-me-a-coffee.svg" height="40" alt="Buy me a coffee"></a></p>

---

Every task gets a **workspace**: a git worktree on its own branch, plus a chat, a diff, a live preview, a terminal and a PR bar with GitHub status.

**Moving from Conductor?** One import brings over your repos, active workspaces and chats, then moves your worktrees out of Conductor's folder with `git worktree move`. Same shortcuts, same agent sessions. [What comes over](docs/MOVING-FROM-CONDUCTOR.md).

## Tour

Screenshots use sample data. Light and dark follow your Mac.

**Start a workspace.** Type the task, press Enter. Start from a GitHub or Linear issue, a PR, a branch or another workspace.
<img src="docs/screenshots/new-workspace.png" width="900" alt="New workspace panel with the task box, the Claude, Codex, Gemini, OpenCode and Claude + Codex picker, and sources">

**Pick the model and how hard it thinks** (⌘/).
<img src="docs/screenshots/model-picker.png" width="900" alt="Model and intelligence picker in the chat box">

**Approve the plan, or split it** into parallel workspaces (⌘⇧↵).
<img src="docs/screenshots/plan.png" width="900" alt="Plan card with Approve and build, Split into 3 workspaces, a todo list and queued messages">

**Or approve each change.** In Ask before changes, every edit and command waits for Allow (↵) or Deny (⌫).
<img src="docs/screenshots/opencode.png" width="900" alt="An OpenCode chat in Ask before changes mode, waiting on an edit with Deny, Allow for this chat and Allow">

**Review it like a PR.** Click a line number to comment. Send review hands every comment to the agent in one message.
<img src="docs/screenshots/changes.png" width="900" alt="Changes tab with draft review comments on lines, an Outdated comment, GitHub comments inline and the Send review button">

**Rewind to here.** Every message you send saves your files first. Rewind puts them back, and can be undone.
<img src="docs/screenshots/rewind.png" width="900" alt="Rewind sheet: 2 files go back to how they were before the message, 1 commit comes off the branch">

**See your app and mark it up** (⌘4). Draw on the running page and send it to the agent as a screenshot.
<img src="docs/screenshots/preview.png" width="900" alt="Preview tab in Annotate mode with a box and an arrow drawn on a sample app and a note for the agent">

**Stack work on work.** Build on an unmerged workspace. When the parent merges, one click moves it onto main.
<img src="docs/screenshots/stacked.png" width="900" alt="PR bar of a stacked workspace showing On Mochi #482 merged, with a Move onto main banner">

**See everything on a board** (⌘⇧K). Drag to change status.
<img src="docs/screenshots/board.png" width="900" alt="Kanban board of workspaces">

**Race Claude and Codex.** Same task, two workspaces. Compare files, time and cost; keep the winner.
<img src="docs/screenshots/compare.png" width="900" alt="Compare agents: a Claude and a Codex workspace side by side">

**See which agent works for you.** Finish rate, turn time, spend and merged PRs, from your own history.
<img src="docs/screenshots/scorecard.png" width="900" alt="Agent scorecard comparing Claude and Codex">

**Get disk space back.** See each workspace's size and clean ignored build folders in one click.
<img src="docs/screenshots/storage.png" width="900" alt="Storage sheet with per-workspace sizes and Clean buttons">

**Do anything from the keyboard.** ⌘K jumps to any workspace or action.
<img src="docs/screenshots/palette.png" width="900" alt="Command palette">

**Share MCP servers.** Type `/mcp` to see every agent's servers and copy one to another agent.
<img src="docs/screenshots/mcp.png" width="900" alt="MCP servers and skills sheet with connection status and which agents have each one">

**Light up the night sky.** Every workspace is a star in a cat constellation. Merged PRs make it shine.
<img src="docs/screenshots/night-sky.png" width="900" alt="Night sky with constellations">

**First launch checks your setup.** It shows the command for anything missing and offers the Conductor import.
<img src="docs/screenshots/welcome.png" width="900" alt="Welcome screen in light mode with the setup check and Conductor import">

## Why

- **Open source and local.** MIT. Workspaces, branches and chats stay on your disk.
- **Your agents, your logins.** Sixteen agents through their official CLIs: Claude Code, Codex, Gemini CLI, OpenCode, Cursor, GitHub Copilot, Goose, Qwen Code, Kimi, Auggie, Grok, Kiro, Droid, Mistral Vibe, Cline and Kilo. No proxy, no markup.
- **Hand off without starting over.** When an agent runs out of usage or loops, another carries on in the same branch, from a brief you read first.
- **Conductor work, kept safe.** Nothing is copied or deleted: git renames each worktree's folder and NekoTrail checks it.

## What it does

| | |
|---|---|
| Start | Open a local repo, clone a GitHub repo, or import from Conductor. Start from a task, a GitHub or Linear issue, a PR or a branch. |
| Workspaces | One worktree and branch per task. Archive removes the folder and keeps the branch. |
| Chat | Any of the sixteen agents, several chats per workspace. Queue messages, `@` a file, paste a screenshot. Modes: plan, auto-accept, full-auto. |
| Compare agents | Pick "Claude + Codex": same task, two workspaces. Compare files, turns, time and cost. |
| Hand off | Another agent carries on in the same branch (+ menu, ⌘K or /handoff). You read and edit the brief first. The new chat keeps this chat's mode. |
| Linear | Start from a Linear issue: its branch name, the issue in the first message, "Fixes ENG-123" in the PR. |
| Second opinion | The other agent reviews the branch read-only. Its findings become review comments. |
| Checkpoints | Every message saves your files first. Rewind to here puts them back, including untracked files. Undo works. Last 50 per workspace. |
| Changes | Everything changed versus the target branch. Comment on lines and send them as one review. |
| Preview | Your running app inside the workspace (⌘4), localhost only. Annotate draws on it for the agent. |
| Stacks | Build on another workspace. When the parent merges, one click moves it onto main. |
| Terminal | A real login shell in the workspace folder. |
| PRs | One button for the next step: open the PR, fix failing checks, resolve conflicts, then squash and merge. |
| Scripts | Setup, run and archive scripts per repo. Share them with a committed `nekotrail.json`. Each workspace gets its own 10 ports. |
| Repo settings | Instructions for every chat, review and PR prompts, `.env` files copied into new workspaces. |
| Night sky | 50 stars in 8 cat constellations. Merged PRs make them shine. |

### The cat

The cat shows the real state of your work:

- **Tail wagging:** an agent is working.
- **Asleep:** nothing happened for five minutes.
- **Worried:** CI is failing or a turn errored.
- **Happy:** a PR merged.
- **Brings you something:** an agent finished while you were away. ⌘⌥L jumps there.

## Install

Download the `.dmg` from the [latest release](https://github.com/Harshil-Jani/nekotrail/releases), open it and drag NekoTrail to Applications. The app isn't notarized yet, so macOS blocks it on first launch. Run this once in Terminal to clear the download quarantine flag, then open it as usual:

```sh
xattr -dr com.apple.quarantine /Applications/nekotrail.app
```

**Updates.** NekoTrail checks for a new release on launch and every 6 hours, and offers to update itself (⌘K → Check for updates, or Settings). It waits for your agents before restarting. If you're on 0.10.1 or earlier, install the latest release from the download link once by hand: those versions can't update themselves yet.

### Linux (new, experimental)

Since 0.14.0, each release also carries an x86_64 `.deb` and `.AppImage`. This is new and has not been tested on real Linux machines yet, so expect rough edges and please [report them](https://github.com/Harshil-Jani/nekotrail/issues/new/choose).

- **Debian, Ubuntu and similar:** download the `.deb` and run `sudo apt install ./nekotrail_*_amd64.deb`.
- **Other distros:** download the `.AppImage`, make it executable (`chmod +x nekotrail_*.AppImage`) and run it. It needs glibc 2.35 or newer (Ubuntu 22.04 era).
- Both update themselves from inside the app. A `.deb` update asks for your password, since it installs with `pkexec`.
- Shortcuts use Ctrl where the Mac uses ⌘. Two moves: switching workspace is Alt+1 to Alt+9 (Ctrl+1 to Ctrl+4 switch tabs), and Open in editor is Ctrl+Shift+O (Ctrl+O shows tool output). In the terminal, Ctrl keys go to your shell; copy and paste are Ctrl+Shift+C and Ctrl+Shift+V. Settings, Shortcuts lists them all.
- Not on Linux yet: the Conductor import (Conductor is Mac-only), vibrancy, and the Dock badge (most Linux desktops don't show one). Annotate in Preview uses WebKitGTK's own snapshot and is the least tested part.
- Requirements as below, plus WebKitGTK 4.1 (`libwebkit2gtk-4.1-0`, pulled in by the `.deb`).

NekoTrail checks your setup on first launch and gives you the exact command for anything missing. Requirements: macOS (or Linux, see above), [`git`](https://git-scm.com), [`gh`](https://cli.github.com) signed in, and at least one agent CLI, such as [`claude`](https://docs.anthropic.com/claude-code), [`codex`](https://github.com/openai/codex), [`gemini`](https://github.com/google-gemini/gemini-cli) or [`opencode`](https://opencode.ai). Setup lists all sixteen with their install commands.

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

Conductor's shortcuts still work, so muscle memory carries over. The full list is in Settings (⌘,), under Shortcuts, and [Moving from Conductor](docs/MOVING-FROM-CONDUCTOR.md) maps Conductor's concepts to NekoTrail's.

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
can change this file, NekoTrail shows you its commands and runs them only after you approve them. It asks
again whenever they change. An archive command you never approved is skipped.

The same goes for agent config a repo commits that would run commands as soon as a chat starts: hooks,
`apiKeyHelper` and `env` in `.claude/settings.json`, servers in `.mcp.json`, and `mcp_servers`,
`shell_environment_policy.set` and `.codex/rules` for Codex, hooks, MCP servers and risky `.env` variables for
Gemini, and MCP servers, plugins, custom tools, formatters and `allow` permissions for OpenCode. Claude and
Codex skip their own trust prompt when run headless, so NekoTrail lists what would run before the first message.
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

NekoTrail is free, open source and local-first, with no account and no paid tier. If it saves you time, you can support its development on [GitHub Sponsors](https://github.com/sponsors/Harshil-Jani) or with a one-off coffee on [Buy Me a Coffee](https://buymeacoffee.com/harshiljani). In the app: Settings, next to the version, or "Sponsor NekoTrail" and "Buy me a coffee" in ⌘K.

<p><a href="https://github.com/sponsors/Harshil-Jani"><img src="docs/sponsor/github-sponsors.svg" height="40" alt="Sponsor on GitHub"></a> <a href="https://buymeacoffee.com/harshiljani"><img src="docs/sponsor/buy-me-a-coffee.svg" height="40" alt="Buy me a coffee"></a></p>

## License

MIT. The open-source components it ships with, and their licenses, are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). NekoTrail is an independent project and is not affiliated with Conductor, Anthropic, OpenAI, Google or OpenCode.
