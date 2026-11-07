# Simplify: an audit of visible chrome

NekoTrail does a lot, and until now it showed most of it at once. People coming from Conductor told us
finding things takes effort: the sidebar repeats rows, every row carries two lines and five marks, and the
workspace screen stacks three toolbars before the chat starts. This audit lists every control that was on
screen, decides where each one belongs, and records what moved where, so nothing is lost.

The rule: the agent's work is the content (PRODUCT.md, principle 1). A control stays visible only if you
use it on most visits to that screen. Everything else moves to a menu, the right-click menu, or the
command palette (⌘K), and keeps its shortcut.

Screens audited at 1440 x 900, dark, in the preview harness (`preview.html?ws=w1&tab=chat`, `&tab=changes`,
`&settings`, `?view=board`, `&many`).

Legend: **Keep** stays visible. **Menu** moves to a menu named in the row. **⌘K** is in the command
palette (with its shortcut where it has one). **Hover** shows on hover or in a tooltip. **Remove** goes
because something else already says it.

## Workspace screen, before: 53 visible controls and labels

Counted on the default preview workspace (Chat tab, two chats, an open PR, 5 workspaces in 2 repos).

### Sidebar head and search row (10)

| Control | Decision | Where it goes |
| --- | --- | --- |
| Hide sidebar | Keep | |
| The cat (pet it) | Keep | Smaller, the only brand mark in the chrome |
| Memory chip (1.56 GB) | Menu | Account menu at the bottom, with the per-process breakdown as its tooltip |
| Back, Forward | Keep | |
| Search field | Keep | Becomes a plain "Search" row in the top navigation |
| Board button | Keep | A plain "Board" row in the top navigation (⌘⇧K) |
| Filter by repo | Menu | "Workspaces" ⋯ menu, and ⌘K "Show only <repo>" |
| Group by status / repo | Menu | "Workspaces" ⋯ menu, and ⌘K |
| Add repo (+) | Menu | "Workspaces" ⋯ menu, ⌘K "Add local repo", ⌘⌥A |

### Sidebar list (25 on the sample data)

| Control | Decision | Where it goes |
| --- | --- | --- |
| Queue row, with "3 running, 4 up next" subtitle | Keep, one line | Plain "Queue" row; the needs-you count stays as a quiet trailing number, the rest in its tooltip |
| Usage row, with "$11.50 today" subtitle | Keep, one line | Plain "Usage" row; today's spend in its tooltip |
| Pinned header, fold, count | Keep | Quiet section label |
| "Needs you" group (repeats rows shown again below) | Remove the copy | Each row that needs you gets a lantern dot; ⌘⌥L and ⌘K "Go to next workspace with news" still jump there |
| Custom section headers ("This week") | Keep | Quiet section label |
| "Empty. Drag a workspace onto the name." | Remove | An empty section shows its name only; dragging still works |
| "Add section" row | Menu | "Workspaces" ⋯ menu, and ⌘K "Add sidebar section" |
| Repo header: fold, letter avatar, name, star count | Keep, real avatar | The owner's GitHub avatar (cached), name, and a "+" that is always there. Star count moves to the tooltip |
| Repo settings (hover gear) | Menu | Right-click the repo row, and ⌘K "Edit <repo> setup and run scripts" |
| Row subtitle (star name, e.g. "Mochi") | Hover | Tooltip, with repo, branch, lines changed and overlaps |
| Row PR icon (leading) | Move | Leading icon is always the branch; PR state becomes the one trailing indicator |
| Row +/- lines | Hover | Tooltip; the Changes tab still has them |
| Row overlap mark (layers icon) | Hover | Tooltip; the PR row in the header keeps the overlap button with the merge order |
| Row check dot | Merge | The one trailing indicator (red when checks fail) |
| Row pin icon | Remove | The Pinned section already says it |
| Row "3 repos" chip on multi-repo tasks | Hover | Tooltip |

### Sidebar footer (6)

| Control | Decision | Where it goes |
| --- | --- | --- |
| Avatar, name, @login, "10/18 stars" | Keep, one line | Avatar and name; @login and stars in the tooltip and the account menu, which also opens the night sky |
| Low disk warning with "Clean up" | Keep | Only shows when the disk is nearly full |
| "Show archived" | Menu | An icon button in the footer, and ⌘K |
| (new) Settings | Keep | An icon button in the footer, as in Conductor (⌘,) |

### Workspace header and PR bar (21)

| Control | Decision | Where it goes |
| --- | --- | --- |
| Title (15px bold), pin mark | Move | The sidebar row already shows the title; the header shows a breadcrumb `repo › branch`. Rename and Pin are in the ⋯ menu |
| Subtitle "star · repo · branch → target" | Remove | The breadcrumb says repo and branch; star and target branch are in its tooltip |
| Chat, Changes, Terminal, Preview, Checks segmented control | Keep, moved | One tab row under the header, shared with the chat tabs |
| Ports | Menu | ⋯ menu and ⌘K "Show ports" |
| Compare | Menu | ⋯ menu (when the workspace has a pair) |
| Run | Menu | ⋯ menu, ⌘R, ⌘K "Run the app". While the run script is live, a Stop button shows |
| Custom actions chevron | Menu | ⋯ menu |
| Open in (select) | Menu | ⋯ menu, ⌘O |
| ⋯ menu | Keep | Now the one place for everything above |
| Branch "→ main" button in the PR bar | Remove | Duplicate of the breadcrumb. "Rename branch" moves to the ⋯ menu |
| "X builds on this" note | Hover | An icon with a tooltip |
| "Same files as ..." chip | Keep, icon only | Click still opens the merge order |
| PR author avatar | Remove | Tooltip on the PR number |
| Open pill, #number, CI pill, reviewers | Keep | In the header's right side, folding to icons when narrow |
| +38 -1 | Remove | The Changes tab tooltip and the sidebar row tooltip |
| Worried cat when CI fails | Remove | The CI pill is already red |
| Squash and merge, chevron | Keep | The one primary action |

### Chat tabs and composer (12)

| Control | Decision | Where it goes |
| --- | --- | --- |
| Chat tabs, + | Keep | Share the tab row with Changes, Terminal, Preview, Checks |
| Agent badge ("Claude") | Remove | The chat tab names the agent when it isn't Claude; the + menu in the composer says it too |
| Model and effort picker | Keep | |
| Permission select | Menu | Composer + menu. A chip shows only when the mode is not the default (Plan, Ask, Full auto) |
| Context ring (64%) | Hover | Shows only past 70%, when it starts to matter |
| Save as prompt | Menu | Composer + menu |
| Attach | Menu | Composer + menu; paste and drag still attach directly |
| ↵ hint | Remove | Send's tooltip |
| Send | Keep | A round icon button |
| Placeholder "Ask for a change, fix or plan. Type @ ..." | Keep, shorter | "Ask for a change, @mention files, or type / for commands" |

## Workspace screen, after: 27 visible controls and labels

53 to 27 on the same sample data is a 49% cut. Toolbar rows above the chat go from three (52 + 40 + 36 px)
to two (52 + 38 px).

## Other screens

- **Changes:** the file list header (All, Uncommitted, Commit, refresh) stays; the per-file toolbar (Show
  side by side, Edit file, Discard) keeps its labels because each is a distinct, frequent action.
- **Settings:** the tab strip and grouped rows are already calm. The footer row mixed five links with Done;
  Sponsor and Buy me a coffee move to General's "About" row, keeping version, What's new, Check for updates and Done.
- **Board:** unchanged; it is a destination you choose, and its density is the point.

## What we did not remove

Every action listed above is still one click (a menu) or one shortcut away, and all of them are in ⌘K.
Nothing was deleted from the product; the right-click menu on a workspace row still has Pin, Move to
section, Mark as unread, Rename and Archive.
