# Conductor parity tracker

What a daily Conductor user reaches for, and where nekotrail stands. Built from the installed Conductor
app (its UI text, bundled scripts and database schema), its docs and its changelog. Only feature ideas
are taken; nekotrail's code and design are its own. Cloud and team features are left out on purpose.

Status: ✅ done · 🟡 partial · ⬜ not yet. Rounds are worked top to bottom.

## Round 1: agent settings and workspace basics

| Status | Feature | Notes |
|---|---|---|
| ✅ | Effort level per chat | `--effort` for Claude, `model_reasoning_effort` for Codex |
| ✅ | Repo instructions for every chat | `--append-system-prompt`; first message for Codex |
| ✅ | Repo environment variables | Agents, scripts and terminals |
| ✅ | Copy `.env*` (any globs) into new workspaces | Default `.env*`; imported from Conductor's `file_include_globs` |
| ✅ | Keep the Mac awake while agents work | `caffeinate -i -w <pid>`; setting to turn off |
| ✅ | Context usage meter | Last request's tokens / model context window |
| ✅ | Branch named after the first message | Only before the branch is pushed; setting to turn off |
| ✅ | Rename workspace and branch | Branch rename refused once pushed |
| ✅ | Workspace status (backlog, in progress, in review, done) | Derived from PR state unless set by hand |
| ✅ | Close chat tabs | History kept |
| ✅ | Agent cost per workspace | Summed from finished turns |
| ✅ | UI for all of the above | Repo settings (Scripts / Agent / Files and env), composer, header, Settings |
| ✅ | Ports and workspace env vars | `NEKOTRAIL_PORT` (+9), `NEKOTRAIL_WORKSPACE_NAME/PATH`, `NEKOTRAIL_ROOT_PATH` |

## Round 2: git and PR actions

| Status | Feature | Notes |
|---|---|---|
| ✅ | Commit and push (⌘⇧Y) | |
| ✅ | Create PR written by the agent (⌘⇧P) | Repo PR prompt |
| ✅ | Merge PR (⌘⇧M) | `gh pr merge`, blocked on failing checks |
| ✅ | Pull latest target branch (⌘⇧L) | Merge `origin/<target>` |
| ✅ | Fix errors with the failing check logs (⌘⇧X) | `gh run view --log-failed` |
| ✅ | Agent reviews the diff (⌘⇧R) | Repo review prompt |
| ✅ | Open PR in browser (⌘⇧G) | |
| ✅ | New workspace from a PR or branch (⌘⇧N) | |
| ✅ | Archive on merge | |

## Round 3: chat

| Status | Feature | Notes |
|---|---|---|
| ✅ | Checkpoints and rewind | |
| ✅ | @ file mentions, attachments, pasted images | |
| ⬜ | Plan card: approve and run (⌘⇧↵) | Approve switches the chat to auto-accept and sends "go" |
| ⬜ | Queue: edit, delete, run next | |
| ✅ | Enter sends (setting), Shift+Enter newline | |
| ⬜ | Slash commands: /clear /compact /plan /add-dir | |
| ⬜ | Search in chat (⌘F), copy transcript | |
| ⬜ | Fork chat into a new tab | `--fork-session` |
| ⬜ | Chat tabs: ⌘T new, ⌘W close | |
| ⬜ | Todo list from the agent | `TodoWrite` |
| ⬜ | Expand all tool calls | |
| ⬜ | Tool approvals mid-turn | Needs Claude's streaming input; later |

## Round 4: diff and review

| Status | Feature | Notes |
|---|---|---|
| ✅ | Line comments, send to agent | |
| ⬜ | Uncommitted-only toggle | |
| ⬜ | Split view, ↑/↓ between files, mark viewed | |
| ⬜ | GitHub PR review comments in the diff | `gh api` review comments |
| ⬜ | Quick open (⌘P) and edit a file | |

## Round 5: terminal, run, ports

| Status | Feature | Notes |
|---|---|---|
| ⬜ | Terminal tabs | |
| ✅ | Ports per workspace (`NEKOTRAIL_PORT` and 9 more) and workspace env vars | |
| ⬜ | Run / stop toggle (⌘R), open localhost (⌘⇧B) | |
| ⬜ | Setup log and rerun setup | |
| ⬜ | Spotlight: mirror a workspace into the main checkout | Uses checkpoints |

## Round 6: sidebar, shortcuts, settings

| Status | Feature | Notes |
|---|---|---|
| ⬜ | Conductor's shortcuts, so muscle memory carries over | ⌘J terminal, ⌘⇧D diff, ⌘1-9 workspaces, ⌘⌥↑/↓, ⌘B sidebar |
| ⬜ | Group sidebar by status | |
| ⬜ | Changed-line counts in the sidebar | |
| ⬜ | Default model and effort for new chats | |
| ⬜ | More "open in" targets (Zed, Xcode, iTerm, Warp, Ghostty) | |

## Skipped

Cloud workspaces, organisations, assignees, port forwarding to remote machines, Routines, Pokes, iOS,
remote control, billing. Themes beyond the one dark theme. Linear (needs an account integration).
