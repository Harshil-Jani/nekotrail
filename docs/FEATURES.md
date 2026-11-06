# Features: keep, next, skip

This list was built from Conductor's public changelog (about 150 entries, July 2025 to November 2026) and from daily use. It holds only feature ideas. nekotrail's code and design are original.

## In v0.1

| Feature | Notes |
|---|---|
| Workspaces as git worktrees | One branch and folder per task. Archive keeps the branch; restore re-creates the folder. |
| Claude Code and Codex chats | Several per workspace. Streamed tool calls, thinking, cost and time. |
| Message queue and stop | Send while the agent works and it runs next. |
| Plan / auto-accept / full-auto | Maps to Claude permission modes and Codex sandbox modes. |
| Diff vs target branch | Includes uncommitted and untracked files. |
| Terminal per workspace | Real PTY login shell. |
| Notes, pin, unread, search | |
| Setup / run / archive scripts | Saved per repo, or committed as `nekotrail.json`. |
| PR create and status | Author, reviewers, CI, comments, +/- lines, via `gh`. Works for fork PRs. |
| Fix CI / resolve conflicts | Drafts the prompt for the agent; you review and send. |
| GitHub identity | Avatar, name, repo owners, stars. |
| Conductor importer | Repos, active workspaces, chats. Read-only. |
| Notifications and Dock badge | When an agent finishes while you're elsewhere. |
| Night sky and cat moods | Workspaces light stars, merged PRs make them shine; mascot moods map to real state. |
| Checkpoints | Snapshot before every message; rewind files (and commits) to any message. |
| Review comments | Comment on diff lines, send them to the agent as one message. |
| @-mentions and attachments | Fuzzy file picker in the composer; pasted screenshots are saved in a git-ignored folder. |
| Command palette and settings | ⌘K for everything, ⌘, for notifications, chime and editor. |
| Keyboard | ⌘K, ⌘N, ⌘1 / ⌘2 / ⌘3 tabs, ⌃1-9 workspaces, ⌘O, ⌘↵, ⌘,. |

## Next (ranked)

1. **Create a workspace from a PR, a GitHub issue or a Linear issue.**
2. **Edit files in the diff view.**
3. **Mid-turn tool approval** (needs Claude Code's streaming input mode).
4. **Notarized builds and auto-update.**
5. **Port forwarding** for web apps. (The Preview tab already shows localhost dev servers, with Annotate.)
6. **Workspace links** (`nekotrail://ws/<id>`) to share in chat.

## Skipping for now

- Hosted cloud workspaces, mobile apps, multiplayer and dispatchers. They need a server, and nekotrail stays local.
- Per-model launch announcements. The CLIs already ship new models; nekotrail passes the model name through.
- Year-in-review style "Wrapped" screens.
