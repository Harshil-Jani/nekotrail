# Product

<!-- impeccable:product-schema 1 -->

Who nekotrail is for and the principles behind it. Read this before proposing a feature or changing how something works; the visual rules are in [DESIGN.md](DESIGN.md).

## Platform

web

(Tauri desktop app on macOS and Linux. The UI is a web view, so the design language is web, not native AppKit.)

## Users

Any developer who already uses Claude Code, Codex or another coding agent from the terminal and wants to run several agent tasks at once without juggling branches, folders and terminal tabs by hand. Many arrive from Conductor, but the product must make sense to someone who never used it.

Their job: hand a task to an agent, keep working on other tasks while it runs, review what it changed, and turn each finished task into a pull request.

## Product Purpose

nekotrail runs many coding agents in parallel on your own machine. Every task gets its own workspace: a git worktree on its own branch, a chat with an agent, a diff view against the target branch, a live preview, a real terminal, notes, and a PR bar showing GitHub status and CI checks. Success is a developer shipping several reviewed PRs in the time one used to take.

## Positioning

- **Open source and local-first.** MIT licensed. Runs on your machine; code and chats stay there.
- **Bring any agent.** Drives sixteen agents through their official CLIs with the user's own subscriptions and settings. Agents that speak the Agent Client Protocol (ACP) are one registry entry each.
- **A safe move from Conductor.** Imports repos, active workspaces and chats from a local Conductor install and moves the worktrees out of Conductor's folder, checking each one. Imported chats continue the same agent session.

## Operating Context

- Lives next to the terminal, the editor and GitHub in a developer's day.
- Uses the user's existing git, `gh` login and agent CLI logins; nothing to sign up for.
- Long-running agent turns: users switch between workspaces while agents work, then come back to unread results.
- The night sky: every workspace is named after a star in one of 8 original cat constellations (50 stars). Creating a workspace lights its star; a merged PR makes it shine; a line between two stars appears only when both have shipped. Felis honours Lalande's real 1799 cat constellation.

## Capabilities and Constraints

- What ships today is in the [README](README.md) and the [changelog](CHANGELOG.md); what's next is in the [roadmap](docs/ROADMAP.md).
- GitHub details come from the user's `gh` login. Linear is optional, with a personal API key kept in the system keychain.
- Agents run headless through their CLIs. "Ask before changes" works where the agent can ask mid-turn (Claude and OpenCode, for example); a hand-off to an agent that can't starts it in Plan instead.
- No cloud, accounts or multiplayer, by design.

## Brand Commitments

- Name: **nekotrail**, always lowercase in the product (neko is Japanese for cat).
- Mascot: an original cute cat. It must not resemble any existing product's mascot.
- Voice: plain, friendly, short. Cat puns allowed in small doses, never in errors.
- Visual system: Apple's macOS Human Interface Guidelines, so nekotrail feels like a native Mac app. System font, semantic label colours, hairline separators, a vibrancy sidebar with the traffic lights inside it, segmented controls, pop-up menus, and light and dark following the system. The cat orange is the one accent. Tokens and rules live in DESIGN.md.
- Simple layouts, a consistent spacing scale, contrast checked numerically.

## Evidence on Hand

- No testimonials, benchmarks or press are claimed anywhere in the product or the README. Don't invent any; numbers in copy must come from something we measured.

## Product Principles

1. The agent's work is the content; chrome stays out of the way.
2. Never lose work: archive keeps branches, moves are checked, destructive actions confirm and can be undone where possible.
3. Your tools, your accounts: reuse the user's CLIs and logins, never proxy them.
4. Delight lives in the night sky and the mascot, not in the working surfaces.
