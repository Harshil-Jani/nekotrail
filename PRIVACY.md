# Privacy

NekoTrail runs on your machine. It has no account, no server of its own and no telemetry: no analytics,
no crash reporting, no usage pings. Nothing you do in it is sent to the NekoTrail project.

## Every network connection NekoTrail makes

| Connection | When | What the other side sees |
|---|---|---|
| **Update check** to GitHub Releases (`github.com/Harshil-Jani/nekotrail/releases`) | On launch and every 6 hours, and when you choose Check for updates. The update itself downloads only when you accept it. | Your IP address and NekoTrail's version, as with any web request. |
| **Repo avatars** from GitHub (`github.com/<owner>.png`, which redirects to `avatars.githubusercontent.com`) | For repos with a GitHub remote, about once a week per owner. The picture is cached on disk. | Your IP address and the repo owner's name. |
| **Images in chats and pull requests** from GitHub (`github.com` and its image hosts on `githubusercontent.com`) | When a message or PR description you open contains one. No other remote images can load. | Your IP address and the image address. |
| **`git` and `gh`** (your own installs, with your own logins) | When NekoTrail fetches a base branch, pushes, opens or merges a PR, or checks PR and CI status for repos with a GitHub remote. | Your git host and GitHub, exactly as if you ran the same commands yourself. |
| **Linear** (`api.linear.app`) | Only after you connect Linear in Settings, Integrations: listing your issues, and moving one to In Progress when you start it (you can turn that off). | Linear, under your own API key. |
| **Away alerts** (ntfy, Slack, Discord, Pushover or a webhook you enter) | Only after you add an alert in Settings, Notifications. | That service receives a short summary of what happened. ntfy, Slack and Discord also fetch the app icon from the project website to show beside it. NekoTrail scrubs secrets it can recognise first, but this is pattern matching and best effort. |
| **Agent CLIs** (Claude Code, Codex, Gemini CLI, OpenCode and the rest) | When you start an agent. | Each CLI talks to its own provider under your own login or key, under that provider's terms and privacy policy. NekoTrail does not proxy, read or change that traffic. |

NekoTrail never reads, stores or forwards your agent logins. Each CLI signs in by itself, and NekoTrail
only starts it.

The app window's content security policy lets it talk only to NekoTrail itself and load images only from
GitHub, so a page or message cannot make it call anywhere else. Preview shows dev servers on `localhost` only.

## What stays on your machine

- **Chats, prompts, agent output, diffs and settings**: a SQLite database in the app's data folder
  (`~/Library/Application Support/dev.nekotrail.app` on macOS, `~/.local/share/dev.nekotrail.app` on Linux).
  The folder is readable by your user account only. Agent output is stored as plain text, so if an agent
  prints a secret, it ends up in that database too.
- **Your worktrees**: `~/nekotrail/workspaces`, ordinary git worktrees on your disk.
- **Secrets you give NekoTrail** (the Linear API key, alert tokens and webhook addresses): your system
  keychain (macOS Keychain or the Secret Service on Linux), never the database.
- **Your shell's environment**: `login-env.json` in the app's data folder, readable by you only. It keeps
  the PATH and variables your login shell set at the last launch, so the window can open without waiting
  for your shell profile. Variables whose names look like secrets (containing KEY, TOKEN, SECRET,
  PASSWORD, AUTH and similar) are left out: they are only ever held in memory.
- **Avatar cache**: small images in the app's data folder.
- **Exports and diagnostics**: only where you put them. A session export is saved to a file you choose,
  after a secret scan you can review. Copy diagnostics puts versions and counts on your clipboard and
  nothing else.
- **Conductor import**: reads your own Conductor data file on this Mac, read-only, and sends nothing.

## Website

The NekoTrail website sets no cookies and runs no analytics or third-party scripts. It is hosted on
GitHub Pages, and GitHub keeps server logs (including IP addresses) under the
[GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).

## Questions

Open an issue at [github.com/Harshil-Jani/nekotrail/issues](https://github.com/Harshil-Jani/nekotrail/issues).
Security problems go through [SECURITY.md](SECURITY.md) instead.
