# Changelog

Every release of NekoTrail, newest first. Each version is tagged on GitHub, and its macOS app is attached
to the [release page](https://github.com/Harshil-Jani/nekotrail/releases).

## Unreleased

**New look**
- New Moonlight theme: midnight indigo in dark mode, indigo in light mode. All text meets WCAG AA contrast in both.
- The cat is now the Starlight tabby: silver fur, indigo stripes, gold stars on its tail. It still naps, works, worries and watches your pointer.
- Gold now means "needs you": the Needs you list, unread dots, an agent asking for your OK, the away banner and your Preview notes. Warnings are orange.
- The night sky is moonlit. Lit stars glow, and merged ones shine gold.
- New app icon, logo, favicon and VS Code extension icons.

**The NekoTrail name**
- The app now writes its name as NekoTrail everywhere you read it: title bar, menus, Settings, ⌘K, alerts, exports and the VS Code extension.
- What you type or set up keeps the lowercase name, so nothing breaks: the `nekotrail` command, `nekotrail.json`, `nekotrail://` links and the app file. Updates install over the app you have.
- The Mac app now requires macOS 13 or newer, as the download page already said.

**Support and contribute**
- Buy me a coffee: a one-off thank you, no GitHub account needed. Find it next to Sponsor in Settings, and in ⌘K.
- On GitHub: a [roadmap](docs/ROADMAP.md), a contributing guide, a security policy, a code of conduct and forms for bugs and feature requests.
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) lists every open-source component the app ships with, and its license.

**Design ship-gate fixes**
- The Queue's "needs you" count in the sidebar is gold, like every other thing waiting on you. It was orange, the warning colour.
- Website: the brand kit shows its logos on first load instead of an empty space until you scroll.
- Website: on phones, the Copy button no longer covers the end of the Terminal commands on the download page.
- Website: the Buy Me a Coffee cup shows on the sponsor page in dark mode.

## 0.21.0 (2026-11-07)

**Moving from Conductor**
- Importing from Conductor now opens a sheet that leads with keeping your work safe: your worktrees live inside Conductor's folder, and Conductor says it will delete all user data when its app shuts down in early 2027, so NekoTrail moves them into its own folder. **Move into NekoTrail** is picked for you; Import only is still there.
- The sheet shows exactly what moves before anything happens: each repo and worktree, its branch, its size and how many uncommitted files it has.
- Moving runs only `git worktree move`. Nothing is ever copied or deleted, and a worktree that can't move stays where it was.
- After each move NekoTrail checks that `git worktree list` shows the new folder, the branch and its head commit are unchanged, and the uncommitted files are the same files with the same content.
- Results are per worktree: Checked, Moved with a difference to look at, or Not moved with the reason in plain words (a locked worktree, submodules, another disk). Try again retries only the ones that didn't move.
- Workspaces imported earlier and left in Conductor's folder get a one-time notice offering the move, on the welcome screen and above the chat. Dismiss it and ⌘K still has the move.
- First launch and the welcome card say the same thing when they find Conductor's data, and open the sheet.
- The sheet notes that NekoTrail can't read Conductor Cloud, which goes read-only on 13 November and shuts down on 20 November 2026: use Conductor's Transfer to local on each Cloud workspace first, then import.
- New guide, [Moving from Conductor](docs/MOVING-FROM-CONDUCTOR.md): what imports, what doesn't, Conductor Cloud, the move, and Conductor's concepts and shortcuts in NekoTrail. The README links it, lists all sixteen agents, Linear and hand-off, and gives the `xattr` install step until the app is notarized (the release notes say the same).

**Design ship-gate fixes**
- The sheet, notices and first run quote what Conductor said (it will delete all user data when its app shuts down in early 2027) instead of saying it deletes the folder, and the guide links Conductor's announcement.
- The sheet opens with plain text instead of a warning box, and the two choices no longer repeat each other. Import only says the worktrees keep depending on Conductor's folder.
- The Conductor Cloud note says how: right-click each Cloud workspace in Conductor and choose Transfer to local.
- The notice above the chat keeps its icon and moves its buttons under the text in a narrow window, instead of squeezing into a column.
- Welcome shows one Conductor card at a time, and its button says Import from Conductor, as first run and ⌘K do.
- A worktree that didn't move shows its reason in normal text with the red icon, and commands in it show as code.
- The list of worktrees shows a soft edge where more rows scroll, and the footer stays on one line next to the buttons.
- Skipped items read "already in NekoTrail, or the folder is gone".

## 0.20.0 (2026-11-07)

**Hand off to another agent**
- New Hand off to another agent (a chat's + menu, ⌘K, or /handoff): another agent, Codex, Gemini or any you have installed, carries on the task in the same workspace and branch, so you don't explain it again.
- Pick the agent and its model, then read the brief NekoTrail wrote before anything is sent, and edit it if you like. It has the original task, what you said since, the agent's last messages, the files changed against the target branch (every repo of a multi-repo task), the last checks, the commands still failing, its last steps, and its todo list or plan.
- The brief is built on your Mac from the chat itself, with no model asked to summarise, and stays under about 6,000 characters: older steps and long messages are shortened first. Things that look like tokens, keys, passwords or emails are hidden, as in Export session, and paths in your home folder show as ~.
- When an agent runs out of usage, or NekoTrail stops it going in circles, the card in the chat offers the hand-off.
- The new chat keeps this chat's mode. An agent that can't ask before each change starts in Plan instead of Ask before changes, so a hand-off never gives an agent more room. Spend caps and protected files apply to it as before.
- The old chat ends with "Handed off to Codex" and the new one starts with "Continued from Claude", each opening the other. The brief sits folded at the top of the new chat, and Export session notes the hand-off.
- From a terminal: `nekotrail handoff <workspace> --to codex` opens the hand-off in NekoTrail with its brief to read, and `--now` sends it straight away. `.` is the workspace you're in.
- The old "Continue in" button on an out-of-usage card, which pasted the raw transcript into the message box, is replaced by the hand-off.

**Design ship-gate fixes**
- The hand-off sheet picks the agent with the same switch as the rest of the app.
- The new chat's mode sits on its own line with a shield, so you see at a glance that a hand-off never gives an agent more room.
- The sheet says the brief is yours to read and edit, and that the new chat links back to this one.
- Both stop cards offer the same "Hand off to another agent…" button.
- "Handed off to Codex" and "Continued from Claude" end with an "Open Codex's chat" link, so the way to the other chat is plain.
- The brief at the top of the new chat shows in the code font, as you read it in the sheet.
- Your messages sit at the right edge of the chat again.

## 0.19.0 (2026-11-07)

**Linear issues**
- New Integrations pane in Settings: connect Linear with a personal API key. The key is kept in your Keychain (on Linux, your keyring), never in NekoTrail's database, logs or exports. Test shows whose key it is.
- New workspace has a Linear source: your open issues first, with search (type ENG-123 to find any issue), Mine or Anyone, a team and a state.
- Picking an issue starts the workspace on Linear's branch name for it, names the workspace after it ("ENG-123 Fix the login loop"), and puts the issue's title, description and link in the first message.
- The PR NekoTrail opens for that workspace says "Fixes ENG-123", so Linear links it.
- The issue moves to In Progress when the agent's first turn starts, if it hasn't started yet. Switch this off in Settings, Integrations. NekoTrail never comments on Linear.
- From your terminal: `nekotrail new --linear ENG-123`, with an optional task after it.
- A bad key, a rate limit or no network each show a plain message, and never hold up the app.
- Linear API keys are hidden in exported sessions and away alerts, like other keys.

**Design ship-gate fixes**
- After you pick a Linear issue, New workspace says the branch is Linear's suggestion, and that the workspace gets the next free name if your repo already has that branch.
- With no issues to show, the list offers the next step: Show anyone's issues, or Show any state.
- Try again sits right after the error message, and the team and state menus keep their width while issues load.

## 0.18.0 (2026-11-07)

**VS Code and Cursor extension**
- New extension (`extensions/vscode`): lists the open repo's workspaces with state, agent, PR and needs-you. Click one to see its changes in the editor's diff view, or open its worktree in a new window.
- New task from selection sends the selected code and its file and line to NekoTrail. NekoTrail asks "Start Claude from your editor?" and nothing runs until you press Start agent.
- Send selection to workspace chat puts the code in a workspace's message box; you add to it and press Send.
- A status bar item shows how many workspaces need you. The extension talks only to NekoTrail on your machine, over the terminal command's private socket, and sends no telemetry.

**Away alerts**
- New away alerts (Settings, Notifications): get a short message on your phone or in a team chat through ntfy, Slack, Discord, a webhook or Pushover. Off until you add one.
- Pick the events for each destination: needs you, turn finished, PR checks failed or green, PR merged, queue done, a spend cap, a runaway agent stopped, and a morning summary of the queue.
- Each message names the workspace, its repo and what happened, with a link that opens it on your Mac. It never contains your code, prompts, commands or diffs. "Include the agent's last line" is off by default, and hides secrets in that line as Export session does.
- Addresses and tokens are kept in your Keychain (on Linux, your keyring), never in NekoTrail's database or logs.
- Several updates within a minute arrive as one message, and quiet hours hold messages until they end. "Send a test" checks a destination, and the last error shows under it.

**Design ship-gate fixes**
- Away alerts state what a message holds and where its address and tokens are kept once, in a box under the heading, where you add a destination. A saved destination shows a lock next to where it sends, and one still to set up can't be switched on until you save it.
- "Include the agent's last line" now says the line can mention your code, and that anything that looks like a key or token is hidden.
- Quiet hours and the morning summary sit under "When to send".
- "Start Claude from your editor?" names the file and lines the code came from, and for a long task how many lines and characters it has. Code in the task shows in a code font.
- The "Your selection is in the message box" note sits under the chat tabs, so it no longer covers Send.
- A finished turn's cost says it is API-equivalent, and Usage with no turns yet says so for agent time instead of 0 s.

## 0.17.0 (2026-11-07)

**Spend you can trust**
- New Usage view (sidebar, ⌘K): tokens, API-equivalent cost, turns and time per day, repo, agent and model, each agent's usage limit and when it resets, and Export CSV. Kept on your Mac only.
- Costs say they are API-equivalent: on a subscription you pay a flat fee. Agents that report no cost show turns and tokens, never $0.
- Spend caps, for every repo or one repo: a daily cap and a cap per task, in dollars, turns or tokens. When one is reached, the queue starts nothing new and autopilot stops, and NekoTrail says so. A running turn always finishes, and your own messages always go through.
- Runaway protection: when the queue or autopilot sees the same command fail 4 times in a row, a file go back and forth, or 3 turns that change nothing, it stops giving the agent turns. The task moves to Needs you with what it saw.

**A first run that gets you to your first answer**
- New first run, one screen at a time: the agents on your Mac, where your code is (Conductor, terminal chats or a local repo, whichever it finds), then a first task suggested from your repo, run in Plan mode so nothing changes.
- A short tour of the main parts after your first workspace opens. It shows once; ⌘K, "Show the tour" brings it back.
- Empty sidebar, board, night sky and storage now say what to do next.
- Copy diagnostics now includes how long the first run took to the first agent answer. It's kept on your machine only.

**Export session**
- New Export session (⋯ menu, ⌘K, or a chat's + menu): a report of what the agent did, with the task, the plan, a timeline with one line per command or file and failed commands marked, the changed files with key hunks, checks, time, cost and the PR link.
- Copy or save it as Markdown, or save one HTML file that works offline in light and dark, with no scripts and nothing loaded from the web.
- Before you share, a preview lets you leave out tool outputs and file contents. Things that look like tokens, keys, passwords, .env values or emails are hidden, and you can show any one of them again. Paths in your home folder show as ~.
- Add to PR description puts a short summary (what changed, how it was tested, which agent, cost) in the PR form. Nothing is posted until you press Push and create PR.
- A small "Made with NekoTrail" line ends each export. Switch it off in Settings, General.
- Fix: Export prompts in Settings can save its file again.

**Design ship-gate fixes**
- The first run's steps stay in one place from screen to screen, and every missing agent shows its install command with the sign-in line under it.
- While the first run is open, the sidebar only says where your repos will show up, so the buttons to add code are in one place.
- Try it: the note about the new workspace and the tour sits above Start in Plan mode, and the Plan mode label is easier to read in dark mode.
- The tour's card has an arrow pointing at what it describes, and its ring stays inside the window.
- With no repos yet, the empty board and night sky offer Add a repo, and the board has one button instead of two. Storage with no workspaces reads as one short note.
- Export session: Add to PR description is off when no chat has a message yet.
- The queue shows one Add task when it is empty and one Resume when it is paused. The repo switcher leaves out "-0", and a task across repos says "3 repos" in the sidebar.
- Usage: the Today and period totals say how many turns reported no cost, so a partial number never reads as your whole spend. The view says at the top when a daily cap is reached, and a cap you typed wrong is outlined with the reason under it.
- An agent that reports no cost never shows as $0.00: the sidebar, Compare and the agent scorecard say Not reported, and Compare only names the cheapest result when every result has a cost.
- A chat stopped at a spend cap has Change caps, which opens Usage.
- The first run's main button no longer looks pressed on load, and the "You lit" star card sits above the composer so Send stays in reach.
- An export of an empty workspace says "none" for agent time and changes, and Changes and Commit leave out "+0" and "-0".

## 0.16.0 (2026-11-07)

**Tasks across repos**
- New workspace, "Several repos": pick 2 to 5 repos and NekoTrail makes a workspace in each, all on one branch. The sidebar shows them as one task.
- One chat works across all of them: the agent runs in the main repo and can read and change the others. Claude, Codex, Gemini and Copilot can do this; agents that only open one folder aren't offered.
- Changes and Checks have a repo switcher, and Ports lists each repo of the task by name.
- Create PRs opens one PR per repo, and every description links to the others ("Part of a change across: org/api#12, org/web#34").
- Merge all merges in the order you pick, only when every PR is green, and stops at the first one that fails.
- Archive takes the whole task, and every repo keeps its branch. Protected files and repo trust hold per repo.

**Task queue**
- New Queue (sidebar, ⌘K, `nekotrail queue add "task"`): line up tasks and NekoTrail runs 3 at a time, each in its own workspace, starting the next when one finishes or needs you. Set how many run at once overall and per repo.
- Each task picks its repo, branch, agent, model and permissions, and what happens after: run checks, open a draft PR, or autopilot to green.
- When an agent says it is out of usage, its tasks wait until the reset time it gave, or switch to the fallback agent you picked. With no time given, NekoTrail waits 30 minutes and longer each time, and only one task tries again first.
- Tasks set to Ask before changes wait for you to start them. Drag to reorder, pause and resume, retry failed tasks. The queue survives a restart.
- Coming back after a while shows what the queue did: "8 done, 2 need you, 1 failed".

**Design ship-gate fixes**
- The queue's "while you were away" summary is a toast with Open queue, not a window you have to dismiss.
- Tasks that wait for you to start them keep their place number in the line, and Start no longer turns every one of them orange. Retry shows only on failed tasks.
- A new task across repos says in its empty chat which repo the agent works in and that it can change the others.
- Ports heads each repo of the open task "This task, pawprint-web".
- When Merge all stops part way, the PRs it did merge say Merged. Create PRs and Merge all stay plain while their form is open, so there is one button to press.
- The Several repos picker marks picked repos with the tick only.
- Commit says why Amend and Undo are off when the last commit is pushed, the unsaved-rules prompt names the file, and protected-file prompts show paths in the code font.

## 0.15.0 (2026-11-07)

**Import terminal sessions**
- Bring in the Claude Code, Codex and Gemini chats you ran in a terminal: ⌘K, "Import terminal sessions". NekoTrail only reads the CLIs' folders and never changes them.
- Sessions are grouped by the repo and folder they ran in, with sessions, messages, dates and size shown before anything comes in. Tick a whole repo, a folder or single chats.
- A worktree's chats land in a workspace pointing at that folder. Your main checkout's chats land in a read-only History workspace, and no worktree is made.
- Imported chats keep their session, so a new message continues it. They show up in ⌘⇧F search, and importing again only adds new sessions.

**One set of rules for every agent**
- New Rules sheet (⋯ menu or ⌘K): see which rules files your repo has (AGENTS.md, CLAUDE.md, GEMINI.md, Copilot's, Cursor's, Cline's and more), which of the 16 agents reads which, and how each one differs from your main file.
- "Use AGENTS.md for every agent" makes the other files point at one file: an `@AGENTS.md` import for Claude and Gemini, a short note where the agents read AGENTS.md anyway, or a copy marked do-not-edit. You see every change first, and a file with your own words is only replaced after you confirm its diff. Old files are backed up and nothing is committed.
- Edit rules: a markdown editor with a live preview that won't save over a file an agent changed since you opened it.
- Rules for this task: a note per workspace that goes in front of the first message of every new chat there, whichever agent it is.

**Commit composer**
- New Commit view in Changes: stage or unstage whole files, single hunks, or the lines you tick. Your files on disk never change, only what's staged.
- The message box counts your subject line against 72 characters. Draft message asks the workspace's agent for a conventional message from the staged changes, outside your chat. If the agent is busy, the draft comes from the file list instead.
- Split into commits: the agent groups your uncommitted changes into commits. Check each message, move changes between commits, then commit them in order.
- Amend last commit, and Undo last commit (its changes come back staged). Both are off once the commit is pushed.
- Staging or committing a protected file asks first.

**Design ship-gate fixes**
- Commit: "Write a subject line first" is readable, and Split into commits shows each change as a coloured diff with plain labels like "Binary file".
- Rules: the sheet keeps one height on every tab, Who reads what shows each file as its own chip, and when Write is off the footer says which box to tick.
- Import terminal sessions: a chevron on each repo shows it opens to its folders and chats, and agent names are plain grey chips.
- History: the sidebar shows a history icon and no repeated name, and the subtitle says it is your main checkout instead of "main → main".
- Prompts: the / sits inside the Name field. New workspace no longer offers prompts that need a selection from Changes.
- The Checks line in a chat starts with a capital: "Test passed before and fails now."

## 0.14.0 (2026-11-06)

**Checks: tests, lint and typecheck**
- New Checks tab (⌘5): runs your test, lint and typecheck commands in the background and lists each failure with its file, line and the code around it. The raw output is one click away.
- "Fix with agent" sends one failure to the agent with its code; "Fix all" sends them all in one message.
- No checks yet? NekoTrail suggests them from the repo (npm, bun, pnpm, yarn, cargo, go, pytest, ruff, mypy, make) and saves the ones you tick.
- A `checks` section in `nekotrail.json` shares checks with the team. Like its scripts, they ask before they run.
- "Run after every agent turn" puts the result in the chat. If a check that passed before now fails, the workspace moves to Needs you. A dot on each sidebar row shows passing, failing or running.

**Prompt library**
- Save a prompt from the chat box or from any message you sent (⋯, Save as prompt), with a name, a description and blanks like `{{ticket}}` or `{{env:staging}}`.
- Type `/name` to insert one, or use ⌘K or New workspace. Tab moves between blanks, and `{{branch}}`, `{{base}}`, `{{files_changed}}` and `{{selection}}` fill themselves.
- Share prompts through git in `.nekotrail/prompts/*.md` or a `prompts` list in `nekotrail.json`. They're read-only in the app and marked Repo.
- Your Claude Code commands show in the `/` menu of Claude chats, and Claude runs them itself. New Prompts pane in Settings: edit, duplicate, delete, export and import. Your old snippets move over by themselves.

**Review your teammates' PRs with an agent**
- New "Review requests" in the sidebar and ⌘K: open PRs where someone asked for your review, in the same GitHub check as your own PRs.
- One click starts a review: the PR's code in a read-only workspace, and the agent you pick reviews it. Its findings land in Changes as draft comments, with a suggested change when it knows the exact fix.
- Edit or delete any draft, then Submit review posts them to GitHub as one review: Comment, Approve or Request changes. Nothing is posted until you press it, and the workspace is archived after.
- A review workspace can't push: every chat only reads, `git push` fails for the agent and the terminal, and push, merge and autopilot are switched off there.

**Ports for each workspace**
- Each workspace gets 10 ports of its own (4100 to 4109, 4110 to 4119, ...), kept across restarts and given back when you archive it. Five workspaces can run the same app without fighting over 3000.
- Terminals, scripts and agents get `NEKOTRAIL_PORT`, `NEKOTRAIL_PORT_1` to `_9`, and `PORT`. A `PORT` you set yourself is never replaced. Name ports in `nekotrail.json`: `"ports": {"web": "PORT", "api": "API_PORT"}`.
- The new Ports button shows what listens on every workspace's ports, opens one in your browser or Preview, and frees a port by stopping a server that workspace started.
- If your dev server ignores `PORT` and lands on its default, or finds another workspace already there, NekoTrail says so and shows the fix. Preview opens the workspace's port until your app prints an address.

**Drive NekoTrail from your terminal**
- New `nekotrail` command: `nekotrail .` opens a repo, `nekotrail new "fix the login bug" --agent codex` starts an agent in a new workspace, and `nekotrail ls`, `open` and `status` show your workspaces and what needs you. Install it from Settings, General. NekoTrail starts if it isn't running.
- `nekotrail://open?workspace=...` links jump to a workspace. `nekotrail://new?repo=...&prompt=...` links ask first and show the full task, so a web page can never start an agent on its own.
- The command reaches the app through a socket only your user can open, never a network port.

**Linux (new, experimental)**
- NekoTrail runs on Linux (x86_64), shipped as a `.deb` and an `.AppImage` that update themselves from inside the app.
- On Linux, shortcuts use Ctrl where the Mac uses ⌘. Switching workspace is Alt+1 to Alt+9 and Open in editor is Ctrl+Shift+O. The terminal copies and pastes with Ctrl+Shift+C / Ctrl+Shift+V.
- On Linux, discarded files go to the desktop's Trash, Open in finds your editors and terminals on PATH, and agents keep the computer from idling to sleep.

**Search every chat**
- Search every chat with ⌘⇧F: your prompts, the agents' replies, the commands they ran and the files they touched, in every workspace, archived ones too.
- Filter by repo, agent or date, or show only your messages or only commands. Enter opens the chat at that message and highlights it.
- Chats from before this version are indexed once in the background. The search sheet shows how far it has got, and the app opens as fast as before.
- Archived workspaces open read-only, so you can read their chats without restoring them.
- ⌘F searches the whole chat, not just what's loaded, finds commands and file paths too, and highlights each match.

**Design ship-gate fixes**
- A long workspace name ends in an ellipsis instead of running under the tabs, and the toolbar turns its buttons into icons before it cuts the name short (for example a review workspace at 1100 px).
- Error toasts are readable: white text on red in light mode.
- A review workspace's chat says it only reads: "Ask about #731", a matching placeholder, and a Read only chip where the permissions menu was.
- Review drafts show a suggested change as code, the way GitHub does, instead of the raw suggestion block. Discard is gone from read-only reviews, and Submit review says why it is off (for example a request for changes with no summary).
- ⌘F rings the current message with a rounded outline that sits clear of the text.
- Ports: the popover grows to fit every workspace, free ports have no dead buttons, and freeing a port uses a stop icon.
- "Start this task?" from a NekoTrail:// link lists the repo, folder, branch and agent on their own lines, gives the task more room, and says when there is more to scroll.
- An archived workspace says "Archived, read only" next to Restore.
- Search results show replies as plain text, without markdown marks like bold stars and backticks.
- Checks: the bar lines up with the cards, Fix all is the main button when something fails, a running or waiting check says so instead of showing its last result, and Allow these commands and Add checks are the main buttons on their cards. With no checks yet, the empty page says it once and keeps its text to a readable width.
- Settings, Prompts: a little more room between the buttons and the note under them, and a prompt whose blank sits on its own line no longer shows a stray "…" in its preview.
- Review with lists the agents you have first, then the rest under "Not installed".
- Settings, General calls the command line tool "Terminal command", and a port problem keeps a command like `$PORT` on one line.

## 0.13.0 (2026-11-06)

**Protected files**
- Protect files per repo (`.env*`, `migrations/**`, `infra/**`) in Repo settings or in `nekotrail.json`. The repo file can add to your list but never remove from it.
- Claude and OpenCode can't edit protected files, in any mode. Other agents are refused whenever they ask first, and every agent's turn is checked afterwards.
- If a turn changes a protected file anyway, the chat says which one, the workspace moves to Needs you, and "Put it back" restores just those files. The agent's copies go to the Trash.
- New "Commands the agent ran" view for each workspace, in the ⋯ menu and ⌘K.
- The first time you pick Full auto, NekoTrail explains what it means and asks once.

**Twelve more agents**
- Cursor, GitHub Copilot, Goose, Qwen Code, Kimi, Auggie, Grok, Kiro, Droid, Mistral Vibe, Cline and Kilo join Claude, Codex, Gemini and OpenCode. Pick one for a workspace or chat; each uses your own login, and setup shows its install command if it's missing.
- The agent pickers show Claude, Codex and the agents installed on your Mac; the rest are one click away under "More agents".
- "Ask before changes" works for Copilot: each edit or command waits for Allow, Deny or "Allow for this chat".
- "Auto-accept edits" approves file edits inside the workspace for these agents; commands still ask first.
- Repo settings these agents would run (hooks, MCP servers, allow rules) ask before the agent starts. Until you trust them, that agent doesn't run in the repo.
- Setup checks all agents at once, so it stays quick with sixteen.

**Workspaces named after their task**
- The first message names a workspace (no AI call, stays on your Mac), then the PR's title replaces it. A name you type is never replaced; clear it to go back to automatic.
- The star moves under the title in the sidebar, the toolbar and ⌘K, and you can still search by it. Existing workspaces keep their names; untitled ones with a chat are named once.

**MCP sheet polish**
- The sheet fades at the bottom when there's more to scroll, clears an old error when you change how a secret comes across, and its Skills note is shorter.

**Design ship-gate fixes**
- Sidebar rows give the task title the full width: line counts, the overlap mark and the pin sit on the star's line underneath, and two-line rows are 36 px instead of 40, so more workspaces fit.
- Compare shows the shared task right under its title, and the "Same files as" note in the PR bar quotes task titles like the rest of the app.
- Setup lists the agents you have first, and "more agents you can add" folds open in line with the other checks.
- "Installed" and "Not installed" in the agent menus line up with the agent names. Settings, Agents says other agents show up once they're installed.
- The Settings footer reads as one line (version, What's new, Check for updates), with Sponsor set apart by a heart.
- "Commands the agent ran" wraps long commands in full instead of drawing them over the next row, and with a filter on, Copy names how many it copies.
- Repo settings keeps Save in view on long tabs and stops jumping when you switch tabs. Protected files says once that every turn is checked after it ends, so each agent's row only says what's different.
- Protected file cards name each file once. After Put back, a file the agent created reads "removed", and the confirm says by name which files go back and which go to the Trash.

## 0.12.1 (2026-11-06)

**Sponsor**
- NekoTrail is free and open source. If it saves you time, you can now sponsor it on GitHub Sponsors: Settings, next to the version, or "Sponsor NekoTrail" in ⌘K.

**Faster, quieter GitHub checks**
- The PR bar, sidebar, board and autopilot share one GitHub check: one `gh` call per repo for all its workspaces. It runs every 20 s while checks run or autopilot is on, every 5 min otherwise, and pauses while the window is hidden.
- PR state updates right after you push, open a PR, merge, or an agent finishes a turn. Running out of GitHub API quota no longer shows errors; NekoTrail waits for the reset and keeps the last answer.

**Share MCP servers and skills**
- The MCP sheet (`/mcp`) shows every server your agents have and copies one into another agent in one click. It shows exactly what goes in, backs up your settings file first, and changes nothing else in it.
- Values that look like secrets are never copied without asking: bring each one across as a variable from your shell, or copy the value.
- MCP servers share with OpenCode too: add Claude, Codex or Gemini servers to OpenCode, and copy OpenCode's servers to them, with variables converted or refused with a reason. A new settings file is previewed in full before it is written.
- Share Claude skills with Codex and Gemini in one click. It's a link in `~/.agents/skills`, so the skill lives in one place and edits show up for every agent. OpenCode already reads `~/.claude/skills` and `~/.agents/skills`, and the sheet says so.

**Design ship-gate fixes**
- The MCP sheet shows one column per agent (Claude, Codex, Gemini, OpenCode) with a tick where an agent already has a server or skill and an Add button where it doesn't, so rows line up and never wrap.
- Servers an agent can't take say "Can't add" right in the row; open it to see why.
- If a settings file changes between the preview and Add, the sheet shows the preview again from the file as it is now, with the reason next to the button.
- A settings file NekoTrail can't read gets a Try again button, and the sheet stays anchored at the top as you move between the list and a preview.

## 0.12.0 (2026-11-06)

**OpenCode, and any ACP agent**
- OpenCode joins Claude, Codex and Gemini. Pick it for a workspace or chat; it uses your own OpenCode setup, and setup shows its install command if it's missing.
- "Ask before changes" works for OpenCode: each edit or command waits for Allow, Deny or "Allow for this chat", and Stop cancels cleanly mid-turn.
- Repo settings that would make OpenCode run commands (MCP servers, plugins, custom tools, formatters, `allow` permissions) ask first; until you trust them, OpenCode runs with them switched off.

**Appearance**
- New Appearance pane in Settings: Light, Dark or Follow system, and zoom from 80% to 150% (⌘+, ⌘- and ⌘0 still work anywhere).
- Pick the font and size for code (diffs, code blocks in chat, tool output) and for the terminal, from the monospace fonts on your Mac. Terminals change right away.
- Mermaid diagrams in chat have a Diagram / Source toggle and redraw when you switch between light and dark.

**Review the agent like a pull request**
- Click a line number in Changes (shift-click or drag for more lines) to leave a comment. Drafts stay with the workspace, even after a restart.
- "Send review" sends all your comments to the agent in one message, with each file and line, the code and your note. If the agent is busy, it waits in the chat box for Queue.
- Comments on lines the agent has since changed are marked Outdated, with the old code shown, and can be cleared in one click.

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

**Design ship-gate fixes**
- Side by side in Changes wraps long lines instead of cutting off the code, so it stays readable in a 1100 px window.
- In small windows the PR bar keeps the name in "Same files as ..." and shortens the branch first. The bar keeps its height above the "Move onto main" and merged banners.
- Gemini's model picker lists Default once, the agent choice "Both" now reads "Claude + Codex", and "On a workspace" starts with the workspace you have open.
- Agent defaults in Settings line up in columns, and the comment box in Changes has one focus ring instead of two.
- The toolbar and the PR bar never clip at 120% or 150% zoom or in narrow windows: labels turn into icons first, then Open in, Run and Compare move into the ⋯ menu. The PR bar drops the branch (the toolbar shows it) before cutting it to a few letters, so "Dash builds on this" reads in full.
- The + menu in the chat tabs shows all four agents again instead of being cut off under the tab strip.
- OpenCode's model picker lists Default once and says the model comes from OpenCode's own config.
- Preview in a repo without a Run script says how to set one, with a button to the repo settings, instead of offering to start a script that doesn't exist. Its buttons are plain, so the PR bar keeps the one orange button.
- Changed files show the file name first with its folder dimmed after it, instead of cutting the path from the left.
- When the file filter hides the open file, the first match opens; with no match the diff pane says so and offers Clear filter.
- The rewind sheet puts "1 commit comes off the branch" on its own line.
- Disabled orange and green buttons are a quiet grey in both appearances instead of muddy brown in dark mode.
- Mermaid diagrams are flat (no node shadows) with larger labels, and very wide ones scroll instead of shrinking past readable.
- At high zoom the chat box's controls wrap to a second line, so Send and Stop never go out of view, even in the smallest window at 150%.
- The status dots in the workspace ⋯ menu are dots again, not long coloured bars.

## 0.11.0 (2026-11-06)

**Updates**
- NekoTrail now tells you when a new version is out: a small card with the cat, what's new, and Update or Not now. "Not now" waits a day before asking about that version again; a newer one asks right away.
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
- Autopilot: turn it on from the PR menu and NekoTrail keeps the PR moving on its own. Failing checks go to the agent with their logs, conflicts get merged and resolved, requested changes get answered. It stops after 3 agent turns or when the PR is green, and tells you with a ting and a notification.
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
- Spend in the sidebar counts only real NekoTrail usage.

## 0.1.0 (2026-11-05)

First version: workspaces as git worktrees, Claude Code and Codex chats, diff view, terminal, PR status
from GitHub, setup and run scripts, the Conductor importer, the cat and the night-sky quest.
