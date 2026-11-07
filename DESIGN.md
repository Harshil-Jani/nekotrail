# Design

NekoTrail follows Apple's macOS Human Interface Guidelines: it should feel like Finder, Mail or Xcode, with the cat as its personality. The palette is **Moonlight**: midnight indigo surfaces, moonlight lavender for the brand, and one warm lantern gold for what needs you. Light and dark follow the system. Everything is defined as tokens at the top of `src/App.css`; the values come from `brand/palette.json` (`themes.dark`, `themes.light`), and `tests/brandColors.test.ts` keeps the two in step.

## Foundations

- **Type:** the system font (SF Pro) at 13px for UI, 13.5px for chat text, 15px bold for window titles, 11px semibold for section labels. Sentence case everywhere, no uppercase labels. SF Mono for code, branches and paths only.
- **Colour:** Moonlight.
  - Surfaces: content `--bg` (`#101026` dark, `#FDFDFF` light), sidebar and grouped cards `--surface` (`#161634`, `#F3F3FA`), raised controls `--raised` (`#1F1F45`, `#FFFFFF`). Hairlines `--line` and `--line-strong` are indigo, not grey.
  - Labels: `--text` (`#F0F0FF`, `#14143A`), `--text2`, `--muted` (`#9C9CCC`, `#545485`) and `--faint` for decoration only.
  - Brand: `--accent` is Moonlight `#CCCCFF` on dark with ink text `#1A1A4A`, and indigo `#3D3D85` with white text on light. It fills the primary action, menu hover and the selected palette row, and draws focus rings (`#CCCCFF`, `#5C5C99` on light) and links (`--accent-text`).
  - Lantern gold, `--lantern` (`#FFD27A`; `#B07A00` for fills and `#7A5500` for text on light), is the one warm colour and only means "look here": the Needs you section, unread dots, agent questions waiting on you (approval cards), the away banner, Preview annotations and stars that shine. Never a button fill.
  - State: `--good` green (`#5EE0A0`, `#0B6A3B`), `--bad` rose (`#FF7A8E`, `#B51E3B`), `--wait` amber-orange for warnings (`#FF9F5A`, `#9A4A00`, kept apart from the lantern), `--purple` for merged PRs, each with a `-soft` fill. Merge and destructive fills use `--good-fill` and `--bad-fill` with white text in both appearances.
  - Every text pair passes WCAG AA (4.5:1) on bg, surface and raised; the numbers are in the comment at the top of `src/App.css`.
- **Materials:** the sidebar is native vibrancy in the app (`html.native`, Tauri window effect `sidebar`) tinted with the sidebar colour, and solid `--surface` in the browser preview. Menus, the palette and popovers use `--material`, translucent with a 30 to 40px blur. Content is opaque.
- **Shape:** continuous rounded corners. 4px for tiny chips, 6px for controls and sidebar rows, 8px for menus and code blocks, 10px for cards and popovers, 12px for sheets and the composer. Only avatars and dots are circles.
- **Depth:** controls get a 0.5px hairline and a soft 1.5px shadow. Popovers and sheets get a large soft shadow. No coloured glows, with one exception: stars. Lit and shining stars in the night sky (and on the site) may glow in their own colour.
- **Spacing:** 4, 8, 12, 16, 20, 24, 32.

## Components

- **Window:** overlay title bar. The traffic lights sit in the sidebar head (84px left inset). The 52px toolbar and sidebar head are drag regions.
- **Sidebar:** a calm source list, after Conductor. Top: Search, Board, Queue and Usage as plain icon rows. Quiet 11px section labels (Pinned, your sections, Workspaces with its ⋯ menu). Repo rows carry the owner's GitHub avatar (rounded square, cached on disk, letter fallback) and a + to create a workspace. Workspace rows are one line, 32px, indented under their repo: a muted branch icon, the title in the secondary label colour, and one trailing mark (spinner, lantern dot, red checks dot, or the PR icon). Everything else is in the row's tooltip and right-click menu. Selection is a soft filled rounded row. Footer: you, Archived, Settings.
- **Toolbar:** title and subtitle on the left; Compare, Run, Open in and a "⋯" menu on the right.
- **Tabs:** Chat, Changes, Terminal, Preview and Checks form a segmented control. Preview shows the workspace's local dev server; Annotate marks it up in lantern gold with a dark halo, so the marks read on any page. Checks lists test, lint and typecheck results as grouped cards; a red dot on the tab and the sidebar row means one fails. Chat and terminal sessions are compact rounded tabs.
- **Buttons:** push buttons are 26px. The primary action is accent-filled. Quiet and icon buttons are borderless with a hover fill. Merge is system green.
- **Composer:** one 12px rounded field with its controls inside: agent, one model and intelligence picker ("Opus 5.5 · High", ⌘/), permissions, attach, context ring and Send.
- **Menus:** 5px inset, 24px rows, and the accent fill on hover, as in macOS menus.
- **Cards:** plans, errors, setup checks and review comments are grouped surfaces with no border. A tinted inner ring marks emphasis.

## Exceptions

- The terminal (`--term-bg`) and the night sky stay dark in both appearances. The sky runs from Midnight `#08081A` at the top to Dusk `#3A3A80` at the horizon; lit stars are Starlight `#EEEEFF`, stars that shine (merged) and the lines between them are lantern gold.
- The mascot keeps its own colours: the Starlight tabby, silver fur `#ECECFF` with indigo `#3D3D85` stripes, outline, whiskers and tail, dark eyes `#14143A` with a white glint (so cursor-follow reads), peach blush, and lantern-gold commit stars on the merge tail. Its outline is 2.6 units, 3.2 below 20px so the silver cat keeps its edge on light rows. `src/components/Mascot.tsx` draws it; `scripts/brand.ts` renders the same component into the brand kit.
