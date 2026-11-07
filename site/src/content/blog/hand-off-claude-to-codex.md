---
title: "Hand off: when Claude runs out, Codex picks up"
description: "How NekoTrail's hand-off works: a brief built on your Mac from the chat, which you read and edit before another agent carries on in the same branch."
date: 2026-11-07T11:00:00Z
clip: handoff
clipLabel: "Claude is out of usage; the hand-off sheet opens, the brief is read, and Codex carries on in the same workspace"
---

When an agent runs out of usage or goes in circles, NekoTrail hands the task to another agent in the same workspace and branch. It writes a brief from the chat, you read it, and the next agent carries on. No re-explaining.

This shipped in NekoTrail 0.20.

## The problem

Every agent hits a ceiling. A usage limit. A loop: fix the test, break the build, fix the build, break the test.

You're left with a half-done branch and a chat that knows everything but can't continue. Explaining it all to a new agent takes a while, and you always forget something.

## Where to find it

- A chat's **+** menu: **Hand off to another agent…**
- ⌘K, or `/handoff` in the chat box.
- The card that shows when an agent runs out of usage, or when NekoTrail stops a loop.

From a terminal: `nekotrail handoff <workspace> --to codex`. Add `--now` to send at once. `.` means the workspace you're in.

## Pick the next agent

The sheet lists the agents you have installed. Pick one and its model.

Permissions carry over and never grow. The new chat keeps this chat's mode. If you were in Ask before changes and the next agent can't ask, it starts in Plan. Spend caps and protected files still apply.

## What's in the brief

The brief is the new agent's first message. It says it's taking over partway, in the same folder and branch, so it shouldn't start over. Then:

1. **The task**, quoted.
2. **What you said since**, like "use semicolons".
3. **What the last agent did**: last messages, changed files, failing checks and commands, recent steps.
4. **What's left**: its todo list or plan.
5. **Your turn**: check `git status` and `git diff`, then get the failures passing.

## Built on your Mac

No model writes the brief. NekoTrail builds it from the chat's own record: messages, tool calls, diff, checks. Nothing is invented, and nothing leaves your machine.

- It stays under about 6,000 characters. Older steps shrink first.
- Things that look like keys, tokens, passwords or emails are hidden.
- Paths in your home folder show as `~`.

## Read it before you send

The brief sits in an editable box. If the last agent went the wrong way, say so: "The bug is in the timeout, not the retry." Nothing is sent until you press **Hand off to Codex**.

The brief is all the next agent knows. Reading it beats watching it repeat the same mistake.

## After you press it

A new chat opens in the same workspace.

- The old chat ends with "Handed off to Codex".
- The new one starts with "Continued from Claude".
- Each links to the other.

The brief sits folded at the top of the new chat. Export session records the hand-off too.

## When I use it

- **Out of usage, two tests left.** Codex reads the failures from the brief and finishes.
- **A loop.** When the queue or autopilot drives, by default NekoTrail stops an agent after the same command fails four times, a file flips back and forth, or three turns change nothing. A fresh agent with the list of what failed often gets past it.
- **Review only?** Use Second opinion instead. It reads the branch without changing it.

Picking the right agent up front still matters. Hand-off just means the choice is never final.
