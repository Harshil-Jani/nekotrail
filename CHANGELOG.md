# Changelog

Every release of nekotrail, newest first. Each version is tagged on GitHub, and its macOS app is attached
to the [release page](https://github.com/Harshil-Jani/nekotrail/releases).

## 0.2.1 (2026-11-06)

**Clearer words everywhere**
- About 160 labels, messages and errors rewritten in one plain voice: buttons say what they do ("Create workspace", "Add local repo"), the same thing has the same name everywhere, and every error says what to do next.

**Easier to get started**
- The README has a screenshot tour of every main screen, plus a Download for Mac link.
- A fresh install no longer shows empty sidebar sections.

## 0.2.0 (2026-11-06)

The Conductor-replacement release: everything a daily Conductor user reaches for, plus things it never had.

**Looks like a Mac app**
- Apple's macOS design: frosted sidebar with the traffic lights inside it, system font, rounded controls, light and dark following your Mac.
- Collapsible sidebar (⌘B), resizable, with a RAM meter and back / forward (⌘[ ⌘]).
- The night sky is now a full sky: tap a constellation to read its story.

**Sidebar and board**
- Sections like Conductor: Pinned, your own sections (drag a workspace in, or right-click and pick "Move to section"), then projects.
- One-line rows with the PR state as the icon: branch, draft, open, merged, closed.
- Project filter, "Needs you" inbox, group by status, archived group, changed-line counts.
- Kanban board (⌘⇧K): Backlog, In progress, In review, Done. Drag to set status.

**Agents**
- One picker for the model and its intelligence level, with the newest models (⌘/).
- Slash commands for everything: `/mcp`, `/pr`, `/merge`, `/review`, `/run`, `/fork`, `/add-dir`, plus your own `.claude/commands`.
- Ting ting, a notification and a Dock bounce when an agent needs you (plan to approve, question, sign-in, limit, error).
- Bake-off: give the same task to Claude and Codex, compare, keep the better one.
- Second opinion: the other agent reviews the branch; its findings become review comments.
- Plan approval card, live todo list, editable message queue, chat search, fork a chat.
- Rewind files to any message (checkpoints).

**Git and PRs**
- One button for the next step: create PR, fix failing checks (with their logs), resolve conflicts, squash and merge, merge when green.
- Start a workspace from a task, a GitHub issue, an open PR or a branch.
- Changes: all or uncommitted, side by side, viewed ticks, GitHub review comments inline, edit any file (⌘P).
- Terminal tabs that keep their output, Run / Stop (⌘R), a localhost link, setup in its own tab before the agent starts.

**Setup and safety**
- First-run check for git, GitHub, Claude Code and Codex, with the command for anything missing.
- Clone from GitHub; Conductor import shows what it will bring over first.
- Secret files like `.env.local` can no longer be committed by an agent.
- Spend in the sidebar counts only real nekotrail usage.

## 0.1.0 (2026-11-05)

First version: workspaces as git worktrees, Claude Code and Codex chats, diff view, terminal, PR status
from GitHub, setup and run scripts, the Conductor importer, the cat and the night-sky quest.
