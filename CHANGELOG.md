# Changelog

Every release of nekotrail, newest first. Each version is tagged on GitHub, and its macOS app is attached
to the [release page](https://github.com/Harshil-Jani/nekotrail/releases).

## Unreleased

**Checkpoints**
- Rewind to here: every message you send saves your files first. Rewind puts them back as they were, including untracked files and what you had staged, and tells you how many files change before it does anything.
- Rewinds can be undone: the chat shows "Rewound to before: ..." with an Undo button, and nothing in the chat is deleted. `.env`, `node_modules` and other ignored files are never touched.
- Each workspace keeps its last 50 checkpoints, and archiving a workspace clears them. Saving one takes about a quarter of a second on a 15,000-file repo.

**Gemini CLI**
- Gemini CLI joins Claude and Codex. Pick it for a new workspace or start a Gemini chat in any workspace; it uses your own Gemini login.
- Pick Gemini's model (Auto, Pro, Flash, Flash-Lite) in the model picker, and set its defaults for new chats in Settings.
- The setup check looks for Gemini CLI and shows the install command if it's missing. You still only need one agent.
- Repo settings that would make Gemini run commands (hooks, MCP servers, risky `.env` variables) ask before Gemini runs, the same way Claude and Codex settings do.

**Changes, overlaps and stacks**
- Discard one file in Changes. Only that file's uncommitted edits go, and the old copy lands in the Trash, so you can get it back.
- Filter the changed-file list when a workspace touches lots of files.
- The sidebar quietly marks workspaces that change the same files, and the PR bar suggests which one to merge first. The check no longer runs git in every workspace after each turn.
- Stack a workspace on another one: its PR builds on unmerged work and the PR bar links the parent.
- When the parent PR merges, one click moves the stacked workspace onto main, rebases it and points its PR there.

**Preview**
- New Preview tab (⌘4): see your running app inside the workspace, with reload, open in browser and a localhost-only address bar.
- Annotate: freeze the page, draw boxes and arrows, add a note, and it lands in your chat draft as a screenshot the agent can see.
- Nothing running? Start the Run script from Preview in one click. It shows your app as soon as the server answers.
- Preview only ever loads local addresses, even if you click a link inside your app.

**Updates**
- nekotrail now tells you when a new version is out: a small card with the cat, what's new, and Update or Not now. "Not now" waits a day before asking about that version again; a newer one asks right away.
- Update downloads in the background while you keep working. If agents or terminal commands are running, it says how many and lets you restart now or once everything is idle.
- Check for updates any time from ⌘K or Settings, next to the version.
- If you're on 0.10.1 or earlier, install this release by hand once from the release page. After that, updates come to you.

**Design ship-gate fixes**
- A merged workspace no longer archives itself the moment you open it. With "Archive when the PR merges" on, it archives only after you watched the PR merge, and only once you move on. Archiving opens the next workspace in the sidebar instead of the welcome page.
- New shortcuts: ⌘1, ⌘2 and ⌘3 show Chat, Changes and Terminal. Workspaces in the sidebar moved to ⌃1 to ⌃9. ⌘L, ⌘⇧D and ⌘J still work.
- ⌘K opens on Recent: your five last workspaces, unread first.
- The sidebar list is one Tab stop: arrow keys move, Left and Right fold a group, Return opens. A workspace under Needs you no longer shows as selected twice.
- The PR bar stays on one line in small windows, Create PR waits until there is a change, and the todo list folds while a plan or a tool call waits for your OK.
- Settings has four panes (General, Notifications, Agents, Shortcuts) and a Close button.
- Destructive actions ask in a proper sheet instead of a browser dialog; adding a section uses the inline field.
- Welcome puts setup first while something is missing and has one main button. The Conductor import card can be dismissed.
- Compare agents, the board and Changes fit 1100 px windows; a closed PR counts as Done on the board.
- Higher contrast for diff line numbers, +/- counts, shortcut hints, the merge button, warnings and orange labels (all 4.5:1 or more). The terminal background matches its frame. "New" on a model lasts 30 days.

## 0.10.1 (2026-11-06)

- Fixes the 0.10.0 build: "Copy branch name" and "Copy PR link" were missing from the ⋯ menu, which also broke the type check.

## 0.10.0 (2026-11-06)

- Named actions: besides Run, give a repo commands like test, lint or db:migrate (repo settings, Scripts). Start them from the menu next to Run, from ⌘K, or with /run test in a chat. They run in the workspace terminal so you see the output.
- "Copy branch name" and "Copy PR link" in the workspace's ⋯ menu.

## 0.9.0 (2026-11-06)

**Design pass** (from an outside design review)
- Chat, Changes and Terminal moved into the toolbar, Finder style: three bars at the top became two, so the chat starts about 45 px higher.
- A calmer PR bar: plain text with coloured icons, one height for everything, and a less blinding merge button.
- Orange only where it means "act here": your messages are neutral grey, agent labels are grey, plan cards keep a thin accent edge.
- Toolbar, PR bar and chat tabs share one left edge; toolbar buttons look like Mac toolbar items; pop-up menus use a single chevron.
- Consecutive tool calls group together; letter avatars get a disc; empty sections and board columns stop shouting; the low-disk notice is one quiet row.
- Opening a workspace with news shows one line on what happened while you were away: which agent finished, how long it took, what it cost, lines changed and the PR state.

## 0.8.0 (2026-11-06)

**Storage**
- See how much disk each workspace uses and clean its build output in one click: dependency and build folders that git ignores (node_modules, .venv, target, .next, caches). Code, commits and .env files are never touched, and each folder is checked against git again right before it's deleted. On a real install this found 2.6 GB to free.
- When the disk drops under 3 GB, the sidebar says so and opens Storage. A full disk makes agents and builds fail in confusing ways.

## 0.7.0 (2026-11-06)

**Ask before changes** (Claude)
- A new permission mode: the agent stops before each edit or command and shows you exactly what it wants to run. Allow (↵), Deny (⌫), or "Allow for this chat". You get the ting, a notification and a Dock bounce when it asks, so you can work elsewhere. Tested end to end against the real Claude Code CLI.
- Plan, Ask before changes, Auto-accept edits and Full auto are now the four modes, in the chat box and when you create a workspace. Codex keeps the three it supports.

## 0.6.0 (2026-11-06)

**More done in parallel**
- Split a plan into workspaces: when an agent proposes a numbered plan, one click starts a separate workspace and agent for each step, side by side. Each branch is named after its step.
- Prompt snippets: save prompts you use often (two examples are included), then type /name in the chat box to drop one in and edit it. Manage them in Settings.

**Know which agent works for you**
- Agent scorecard (⌘K, "Open agent scorecard"): clean-finish rate, average turn time, spend per turn and merged workspaces for Claude and Codex, from your own history.

**Richer answers**
- Mermaid diagrams in chat render as real diagrams, in light and dark. The diagram library loads only when a chat has one.

## 0.5.0 (2026-11-06)

**Agents that work as a team**
- Autopilot: turn it on from the PR menu and nekotrail keeps the PR moving on its own. Failing checks go to the agent with their logs, conflicts get merged and resolved, requested changes get answered. It stops after 3 agent turns or when the PR is green, and tells you with a ting and a notification.
- Continue in the other agent: when Claude or Codex hits its usage limit (or isn't installed), one click opens the other agent with the conversation so far, ready to send.
- Overlap warning: the PR bar tells you when another workspace in the same repo changed the same files, before it turns into a merge conflict.

## 0.4.0 (2026-11-06)

**Fast with 30 workspaces** (measured on a real install with 23 workspaces and 18,500 chat messages)
- Big chats open with their latest 600 events: the largest test chat went from 6.2 MB to 566 KB and opens about 10 times faster. "Load older events" brings in the rest.
- Spend totals come from their own table instead of scanning every message: 19 ms to under 0.1 ms on every refresh.
- Changed-line counts are cached; only workspaces an agent or a git action touched are recounted, instead of all of them after every turn.
- GitHub and git checks run in the background, so a slow `gh` call can't hold up the app. The PR bar stops polling while the window is hidden.
- Changed-file lists match git's output by path: exact for renames, spaces in names and binary files. Huge new files are no longer read whole to count lines.

## 0.3.0 (2026-11-06)

**Safe to switch from Conductor**
- Move imported workspaces out of Conductor's folder in one click, so cleaning up or uninstalling Conductor can't delete them. Branches, chats and uncommitted changes come along.
- "Copy diagnostics for a bug report" in ⌘K: versions and counts only, no paths, chats or tokens.

**Polish**
- The plan card no longer says "Plan" twice.
- Compare is a regular button, so Merge stays the one loud action.

## 0.2.2 (2026-11-06)

- Sidebar names use the full width; before, they were cut off halfway.
- Section chevrons, row icons and "Add section" line up on one edge, and the sidebar button sits on the traffic lights' line.
- Repo buttons appear on hover without squeezing the repo name; repos without a picture get a letter.
- README screenshots retaken with a made-up sample project.

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
