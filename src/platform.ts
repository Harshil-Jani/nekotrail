// macOS or not. On a Mac every helper here returns exactly what the code did before Linux support:
// ⌘ is the command key and hints keep their Mac glyphs. Elsewhere (Linux) Ctrl takes ⌘'s place.

type Nav = { platform?: string; userAgent?: string };

/** WebKitGTK reports "Linux x86_64" here and WKWebView "MacIntel"; the user agent is the fallback. */
export function detectMac(nav: Nav | undefined): boolean {
  if (!nav) return true;
  return /Mac|iPhone|iPad/.test(nav.platform || nav.userAgent || "Mac");
}

export const IS_MAC = detectMac(typeof navigator === "undefined" ? undefined : navigator);

/** The app's command key is down: ⌘ on macOS, Ctrl on Linux. */
export const mod = (e: { metaKey: boolean; ctrlKey: boolean }) => (IS_MAC ? e.metaKey : e.ctrlKey);

/** Init for a synthetic KeyboardEvent that holds the command key. */
export const modKey = (): { metaKey: true } | { ctrlKey: true } => (IS_MAC ? { metaKey: true } : { ctrlKey: true });

/**
 * ⌃1 to ⌃9 switch workspace. On Linux Ctrl+1..5 already switch tabs (⌘1..5 on a Mac), so the
 * workspace keys move to Alt+1..9.
 */
export const isWorkspaceDigit = (e: { metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; key: string }) =>
  /^[1-9]$/.test(e.key) && !e.metaKey && !e.shiftKey && (IS_MAC ? e.ctrlKey && !e.altKey : e.altKey && !e.ctrlKey);

/**
 * ⌘O opens the workspace in your editor. On Linux Ctrl+O is taken by "show tool output" (Claude
 * Code's own key), so Open in is Ctrl+Shift+O there.
 */
export const isOpenIn = (e: { shiftKey: boolean }) => (IS_MAC ? !e.shiftKey : e.shiftKey);

const MODS: Record<string, string> = { "⌘": "Ctrl", "⌃": "Ctrl", "⌥": "Alt", "⇧": "Shift" };
const KEYS: Record<string, string> = { "↵": "Enter", "⌫": "Backspace" };

/** Linux spelling of one Mac shortcut, e.g. "⌘⇧K" → "Ctrl+Shift+K", "⌃3" → "Alt+3". */
function linuxCombo(mods: string, key: string): string {
  if (mods === "⌃" && /^[1-9]$/.test(key)) return `Alt+${key}`; // see isWorkspaceDigit
  if (mods === "⌘" && key === "O") return "Ctrl+Shift+O"; // see isOpenIn
  const names = [...mods].map((m) => MODS[m]);
  const k = KEYS[key] ?? key;
  return [...names, k].filter(Boolean).join("+");
}

/**
 * A shortcut hint for this platform. Written in Mac glyphs everywhere in the UI; on a Mac it comes
 * back unchanged, on Linux every combo is spelled out ("⌘K" → "Ctrl+K", "⌘⇧↵" → "Ctrl+Shift+Enter").
 */
export function keyHint(s: string, mac = IS_MAC): string {
  if (mac) return s;
  return s
    .replace(/([⌘⌃⌥⇧]+)(↵|⌫|↑|↓|←|→|[A-Za-z0-9]|[,./\[\]\-+=])/g, (_, m: string, k: string) => linuxCombo(m, k))
    .replace(/[↵⌫]/g, (k) => KEYS[k]);
}
