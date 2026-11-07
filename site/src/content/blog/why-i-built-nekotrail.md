---
title: "Why I built NekoTrail"
description: "Several agents in one folder step on each other. Git worktrees fix that. NekoTrail is the rest: local, open source, your own agent logins."
date: 2026-11-07T09:00:00Z
clip: parallel
clipLabel: "Switching between workspaces where different agents work at the same time, then the board of all of them"
---

I built NekoTrail so I could run several coding agents at once without them wrecking each other's work. Each task gets its own git worktree, and everything stays on my Mac.

## The problem: one folder, many agents

Agents got good enough that my bottleneck became waiting. The fix is to start a second task while the first runs.

In one checkout, that breaks fast:

- One agent runs tests while the other is mid-rename.
- One switches branches and drags the other's edits along.
- You end up with a diff nobody owns.

Extra clones help, but then you track tabs, branches, `node_modules` and port 3000 by hand. That is bookkeeping, and I am bad at bookkeeping.

## The fix: worktrees

`git worktree` gives one repo several working folders, each on its own branch. It is cheap, it can't clobber your main checkout, and removing it keeps the branch.

That maps straight onto agent work:

1. One task, one worktree, one branch.
2. The agent works only in that folder.
3. When it's done, the branch becomes a PR.

In NekoTrail that unit is a **workspace**: the worktree plus a chat, a diff, a terminal, a live preview and a PR bar with CI status. The sidebar shows which agents are working, which need you, and which are done.

## Features came from walls I hit

- Five copies of one app fought over ports, so each workspace now gets 10 of its own.
- Risky changes needed a way back, so every message you send saves your files first.
- An agent looped overnight, so there are spend caps and a check that stops repeated failures.

## Why local

NekoTrail talks only to tools you already have: `git`, `gh`, and your agent CLIs. Workspaces are plain folders. Chats live on your disk. No account, no server of mine.

Two reasons:

- **Trust.** An agent harness sees your code, terminal and prompts. That should stay on your machine.
- **Durability.** If a tool keeps your work on its server, it can take the work with it when it goes. Plain worktrees on your disk outlive any app.

## Why your own agents

I didn't want another model wrapper. NekoTrail drives the official CLIs (Claude Code, Codex, Gemini CLI and 13 more) with your logins. Your subscription is used as is. No proxy, no markup.

It also means no lock-in. Race two agents on one task, or hand a half-done task to another when the first runs out.

## Why open source

MIT, on GitHub. Read what it runs, build it yourself, fork it if you disagree with me. The changelog is public and every release is tagged.

## The cat

Every workspace is named after a star in one of eight cat constellations. Start one and its star lights. Merge its PR and it shines. Ship enough and a whole cat appears. One constellation, Felis, is real: Jérôme Lalande drew it in 1799.

The sidebar cat reports real state: tail wagging while an agent works, asleep after five quiet minutes, worried when CI fails. Agent work is a lot of waiting. A little warmth helps.

## Try it

NekoTrail is early, used daily on macOS, with experimental Linux builds. It's free and will stay free. Found something wrong? [Open an issue](https://github.com/Harshil-Jani/nekotrail/issues).
