# Moving from Conductor

nekotrail imports your repos, workspaces and chats from the Conductor app on your Mac, then moves your
worktrees out of Conductor's folder and into its own, so nothing you haven't committed depends on
Conductor any more.

**Why now.** Meta acquired Conductor on 4 November 2026, and new downloads stopped that day. Conductor Cloud
goes read-only on 13 November 2026 and shuts down on 20 November 2026, when its data is deleted. The
Conductor app on your Mac gets critical fixes until early 2027 and then winds down, and Conductor says user
data will be deleted when it shuts down. Your Conductor worktrees live inside Conductor's folder, so
uncommitted work in them goes with it.

## The short version

1. [Download nekotrail](https://github.com/Harshil-Jani/nekotrail/releases/latest) and open it (see
   [Install](../README.md#install)).
2. On first launch it finds Conductor's data and offers **Import from Conductor**. Later, it is on the
   welcome screen and in ⌘K.
3. The import sheet lists every worktree it would move, with its repo, branch, size and how many
   uncommitted files it has. **Move into nekotrail** is picked for you. Press **Import and move**.
4. Each worktree is moved with `git worktree move` and checked. The sheet says which ones moved, and
   why any didn't.

Using Conductor Cloud? Read [Conductor Cloud](#conductor-cloud) first.

## What comes over

| From Conductor | In nekotrail |
|---|---|
| Repos | Added with their name, folder, default branch and remote. A repo you already added in nekotrail is reused. |
| Setup, run and archive scripts | The repo's scripts in nekotrail (Repo settings, Scripts). |
| Custom prompts (general, code review, create PR) | Repo settings, Agent: instructions for every chat, the review prompt and the PR prompt. |
| Files copied into new workspaces (your `.env` globs) | Repo settings, Files and env. |
| Active workspaces | One nekotrail workspace each, with its name, branch, target branch, pin and notes. |
| Claude and Codex chats | Each chat with its message history. New messages continue the same agent session (`claude --resume`, `codex exec resume`). |
| Keyboard shortcuts | Not imported, because they are already the same. See [Shortcuts](#shortcuts). |

Importing only reads Conductor's database, and running it again only adds what is new.

## What doesn't come over

- **Conductor Cloud workspaces.** nekotrail can only read the Conductor app on your Mac.
- **Archived workspaces.** They have no worktree left to open. Their branches are still in your repo, and
  New workspace (⌘⇧N) can start from any branch.
- **Hidden chats**, and workspaces whose folder is no longer on disk. The sheet counts these as skipped.
- **Conductor's own settings and account**: its theme, notifications, billing, organisation and team
  features. nekotrail has no account; your agents use their own CLI logins.
- **Conductor checkpoints.** nekotrail saves its own before each message you send from now on (Rewind to
  here).
- **On Linux**, nothing: Conductor is Mac-only, so there is no Conductor data to read there.

## Conductor Cloud

nekotrail can't read Conductor Cloud. Cloud goes read-only on 13 November 2026 and shuts down on
20 November 2026, when its data is deleted. Before then, export or transfer your Cloud workspaces to the
Conductor app on your Mac (Conductor offers this), then import them into nekotrail as above.

If a Cloud workspace's branch is pushed to GitHub, you can also start a nekotrail workspace from that
branch or its pull request with ⌘⇧N. Anything that only lived in the Cloud workspace and was never
pushed is only recoverable through Conductor's own export.

## The move, step by step

Conductor keeps each worktree inside its own folder. nekotrail moves imported worktrees into
`~/nekotrail/workspaces/<repo>/<name>` (the next free name if that one is taken).

- **What runs:** only `git worktree move <old folder> <new folder>`, from the repo. Git renames the folder
  and updates its own records, so the branch, its commits, staged and unstaged changes, untracked files,
  and ignored files such as `node_modules` and `.env` all come along.
- **What never happens:** nothing in Conductor's folder is deleted, copied over or cleaned. A worktree that
  can't move stays exactly where it was.
- **The check after each move:** `git worktree list` shows the new folder, the branch and its head commit
  are the same as just before the move, and the uncommitted files are the same files with the same
  content (git's status list, `git diff HEAD`, and each untracked file's content).
- **Results:** each worktree reads Checked, Moved with a difference to look at, or Not moved with the
  reason in plain words. **Try again** retries only the ones that didn't move.
- **Skipped for now:** a workspace with an agent working in it. Move it once the turn ends.
- **Afterwards:** Conductor no longer finds these folders. Your work
  is in nekotrail's folder.

Reasons a move can fail, and what to do:

| Reason | What to do |
|---|---|
| Git has the worktree locked | `git worktree unlock <folder>` in the repo, then move again. |
| It has submodules | Git can't move a worktree with submodules. Commit and push from it, then start a fresh nekotrail workspace on that branch. |
| It is on a different disk from your home folder | Git moves a worktree only within one disk. Commit and push, then start a fresh workspace on that branch. |

If you chose **Import only** at first, nekotrail shows a one-time notice offering the move, and ⌘K always
has **Move workspaces out of Conductor's folder** while any are left.

To check a repo yourself at any time:

```sh
git -C <your repo> worktree list
```

## Concepts

| Conductor | nekotrail |
|---|---|
| Repository | Repo |
| Workspace | Workspace: a git worktree on its own branch, with chats, a diff view, a terminal and a PR bar |
| Chat tabs | Chat tabs, several per workspace (⌘T) |
| Plan mode | Plan, then Approve (⌘⇧↵) or split a numbered plan into parallel workspaces |
| Checkpoints | Rewind to here, on every message you sent |
| Diff viewer | Changes tab (⌘2 or ⌘⇧D), with line comments you send to the agent as one review |
| Terminal | Terminal tab (⌘3 or ⌘J) |
| Run script | Run (⌘R), and the Preview tab (⌘4) shows the running app |
| Setup, run and archive scripts shared in the repo | `nekotrail.json` in the repo; nekotrail asks before running commands from it |
| Workspace ports | 10 ports per workspace: `NEKOTRAIL_PORT`, `NEKOTRAIL_PORT_1` to `_9`, and `PORT` |
| Pinned and sidebar sections | Pinned and your own sections, the same way |
| Kanban | Board (⌘⇧K) |
| Archive | Archive (⌘⇧A): removes the folder, keeps the branch |
| Linear issues | New workspace, Linear source: start from an issue, and the PR says "Fixes ENG-123" |
| Agents | Claude Code and Codex, plus 14 more: Gemini CLI, OpenCode, Cursor, GitHub Copilot, Goose, Qwen Code, Kimi, Auggie, Grok, Kiro, Droid, Mistral Vibe, Cline and Kilo |
| Cloud workspaces, organisations, team features | Not in nekotrail: it runs on your Mac only, with no account |

New in nekotrail with no Conductor counterpart: **Hand off to another agent** (another agent carries on
the same task in the same workspace, from a brief you read first), Claude + Codex bake-offs with
Compare, the task queue, spend caps, and the night sky.

## Shortcuts

Conductor's shortcuts work in nekotrail, so muscle memory carries over. Settings (⌘,), Shortcuts lists
every one.

| Keys | Action |
|---|---|
| ⌘K | Command palette: any workspace or action |
| ⌘N / ⌘⇧N | New workspace / from a pull request or branch |
| ⌘⌥↑ / ⌘⌥↓ | Previous / next workspace |
| ⌘⌥L | Next workspace with news |
| ⌘L / ⌘⇧D / ⌘J | Chat / Changes / Terminal |
| ⌘B | Show or hide the sidebar |
| ⌘O | Open the workspace in your editor |
| ⌘P | Open any file |
| ⌘R | Run or stop the app |
| ⌘/ | Model and intelligence picker |
| ⌘⇧P / ⌘⇧Y / ⌘⇧M | Create PR / commit and push / merge |
| ⌘⇧R / ⌘⇧X / ⌘⇧L | Agent review / fix failing checks / pull latest |
| ⌘⇧G | Open the PR in your browser |
| ⌘⇧↵ | Approve the agent's plan |
| ⌘⇧A | Archive the workspace |
| ⌘⇧K | Board |
| ⌘, | Settings |

nekotrail adds ⌘1 to ⌘4 for Chat, Changes, Terminal and Preview, and ⌃1 to ⌃9 for the sidebar's
workspaces.
