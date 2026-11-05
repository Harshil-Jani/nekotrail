# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Tauri desktop app on macOS. The UI is a web view, so the design language is web, not native AppKit.)

## Users

Any developer who already uses Claude Code or Codex from the terminal and wants to run several agent tasks at once without juggling branches, folders and terminal tabs by hand. Many arrive from Conductor after it announced it is joining Meta (Nov 4, 2026), but the product must make sense to someone who never used Conductor.

Their job: hand a task to an agent, keep working on other tasks while it runs, review what it changed, and turn each finished task into a pull request.

## Product Purpose

purrfectpr runs many coding agents in parallel on your own Mac. Every task gets its own workspace: a git worktree on its own branch, a chat with Claude Code or Codex, a diff view against the target branch, a real terminal, notes, and a PR bar showing GitHub status and CI checks. Success is a developer shipping several reviewed PRs in the time one used to take.

## Positioning

- **Open source and local-first.** MIT licensed. Runs on your machine; code and chats stay there. It cannot be shut down or acquired away.
- **Bring any agent.** Drives the official `claude` and `codex` CLIs with the user's own subscriptions and settings. More agent CLIs can be added.
- **One-click move from Conductor.** Imports your repos, active workspaces and chats from a local Conductor install, read-only, without moving files. Imported chats continue the same agent session.

## Operating Context

- Lives next to the terminal, editor (Cursor, VS Code) and GitHub in a developer's day.
- Uses the user's existing git, `gh` login and agent CLI logins; nothing to sign up for.
- Long-running agent turns: users switch between workspaces while agents work, then come back to unread results.
- Workspaces are named after cat breeds from around the world (the "discover" theme): each new workspace reveals a breed card with its origin and one true fact, and is added to the user's Breedex collection.

## Capabilities and Constraints

- v0.1 shipped: repos, workspaces as git worktrees, Claude/Codex chat with queueing and stop, plan / auto-accept / full-auto permission modes, diff view incl. untracked files, terminal per workspace, notes, pin, unread, archive (keeps branch) and restore, setup/run/archive scripts (`purrfectpr.json` overrides app settings), PR creation and status via `gh`, "fix failing CI" and "resolve conflicts" prompts, Conductor importer.
- GitHub details come from the user's `gh` login: avatar, name, login, PR authors and reviewers.
- Headless agent turns cannot ask for permission mid-turn, so there is no "ask before each edit" mode yet.
- Not built yet: checkpoints, inline review comments, Linear, browser preview, port forwarding, cloud, mobile, multiplayer.

## Brand Commitments

- Name: **purrfectpr** (lowercase in the product). purrfectpr.com and purrfectpr.dev were unregistered on 2026-11-05.
- Mascot: an original cute cat. It must not resemble any existing product's mascot, including Meta's Muse or Conductor's art.
- Launch and GTM should run as a character-led launch (mascot-first), with the discovery theme as the hook. Planned, not yet done.
- Voice: plain, friendly, short. Cat puns allowed in small doses, never in errors.
- Visual standing preference (2026-11-05, user's words): "as aesthetic as conductor.build", taking X (Twitter) dark theme's design system as the reference. Convention is the commitment: true-black ground, hairline dividers, white text, one accent, executed at full craft. Square corners everywhere (user rejected rounded corners on 2026-11-05); only avatars and status dots are round. Borrow the system, never X's or Conductor's logos, names, icons or illustrations.
- User preferences: simple layouts, a consistent spacing scale, contrast checked numerically.

## Evidence on Hand

- Working app at `src/` and `src-tauri/`, 12 Rust tests passing.
- No users, testimonials, benchmarks or press yet. Do not invent any.

## Product Principles

1. The agent's work is the content; chrome stays out of the way.
2. Never lose work: archive keeps branches, imports are read-only, destructive actions confirm.
3. Your tools, your accounts: reuse the user's CLIs and logins, never proxy them.
4. Delight lives in the breed discovery and the mascot, not in the working surfaces.
