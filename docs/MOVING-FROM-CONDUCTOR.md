# Moving from Conductor

NekoTrail imports your repos, workspaces and chats from the Conductor app on your Mac, then moves your
worktrees out of Conductor's folder and into its own, so nothing you haven't committed depends on
Conductor any more.

**Why now.** Conductor is joining Meta, and since 4 November 2026 new users can't sign up or download it.
Conductor Cloud goes read-only on 13 November 2026 and shuts down on 20 November 2026, when Conductor
starts deleting Cloud data. The Conductor app on your Mac gets new models and critical fixes until early
2027 and then winds down, and Conductor says it will "delete all Conductor user data when the app shuts
down". Your worktrees live inside Conductor's folder, so moving them out means your work, including
anything you haven't committed, no longer depends on Conductor. All of this is from
[Conductor's announcement](https://conductor.build/announcement).

## The short version

1. [Download NekoTrail](https://github.com/Harshil-Jani/nekotrail/releases/latest) and open it (see
   [Install](../README.md#install)).
2. On first launch it finds Conductor's data and offers **Import from Conductor**. Later, the same button
   is on the welcome screen, and in ⌘K.
3. The import sheet lists every worktree it would move, with its repo, branch, size and how many
   uncommitted files it has. **Move into NekoTrail** is picked for you. Press **Import and move**, or
   pick **Import only** to leave the worktrees where they are for now.
4. Each worktree is moved with `git worktree move` and checked. The sheet says which ones moved, and
   why any didn't.

Using Conductor Cloud? Read [Conductor Cloud](#conductor-cloud) first.

## What comes over

| From Conductor | In NekoTrail |
|---|---|
| Repos | Added with their name, folder, default branch and remote. A repo you already added in NekoTrail is reused. |
| Setup, run and archive scripts | The repo's scripts in NekoTrail (Repo settings, Scripts). |
| Custom prompts (general, code review, create PR) | Repo settings, Agent: instructions for every chat, the review prompt and the PR prompt. |
| Files copied into new workspaces (your `.env` globs) | Repo settings, Files and env. |
| Active workspaces | One NekoTrail workspace each, with its name, branch, target branch, pin and notes. |
| Claude and Codex chats | Each chat with its message history. New messages continue the same agent session (`claude --resume`, `codex exec resume`). |
| Keyboard shortcuts | Not imported, because they are already the same. See [Shortcuts](#shortcuts). |

Importing only reads Conductor's database, and running it again only adds what is new.

## What doesn't come over

- **Conductor Cloud workspaces.** NekoTrail can only read the Conductor app on your Mac.
- **Archived workspaces.** They have no worktree left to open. Their branches are still in your repo, and
  New workspace (⌘⇧N) can start from any branch.
- **Hidden chats**, and workspaces whose folder is no longer on disk. The sheet counts these as skipped.
- **Conductor's own settings and account**: its theme, notifications, billing, organisation and team
  features. NekoTrail has no account; your agents use their own CLI logins.
- **Conductor checkpoints.** NekoTrail saves its own before each message you send from now on (Rewind to
  here).
- **On Linux**, nothing: Conductor is Mac-only, so there is no Conductor data to read there.

## Conductor Cloud

NekoTrail can't read Conductor Cloud. Cloud goes read-only on 13 November 2026 and shuts down on
20 November 2026. Before then, in Conductor, right-click each Cloud workspace and choose **Transfer to
local**, which moves it and its chats to the Conductor app on your Mac. Then import into NekoTrail as above.

If a Cloud workspace's branch is pushed to GitHub, you can also start a NekoTrail workspace from that
branch or its pull request with ⌘⇧N. Anything that only lived in the Cloud workspace and was never
pushed can only be saved through Conductor's own export (Settings, Account, Export data).

## The move, step by step

Conductor keeps each worktree inside its own folder. NekoTrail moves imported worktrees into
`~/nekotrail/workspaces/<repo>/<name>` (the next free name if that one is taken).

- **What runs:** only `git worktree move <old folder> <new folder>`, from the repo. Git renames the folder
  and updates its own records, so the branch, its commits, staged and unstaged changes, untracked files,
  and ignored files such as `node_modules` and `.env` all come along.
- **What never happens:** NekoTrail never deletes, copies or cleans anything. Git renames each worktree's
  folder, and a worktree that can't move stays exactly where it was.
- **The check after each move:** `git worktree list` shows the new folder, the branch and its head commit
  are the same as just before the move, and the uncommitted files are the same files with the same
  content (git's status list, `git diff HEAD`, and each untracked file's content).
- **Results:** each worktree reads Checked, Moved with a difference to look at, or Not moved with the
  reason in plain words. **Try again** retries only the ones that didn't move.
- **Skipped for now:** a workspace with an agent working in it. Move it once the turn ends.
- **Afterwards:** your work is in NekoTrail's folder, and Conductor no longer finds these folders.

Reasons a move can fail, and what to do:

| Reason | What to do |
|---|---|
| Git has the worktree locked | `git worktree unlock <folder>` in the repo, then move again. |
| It has submodules | Git can't move a worktree with submodules. Commit and push from it, then start a fresh NekoTrail workspace on that branch. |
| It is on a different disk from your home folder | Git moves a worktree only within one disk. Commit and push, then start a fresh workspace on that branch. |

If you chose **Import only** at first, NekoTrail shows a one-time notice offering the move, and ⌘K offers
**Move … out of Conductor's folder** while any are left.

To check a repo yourself at any time:

```sh
git -C <your repo> worktree list
```

## Concepts

| Conductor | NekoTrail |
|---|---|
| Repository | Repo |
| Workspace | Workspace: a git worktree on its own branch, with chats, a diff view, a terminal and a PR bar |
| Chat tabs | Chat tabs, several per workspace (⌘T) |
| Plan mode | Plan, then Approve (⌘⇧↵) or split a numbered plan into parallel workspaces |
| Checkpoints | Rewind to here, on every message you sent |
| Diff viewer | Changes tab (⌘2 or ⌘⇧D), with line comments you send to the agent as one review |
| Terminal | Terminal tab (⌘3 or ⌘J) |
| Run script | Run (⌘R), and the Preview tab (⌘4) shows the running app |
| Setup, run and archive scripts shared in the repo | `nekotrail.json` in the repo; NekoTrail asks before running commands from it |
| Workspace ports | 10 ports per workspace: `NEKOTRAIL_PORT`, `NEKOTRAIL_PORT_1` to `_9`, and `PORT` |
| Pinned and sidebar sections | Pinned and your own sections, the same way |
| Kanban | Board (⌘⇧K) |
| Archive | Archive (⌘⇧A): removes the folder, keeps the branch |
| Linear issues | New workspace, Linear source: start from an issue, and the PR says "Fixes ENG-123" |
| Agents | Claude Code and Codex, plus 14 more: Gemini CLI, OpenCode, Cursor, GitHub Copilot, Goose, Qwen Code, Kimi, Auggie, Grok, Kiro, Droid, Mistral Vibe, Cline and Kilo |
| Cloud workspaces, organisations, team features | Not in NekoTrail: it runs on your Mac only, with no account |

New in NekoTrail with no Conductor counterpart: **Hand off to another agent** (another agent carries on
the same task in the same workspace, from a brief you read first), racing Claude and Codex with
Compare agents, the task queue, spend caps, and the night sky.

## Shortcuts

Conductor's shortcuts work in NekoTrail, so muscle memory carries over. Settings (⌘,), Shortcuts lists
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

NekoTrail adds ⌘1 to ⌘4 for Chat, Changes, Terminal and Preview, and ⌃1 to ⌃9 for the sidebar's
workspaces.
