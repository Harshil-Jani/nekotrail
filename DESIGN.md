# Design

nekotrail follows Apple's macOS Human Interface Guidelines: it should feel like Finder, Mail or Xcode, with the cat as its personality. Light and dark follow the system. Everything is defined as tokens at the top of `src/App.css`.

## Foundations

- **Type:** the system font (SF Pro) at 13px for UI, 13.5px for chat text, 15px bold for window titles, 11px semibold for section labels. Sentence case everywhere, no uppercase labels. SF Mono for code, branches and paths only.
- **Colour:** semantic labels (`--text`, `--muted`, `--faint`) as black or white at fixed opacities, so they sit on any material. One accent, the cat orange `#ff9a4d`, used for the primary action, selection in menus, focus rings and unread dots. Accent text uses `--accent-text`, which is darker in light mode for contrast. State uses system green, red, yellow and purple through `--good`, `--bad`, `--wait`, `--purple` and their `-soft` fills.
- **Materials:** the sidebar is native vibrancy in the app (`html.native`, Tauri window effect `sidebar`), and a solid grey in the browser preview. Menus, the palette and popovers are translucent with a 30 to 40px blur. Content is opaque.
- **Shape:** continuous rounded corners. 4px for tiny chips, 6px for controls and sidebar rows, 8px for menus and code blocks, 10px for cards and popovers, 12px for sheets and the composer. Only avatars and dots are circles.
- **Depth:** controls get a 0.5px hairline and a soft 1.5px shadow. Popovers and sheets get a large soft shadow. No coloured glows.
- **Spacing:** 4, 8, 12, 16, 20, 24, 32.

## Components

- **Window:** overlay title bar. The traffic lights sit in the sidebar head (84px left inset). The 52px toolbar and sidebar head are drag regions.
- **Sidebar:** a source list with 11px section headers, inset rounded rows and grey selection. Repo actions appear on hover.
- **Toolbar:** title and subtitle on the left; Compare, Run, Open in and a "⋯" menu on the right.
- **Tabs:** Chat, Changes, Terminal and Preview form a segmented control. Preview shows the workspace's local dev server; Annotate marks it up in cat orange. Chat and terminal sessions are compact rounded tabs.
- **Buttons:** push buttons are 26px. The primary action is accent-filled. Quiet and icon buttons are borderless with a hover fill. Merge is system green.
- **Composer:** one 12px rounded field with its controls inside: agent, one model and intelligence picker ("Opus 5.5 · High", ⌘/), permissions, attach, context ring and Send.
- **Menus:** 5px inset, 24px rows, and the accent fill on hover, as in macOS menus.
- **Cards:** plans, errors, setup checks and review comments are grouped surfaces with no border. A tinted inner ring marks emphasis.

## Exceptions

- The terminal and the night sky stay dark in both appearances.
- The mascot keeps its own colours.
