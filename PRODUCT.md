# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Tauri desktop app on macOS. The UI is a web view, so the design language is web, not native AppKit.)

## Users

Any developer who already uses Claude Code or Codex from the terminal and wants to run several agent tasks at once without juggling branches, folders and terminal tabs by hand. Many arrive from Conductor after it announced it is joining Meta (Nov 4, 2026), but the product must make sense to someone who never used Conductor.

Their job: hand a task to an agent, keep working on other tasks while it runs, review what it changed, and turn each finished task into a pull request.

## Product Purpose

nekotrail runs many coding agents in parallel on your own Mac. Every task gets its own workspace: a git worktree on its own branch, a chat with Claude Code or Codex, a diff view against the target branch, a real terminal, notes, and a PR bar showing GitHub status and CI checks. Success is a developer shipping several reviewed PRs in the time one used to take.

## Positioning

- **Open source and local-first.** MIT licensed. Runs on your machine; code and chats stay there. It cannot be shut down or acquired away.
- **Bring any agent.** Drives the official `claude`, `codex` and `gemini` CLIs with the user's own subscriptions and settings. More agent CLIs can be added.
- **One-click move from Conductor.** Imports your repos, active workspaces and chats from a local Conductor install, read-only, without moving files. Imported chats continue the same agent session.

## Operating Context

- Lives next to the terminal, editor (Cursor, VS Code) and GitHub in a developer's day.
- Uses the user's existing git, `gh` login and agent CLI logins; nothing to sign up for.
- Long-running agent turns: users switch between workspaces while agents work, then come back to unread results.
- The quest is a night sky: every workspace is named after a star in one of 8 original cat constellations (50 stars). Creating a workspace lights its star; a merged PR makes it shine; a line between two stars appears only when both have shipped. Felis honours Lalande's real 1799 cat constellation.

## Capabilities and Constraints

- v0.1 shipped: repos, workspaces as git worktrees, Claude/Codex chat with queueing and stop, plan / auto-accept / full-auto permission modes, diff view incl. untracked files, terminal per workspace, notes, pin, unread, archive (keeps branch) and restore, setup/run/archive scripts (`nekotrail.json` overrides app settings), PR creation and status via `gh`, "fix failing CI" and "resolve conflicts" prompts, Conductor importer, notifications and Dock badge, night-sky quest.
- GitHub details come from the user's `gh` login: avatar, name, login, PR authors and reviewers.
- Headless agent turns cannot ask for permission mid-turn, so there is no "ask before each edit" mode yet.
- Not built yet: checkpoints, inline review comments, Linear, port forwarding (the Preview tab shows localhost only), cloud, mobile, multiplayer.

## Brand Commitments

- Name: **nekotrail** (lowercase in the product; neko is Japanese for cat). Renamed from purrfectpr on 2026-11-05 because the user disliked it. nekotrail.com, .dev and .app were unregistered that day.
- Mascot: an original cute cat. It must not resemble any existing product's mascot, including Meta's Muse or Conductor's art.
- Launch and GTM should run as a character-led launch (mascot-first), with the night-sky quest as the hook. Planned, not yet done.
- Voice: plain, friendly, short. Cat puns allowed in small doses, never in errors.
- Visual system (2026-11-06, replaces the X-dark look): Apple's macOS Human Interface Guidelines, so nekotrail feels like a native Mac app. The user asked for it ("apple ki design system") and chose Apple rounded corners and light and dark following the system. System font, semantic label colours, hairline separators, vibrancy sidebar with the traffic lights inside it, segmented controls, pop-up menus. The cat orange is the one accent. Tokens and rules live in DESIGN.md.
- User preferences: simple layouts, a consistent spacing scale, contrast checked numerically.

## Evidence on Hand

- Working app at `src/` and `src-tauri/`, 12 Rust tests passing.
- No users, testimonials, benchmarks or press yet. Do not invent any.

## Product Principles

1. The agent's work is the content; chrome stays out of the way.
2. Never lose work: archive keeps branches, imports are read-only, destructive actions confirm.
3. Your tools, your accounts: reuse the user's CLIs and logins, never proxy them.
4. Delight lives in the night sky and the mascot, not in the working surfaces.
