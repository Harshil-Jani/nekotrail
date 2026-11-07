---
title: "Moving from Conductor to NekoTrail"
description: "What NekoTrail imports from the Conductor app, how it moves your worktrees out of Conductor's folder, and what to do about Conductor Cloud."
date: 2026-11-07T10:00:00Z
clip: conductor
clipLabel: "The Conductor import sheet: what moves, then every worktree moved and checked"
canonical: "moving-from-conductor/"
---

Install NekoTrail, click **Import from Conductor**, press **Import and move**. Your repos, workspaces and chats come over, and your worktrees move out of Conductor's folder. The rest of this post is what happens underneath.

## Why move now

Conductor announced on 4 November 2026 that it is joining Meta. Conductor Cloud goes read-only on 13 November, and Conductor starts deleting Cloud data on 20 November 2026. The Mac app gets critical fixes until early 2027, then winds down.

What matters is where your work lives. Conductor keeps each worktree inside its own folder, and says user data is deleted when the app shuts down. Uncommitted work lives in those folders. Moving them puts it somewhere you control.

## The steps

1. [Download NekoTrail](https://github.com/Harshil-Jani/nekotrail/releases/latest). Until it's notarized, run `xattr -dr com.apple.quarantine /Applications/nekotrail.app` once.
2. Open it. It finds Conductor's data and offers **Import from Conductor** (also on Welcome and in ⌘K).
3. Keep **Move into NekoTrail** picked and press **Import and move**.

The sheet lists every worktree first: repo, branch, size, uncommitted files.

## What comes over

- **Repos**, with scripts and custom prompts.
- **Active workspaces**: name, branch, target, pin, notes.
- **`.env` copy rules** for new workspaces.
- **Claude and Codex chats.** New messages continue the same session (`claude --resume`, `codex exec resume`).

Shortcuts aren't imported. They are already the same.

Importing only reads Conductor's database. Running it again adds only what's new.

## How the move works

Each worktree goes to `~/nekotrail/workspaces/<repo>/<name>` with one command:

```sh
git worktree move <old folder> <new folder>
```

It's the same folder under a new path, so branch, commits, staged and unstaged changes, untracked files, `node_modules` and `.env` all come along.

Nothing in Conductor's folder is deleted. After each move, NekoTrail checks:

- `git worktree list` shows the new folder.
- Branch and head commit are unchanged.
- Uncommitted files are the same, with the same content.

Each worktree ends as **Checked**, **Moved with a difference**, or **Not moved** with the reason. **Try again** retries only the failures.

## When a move fails

- **Locked:** run `git worktree unlock <folder>`, then move again.
- **Has submodules:** git can't move it. Commit, push, start a fresh workspace on the branch.
- **On another disk:** same fix. Commit, push, start fresh.
- **Agent mid-turn:** skipped. Move it after the turn ends.

Chose Import only? ⌘K keeps **Move workspaces out of Conductor's folder** until none are left.

## What doesn't come over

- **Cloud workspaces.** See below.
- **Archived workspaces.** Their branches are in your repo; ⌘⇧N starts from any branch.
- **Conductor settings and checkpoints.** NekoTrail saves its own checkpoints from now on.

## Conductor Cloud

NekoTrail can't read Cloud. Before 20 November 2026, right-click each Cloud workspace in Conductor and choose **Transfer to local**, then import. A pushed branch also works: start a workspace from it or its PR with ⌘⇧N.

## Once you're here

Most things map one to one. Changes tab is the diff viewer, Run plus Preview is the run script, Board is Kanban. New: hand-off between agents, sixteen agents, racing two agents on one task, a task queue, spend caps.

Full concept and shortcut map: the [Moving from Conductor guide](https://harshil-jani.github.io/nekotrail/moving-from-conductor/). Import looks wrong? [Open an issue](https://github.com/Harshil-Jani/nekotrail/issues) with what the sheet said.
