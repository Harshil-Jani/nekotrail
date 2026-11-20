/**
 * App themes: Moonlight (the default, light and dark) plus presets and your own.
 *
 * A theme is a few base colours. Every other colour the app uses (hover, selection, lines,
 * soft fills, button presses) is worked out from them, so a theme is short enough to share by
 * copy and paste, and an agent can write one with `nekotrail theme`. Text colours are pushed
 * until they read well on the background (WCAG AA), so a theme can't make the app unreadable.
 */

export type Mode = "dark" | "light";

/** The colours a theme sets. Only background, text and accent are required. */
export type ThemeColors = {
  background: string;
  text: string;
  accent: string;
  /** The sidebar. Default: a little darker than the background (lighter on light themes). */
  sidebar?: string;
  /** Cards, menus and popovers. Default: the sidebar colour. */
  surface?: string;
  /** Secondary text. Default: text faded towards the background. */
  muted?: string;
  /** Hairlines between areas. Default: text at low strength. */
  border?: string;
  green?: string;
  red?: string;
  yellow?: string;
  orange?: string;
  purple?: string;
};

/** xterm colours: 16 ANSI colours in order black, red, green, yellow, blue, magenta, cyan, white, then the bright ones. */
export type TerminalColors = {
  background?: string;
  foreground?: string;
  cursor?: string;
  ansi?: string[];
};

export type AppTheme = {
  name: string;
  mode: Mode;
  colors: ThemeColors;
  terminal?: TerminalColors;
};

// ---------- colour maths ----------

type RGB = [number, number, number];

const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;
export const isHex = (s: unknown): s is string => typeof s === "string" && HEX.test(s.trim());

export function parseHex(hex: string): RGB {
  let h = hex.trim().slice(1);
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as RGB;
}

const clamp255 = (n: number) => Math.max(0, Math.min(255, Math.round(n)));
export const toHex = (c: RGB) => "#" + c.map((n) => clamp255(n).toString(16).padStart(2, "0")).join("");

/** a moved towards b by t (0 = a, 1 = b). */
export const mix = (a: string, b: string, t: number) => {
  const x = parseHex(a), y = parseHex(b);
  return toHex([0, 1, 2].map((i) => x[i] + (y[i] - x[i]) * t) as RGB);
};

export const alpha = (hex: string, a: number) => {
  const [r, g, b] = parseHex(hex);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
};

const lin = (v: number) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
export const luminance = (hex: string) => { const [r, g, b] = parseHex(hex); return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b); };
export const contrast = (a: string, b: string) => {
  const x = luminance(a), y = luminance(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
};

/** fg, moved towards black or white (whichever contrasts more with bg) until it reaches `ratio` on bg. */
export function readable(fg: string, bg: string, ratio = 4.5): string {
  if (contrast(fg, bg) >= ratio) return fg;
  const to = contrast("#000000", bg) >= contrast("#ffffff", bg) ? "#000000" : "#ffffff";
  for (let t = 0.05; t <= 1.0001; t += 0.05) {
    const c = mix(fg, to, t);
    if (contrast(c, bg) >= ratio) return c;
  }
  return to;
}

/** Ink for text on a filled accent button: the background colour or white/near-black, whichever reads best. */
const inkOn = (fill: string, bg: string) => {
  const cands = [bg, "#ffffff", "#111111"];
  return cands.reduce((best, c) => (contrast(c, fill) > contrast(best, fill) ? c : best), cands[0]);
};

/** A fill dark (or light) enough that white text on it passes AA. Used for Merge and Delete buttons. */
const fillForWhite = (c: string) => {
  for (let t = 0; t <= 1.0001; t += 0.05) {
    const f = mix(c, "#000000", t);
    if (contrast("#ffffff", f) >= 4.5) return f;
  }
  return "#000000";
};

// ---------- presets ----------

const T = (ansi: string): string[] => ansi.trim().split(/\s+/);

export const PRESETS: Record<string, AppTheme> = {
  dracula: {
    name: "Dracula", mode: "dark",
    colors: { background: "#282a36", sidebar: "#21222c", surface: "#21222c", text: "#f8f8f2", muted: "#a4a8c2", border: "#383a4a", accent: "#bd93f9", green: "#50fa7b", red: "#ff5555", yellow: "#f1fa8c", orange: "#ffb86c", purple: "#ff79c6" },
    terminal: { background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", ansi: T("#21222c #ff5555 #50fa7b #f1fa8c #bd93f9 #ff79c6 #8be9fd #f8f8f2 #6272a4 #ff6e6e #69ff94 #ffffa5 #d6acff #ff92df #a4ffff #ffffff") },
  },
  "tokyo-night": {
    name: "Tokyo Night", mode: "dark",
    colors: { background: "#1a1b26", sidebar: "#16161e", surface: "#16161e", text: "#c0caf5", muted: "#9aa5ce", border: "#292e42", accent: "#7aa2f7", green: "#9ece6a", red: "#f7768e", yellow: "#e0af68", orange: "#ff9e64", purple: "#bb9af7" },
    terminal: { background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5", ansi: T("#15161e #f7768e #9ece6a #e0af68 #7aa2f7 #bb9af7 #7dcfff #a9b1d6 #414868 #f7768e #9ece6a #e0af68 #7aa2f7 #bb9af7 #7dcfff #c0caf5") },
  },
  "catppuccin-mocha": {
    name: "Catppuccin Mocha", mode: "dark",
    colors: { background: "#1e1e2e", sidebar: "#181825", surface: "#181825", text: "#cdd6f4", muted: "#a6adc8", border: "#313244", accent: "#cba6f7", green: "#a6e3a1", red: "#f38ba8", yellow: "#f9e2af", orange: "#fab387", purple: "#f5c2e7" },
    terminal: { background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", ansi: T("#45475a #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #bac2de #585b70 #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #a6adc8") },
  },
  "catppuccin-latte": {
    name: "Catppuccin Latte", mode: "light",
    colors: { background: "#eff1f5", sidebar: "#e6e9ef", surface: "#e6e9ef", text: "#4c4f69", muted: "#6c6f85", border: "#ccd0da", accent: "#8839ef", green: "#40a02b", red: "#d20f39", yellow: "#df8e1d", orange: "#fe640b", purple: "#ea76cb" },
    terminal: { background: "#eff1f5", foreground: "#4c4f69", cursor: "#dc8a78", ansi: T("#5c5f77 #d20f39 #40a02b #df8e1d #1e66f5 #ea76cb #179299 #acb0be #6c6f85 #d20f39 #40a02b #df8e1d #1e66f5 #ea76cb #179299 #bcc0cc") },
  },
  nord: {
    name: "Nord", mode: "dark",
    colors: { background: "#2e3440", sidebar: "#272c36", surface: "#272c36", text: "#eceff4", muted: "#b6bfcf", border: "#3b4252", accent: "#88c0d0", green: "#a3be8c", red: "#bf616a", yellow: "#ebcb8b", orange: "#d08770", purple: "#b48ead" },
    terminal: { background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", ansi: T("#3b4252 #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #88c0d0 #e5e9f0 #4c566a #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #8fbcbb #eceff4") },
  },
  "gruvbox-dark": {
    name: "Gruvbox Dark", mode: "dark",
    colors: { background: "#282828", sidebar: "#1d2021", surface: "#1d2021", text: "#ebdbb2", muted: "#bdae93", border: "#3c3836", accent: "#fe8019", green: "#b8bb26", red: "#fb4934", yellow: "#fabd2f", orange: "#fe8019", purple: "#d3869b" },
    terminal: { background: "#282828", foreground: "#ebdbb2", cursor: "#ebdbb2", ansi: T("#282828 #cc241d #98971a #d79921 #458588 #b16286 #689d6a #a89984 #928374 #fb4934 #b8bb26 #fabd2f #83a598 #d3869b #8ec07c #ebdbb2") },
  },
  "gruvbox-light": {
    name: "Gruvbox Light", mode: "light",
    colors: { background: "#fbf1c7", sidebar: "#f2e5bc", surface: "#f2e5bc", text: "#3c3836", muted: "#665c54", border: "#d5c4a1", accent: "#af3a03", green: "#79740e", red: "#9d0006", yellow: "#b57614", orange: "#af3a03", purple: "#8f3f71" },
    terminal: { background: "#fbf1c7", foreground: "#3c3836", cursor: "#3c3836", ansi: T("#fbf1c7 #cc241d #98971a #d79921 #458588 #b16286 #689d6a #7c6f64 #928374 #9d0006 #79740e #b57614 #076678 #8f3f71 #427b58 #3c3836") },
  },
  "one-dark": {
    name: "One Dark", mode: "dark",
    colors: { background: "#282c34", sidebar: "#21252b", surface: "#21252b", text: "#d7dae0", muted: "#9da5b4", border: "#3a3f4b", accent: "#61afef", green: "#98c379", red: "#e06c75", yellow: "#e5c07b", orange: "#d19a66", purple: "#c678dd" },
    terminal: { background: "#282c34", foreground: "#abb2bf", cursor: "#528bff", ansi: T("#3f4451 #e06c75 #98c379 #e5c07b #61afef #c678dd #56b6c2 #d7dae0 #4f5666 #ff7b86 #b1e18b #efb074 #67cdff #e48bff #63d4e0 #e6e6e6") },
  },
  "rose-pine": {
    name: "Rosé Pine", mode: "dark",
    colors: { background: "#191724", sidebar: "#1f1d2e", surface: "#1f1d2e", text: "#e0def4", muted: "#908caa", border: "#26233a", accent: "#ebbcba", green: "#9ccfd8", red: "#eb6f92", yellow: "#f6c177", orange: "#ebbcba", purple: "#c4a7e7" },
    terminal: { background: "#191724", foreground: "#e0def4", cursor: "#524f67", ansi: T("#26233a #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4 #6e6a86 #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4") },
  },
  "solarized-dark": {
    name: "Solarized Dark", mode: "dark",
    colors: { background: "#002b36", sidebar: "#00232c", surface: "#073642", text: "#eee8d5", muted: "#93a1a1", border: "#0d4150", accent: "#268bd2", green: "#859900", red: "#dc322f", yellow: "#b58900", orange: "#cb4b16", purple: "#6c71c4" },
    terminal: { background: "#002b36", foreground: "#93a1a1", cursor: "#93a1a1", ansi: T("#073642 #dc322f #859900 #b58900 #268bd2 #d33682 #2aa198 #eee8d5 #586e75 #cb4b16 #859900 #b58900 #839496 #6c71c4 #93a1a1 #fdf6e3") },
  },
  "solarized-light": {
    name: "Solarized Light", mode: "light",
    colors: { background: "#fdf6e3", sidebar: "#eee8d5", surface: "#eee8d5", text: "#073642", muted: "#586e75", border: "#e0d9c3", accent: "#268bd2", green: "#859900", red: "#dc322f", yellow: "#b58900", orange: "#cb4b16", purple: "#6c71c4" },
    terminal: { background: "#fdf6e3", foreground: "#586e75", cursor: "#586e75", ansi: T("#073642 #dc322f #859900 #b58900 #268bd2 #d33682 #2aa198 #93a1a1 #586e75 #cb4b16 #586e75 #657b83 #268bd2 #6c71c4 #2aa198 #002b36") },
  },
  "github-dark": {
    name: "GitHub Dark", mode: "dark",
    colors: { background: "#0d1117", sidebar: "#010409", surface: "#151b23", text: "#e6edf3", muted: "#9198a1", border: "#262c36", accent: "#4493f8", green: "#3fb950", red: "#f85149", yellow: "#d29922", orange: "#db6d28", purple: "#ab7df8" },
    terminal: { background: "#0d1117", foreground: "#e6edf3", cursor: "#2f81f7", ansi: T("#484f58 #ff7b72 #3fb950 #d29922 #58a6ff #bc8cff #39c5cf #b1bac4 #6e7681 #ffa198 #56d364 #e3b341 #79c0ff #d2a8ff #56d4dd #ffffff") },
  },
  "github-light": {
    name: "GitHub Light", mode: "light",
    colors: { background: "#ffffff", sidebar: "#f6f8fa", surface: "#f6f8fa", text: "#1f2328", muted: "#59636e", border: "#d1d9e0", accent: "#0969da", green: "#1a7f37", red: "#d1242f", yellow: "#9a6700", orange: "#bc4c00", purple: "#8250df" },
    terminal: { background: "#ffffff", foreground: "#1f2328", cursor: "#0969da", ansi: T("#24292f #cf222e #116329 #4d2d00 #0969da #8250df #1b7c83 #6e7781 #57606a #a40e26 #1a7f37 #633c01 #218bff #a475f9 #3192aa #8c959f") },
  },
};

/** The order presets show in the picker: dark first, then light. */
export const PRESET_ORDER = Object.keys(PRESETS).sort((a, b) =>
  PRESETS[a].mode === PRESETS[b].mode ? PRESETS[a].name.localeCompare(PRESETS[b].name) : PRESETS[a].mode === "dark" ? -1 : 1);

/** Moonlight's own terminal colours, used when the theme gives none (and by Moonlight itself). */
export const MOONLIGHT_TERMINAL: Required<TerminalColors> = {
  background: "#0b0b1e", foreground: "#e6e6f5", cursor: "#ccccff",
  ansi: T("#2a2a52 #ff7a8e #5ee0a0 #ffd27a #9c9cff #e0a6ff #7adce8 #d8d8ee #8a8ab8 #ff9eae #8aeebc #ffe0a3 #c0c0ff #ecc6ff #a6eaf2 #f5f5ff"),
};

// ---------- validation (pasted and agent-written themes) ----------

const COLOR_KEYS: (keyof ThemeColors)[] = ["background", "text", "accent", "sidebar", "surface", "muted", "border", "green", "red", "yellow", "orange", "purple"];
/** Friendly names people (and agents) tend to use, mapped to ours. */
const ALIASES: Record<string, keyof ThemeColors> = {
  bg: "background", foreground: "text", fg: "text", primary: "accent", panel: "sidebar", card: "surface",
  secondary: "muted", subtext: "muted", line: "border", success: "green", error: "red", danger: "red", warning: "yellow",
};

/**
 * Read a theme from JSON text (or an already-parsed value). Accepts our own format, a bare colour
 * map, and common key names (bg, fg, primary...). Returns the theme or a plain-English reason.
 */
export function parseTheme(input: unknown): { theme: AppTheme } | { error: string } {
  let v: unknown = input;
  if (typeof input === "string") {
    const text = input.trim().replace(/^```(?:json)?\s*|\s*```$/g, "");
    try { v = JSON.parse(text); } catch { return { error: "That isn't a theme. Paste the JSON that Copy theme gives you." }; }
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return { error: "A theme is a JSON object with colors." };
  const o = v as Record<string, unknown>;
  const src = (o.colors && typeof o.colors === "object" ? o.colors : o) as Record<string, unknown>;
  const colors: Partial<ThemeColors> = {};
  for (const [k, val] of Object.entries(src)) {
    const key = (COLOR_KEYS as string[]).includes(k) ? (k as keyof ThemeColors) : ALIASES[k];
    if (!key) continue;
    if (!isHex(val)) return { error: `${k} must be a hex colour like #1e1e2e.` };
    colors[key] ??= (val as string).trim().toLowerCase();
  }
  for (const k of ["background", "text", "accent"] as const) if (!colors[k]) return { error: `The theme needs a ${k} colour.` };
  const mode: Mode = o.mode === "light" || o.mode === "dark" ? o.mode : luminance(colors.background!) > 0.4 ? "light" : "dark";
  const name = typeof o.name === "string" && o.name.trim() ? o.name.trim().slice(0, 40) : "Custom";
  let terminal: TerminalColors | undefined;
  if (o.terminal && typeof o.terminal === "object") {
    const t = o.terminal as Record<string, unknown>;
    terminal = {};
    for (const k of ["background", "foreground", "cursor"] as const) if (isHex(t[k])) terminal[k] = (t[k] as string).toLowerCase();
    if (Array.isArray(t.ansi) && t.ansi.length === 16 && t.ansi.every(isHex)) terminal.ansi = (t.ansi as string[]).map((c) => c.toLowerCase());
  }
  return { theme: { name, mode, colors: colors as ThemeColors, ...(terminal ? { terminal } : {}) } };
}

/** The theme as tidy JSON, for Copy theme. */
export const themeJson = (t: AppTheme) => JSON.stringify(t, null, 2);

// ---------- from base colours to every CSS variable ----------

/** Every CSS variable the theme sets. Moonlight sets none: its values live in App.css. */
export function themeVars(t: AppTheme): Record<string, string> {
  const c = t.colors;
  const dark = t.mode === "dark";
  const bg = c.background;
  const text = readable(c.text, bg, 7);
  const sidebar = c.sidebar ?? mix(bg, dark ? "#000000" : text, dark ? 0.18 : 0.04);
  const surface = c.surface ?? sidebar;
  const raised = dark ? mix(surface, text, 0.08) : mix(bg, "#ffffff", 0.7);
  const border = c.border ?? mix(bg, text, dark ? 0.14 : 0.13);
  const muted = readable(c.muted ?? mix(text, bg, 0.4), bg, 4.5);
  const accent = c.accent;
  const accentText = readable(accent, bg, 4.5);
  const green = c.green ?? (dark ? "#5ee0a0" : "#0b6a3b");
  const red = c.red ?? (dark ? "#ff7a8e" : "#b51e3b");
  const yellow = c.yellow ?? (dark ? "#ffd27a" : "#b07a00");
  const orange = c.orange ?? (dark ? "#ff9f5a" : "#9a4a00");
  const purple = c.purple ?? (dark ? "#d3a6ff" : "#7a3fa8");
  const tone = dark ? "#ffffff" : "#000000";
  const [h, p, s] = dark ? [0.08, 0.12, 0.11] : [0.06, 0.1, 0.09];
  return {
    "--bg": bg,
    "--sidebar": sidebar,
    "--surface": surface,
    "--raised": raised,
    "--field": dark ? alpha(text, 0.05) : raised,
    "--hover": alpha(text, h),
    "--press": alpha(text, p),
    "--select": alpha(text, s),
    "--line": border,
    "--line-strong": mix(border, text, 0.22),
    "--btn-hover": mix(raised, text, 0.05),
    "--btn-press": mix(raised, text, 0.09),
    "--material": alpha(surface, 0.94),
    "--material-solid": alpha(surface, 0.98),
    "--text": text,
    "--text2": readable(mix(text, bg, 0.2), bg, 7),
    "--muted": muted,
    "--faint": alpha(text, 0.3),
    "--accent": accent,
    "--accent-hover": mix(accent, tone, 0.12),
    "--accent-press": mix(accent, dark ? "#000000" : "#000000", 0.12),
    "--accent-text": accentText,
    "--accent-ink": inkOn(accent, bg),
    "--accent-soft": alpha(accent, dark ? 0.16 : 0.12),
    "--accent-ring": alpha(accent, 0.5),
    "--accent-chip-text": readable(mix(accent, tone, 0.12), bg, 4.5),
    "--lantern": yellow,
    "--lantern-text": readable(yellow, bg, 4.5),
    "--lantern-soft": alpha(yellow, 0.12),
    "--lantern-ring": alpha(yellow, 0.45),
    "--good": readable(green, bg, 3),
    "--good-soft": alpha(green, 0.13),
    "--good-fill": fillForWhite(green),
    "--good-text": readable(green, bg, 4.5),
    "--bad": readable(red, bg, 3),
    "--bad-soft": alpha(red, 0.12),
    "--bad-fill": fillForWhite(red),
    "--bad-text": readable(red, bg, 4.5),
    "--wait": readable(orange, bg, 3),
    "--wait-soft": alpha(orange, 0.13),
    "--purple": readable(purple, bg, 3),
    "--purple-soft": alpha(purple, 0.14),
    "--add-bg": alpha(green, dark ? 0.13 : 0.12),
    "--del-bg": alpha(red, dark ? 0.13 : 0.1),
    "--term-bg": terminalColors(t).background,
    "--term-fg": terminalColors(t).foreground,
  };
}

/** The terminal's colours for this theme. Moonlight (or no theme) keeps the classic dark terminal. */
export function terminalColors(t: AppTheme | null): Required<TerminalColors> {
  if (!t) return MOONLIGHT_TERMINAL;
  const x = t.terminal ?? {};
  const c = t.colors;
  const dark = t.mode === "dark";
  if (!dark && !x.ansi) return MOONLIGHT_TERMINAL; // a light theme with no terminal colours: ANSI colours made for dark won't read
  const background = x.background ?? (dark ? c.background : MOONLIGHT_TERMINAL.background);
  const foreground = x.foreground ?? readable(c.text, background, 7);
  const ansi = x.ansi ?? [
    mix(background, foreground, 0.2), c.red ?? MOONLIGHT_TERMINAL.ansi[1], c.green ?? MOONLIGHT_TERMINAL.ansi[2], c.yellow ?? MOONLIGHT_TERMINAL.ansi[3],
    c.accent, c.purple ?? MOONLIGHT_TERMINAL.ansi[5], MOONLIGHT_TERMINAL.ansi[6], mix(foreground, background, 0.1),
    mix(background, foreground, 0.5), c.red ?? MOONLIGHT_TERMINAL.ansi[9], c.green ?? MOONLIGHT_TERMINAL.ansi[10], c.yellow ?? MOONLIGHT_TERMINAL.ansi[11],
    mix(c.accent, "#ffffff", 0.2), c.purple ?? MOONLIGHT_TERMINAL.ansi[13], MOONLIGHT_TERMINAL.ansi[14], foreground,
  ];
  return { background, foreground, cursor: x.cursor ?? c.accent, ansi };
}

/** The xterm.js theme object. */
export function xtermTheme(t: AppTheme | null) {
  const x = terminalColors(t);
  const names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"];
  return {
    background: x.background, foreground: x.foreground, cursor: x.cursor, cursorAccent: x.background,
    selectionBackground: alpha(x.cursor, 0.25),
    ...Object.fromEntries(names.map((n, i) => [n, x.ansi[i]])),
  };
}

/** The theme a palette id points at: a preset, your custom theme, or null for Moonlight. */
export function resolvePalette(id: string, custom: AppTheme | null): AppTheme | null {
  if (!id) return null;
  if (id === "custom") return custom;
  return PRESETS[id] ?? null;
}
