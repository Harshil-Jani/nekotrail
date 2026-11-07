# NekoTrail for VS Code and Cursor

See your [NekoTrail](https://github.com/Harshil-Jani/nekotrail) workspaces from your editor, open
their worktrees and changes, and hand selected code to an agent. Works in VS Code, Cursor and
other VS Code forks, on macOS and Linux.

The extension talks only to the NekoTrail app on your own machine, through the same owner-only
Unix socket the `nekotrail` terminal command uses. It opens no network port, sends no telemetry and
never starts an agent by itself: a new task always waits for you to press Start agent in NekoTrail.

## What it does

- **A NekoTrail view in the activity bar.** Lists the workspaces of the repo you have open, with
  their state (working, needs you, done and unread, idle), agent and PR. Workspaces that need you
  come first.
  - Click a workspace to see its changes against its target branch in the editor's diff view
    (the same files and base commit NekoTrail's Changes tab uses).
  - Open its worktree folder in a new window, show it in NekoTrail, or open its PR, from the
    row's buttons or its right-click menu.
  - Set `nekotrail.click` to `window` to open the worktree on click instead.
- **NekoTrail: New task from selection** (Command Palette, or right-click a selection). Asks what
  the agent should do, then sends that with the selected code and its `file:line` to NekoTrail.
  NekoTrail shows "Start Claude from your editor?" with the whole task. Nothing starts until you
  press Start agent there.
- **NekoTrail: Send selection to workspace chat.** Puts the selected code, with its `file:line`,
  in a workspace's message box in NekoTrail. A file inside a workspace's worktree goes to that
  workspace; otherwise you pick one. It is not sent: you add to it and press Send.
- **Status bar.** Shows how many workspaces need you, across all your repos. Click it to bring
  NekoTrail forward.
- **When NekoTrail isn't running,** the view and status bar say so, and any command offers to start
  it.

## Install

The extension isn't on the VS Code Marketplace or Open VSX yet. Install the `.vsix` file:

- **VS Code:** Extensions view, `...` menu, Install from VSIX..., pick `nekotrail.vsix`.
  Or in a terminal: `code --install-extension nekotrail.vsix`
- **Cursor:** the same menu, or `cursor --install-extension nekotrail.vsix`

You need a NekoTrail newer than 0.17.0 (it speaks protocol 2). With an older NekoTrail the view says to update it.
On Linux, install NekoTrail's terminal command once (NekoTrail, Settings, General) so the extension
can start the app for you.

## Settings

| Setting | Default | What it does |
| --- | --- | --- |
| `nekotrail.click` | `changes` | What clicking a workspace does: `changes` (diff view) or `window` (open its worktree in a new window). |
| `nekotrail.agent` | empty | Agent for New task from selection: `claude`, `codex`, `gemini`, ... Empty uses Claude. You still confirm in NekoTrail. |
| `nekotrail.refreshSeconds` | `5` | How often the view and status bar ask NekoTrail for news, while the editor window has focus. |

## How it talks to NekoTrail

NekoTrail listens on `<app data>/dev.nekotrail.app/cli/nekotrail.sock`
(`~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux). The
folder is readable by you only, and NekoTrail checks that the process on the other end runs as you.
Each request is one JSON line and gets one JSON line back. The extension uses `hello`, `ls`,
`status`, `show`, `open`, `add`, `changes`, `draft` and `propose`. `propose` goes through the same
question NekoTrail asks for `nekotrail://` links, so it can't start work without you.

## Build from source

You need [Bun](https://bun.sh) and Node.js.

```sh
cd extensions/vscode
bun install
bun test test/        # protocol client, view model, activation
bun run package       # type-check, bundle, and write nekotrail.vsix
```

## Not covered by the automated tests

The tests run the protocol client against a real Unix socket, the view model, and activation
against a stand-in for the editor API. They don't drive a real VS Code or Cursor window, so the
diff view, the new-window open and the menus are checked by hand.

## License

MIT
