/** App preferences, kept in localStorage. Read with `getSettings()`, change with `saveSettings()`. */
export type Settings = {
  /** When to send a system notification after an agent finishes. */
  notify: "away" | "always" | "never";
  /** A soft chime when an agent finishes. Off by default. */
  sound: boolean;
  /** Where ⌘O opens the current workspace. */
  editor: string;
  /** Which key sends a message; the other one adds a new line. */
  sendKey: "enter" | "cmd-enter";
  /** Archive a workspace by itself once its PR merges. */
  archiveOnMerge: boolean;
  /** Sidebar grouping. */
  groupBy: "repo" | "status";
  /** When an agent needs you (plan to approve, question, sign-in, limit, error): sound, notification, Dock bounce. */
  needsSound: boolean;
  needsNotify: boolean;
  needsBounce: boolean;
  /** The old saved prompts. Moved into the prompt library (Settings, Prompts) the first time it loads. */
  snippets: { name: string; text: string }[];
  /** Repo ids to show in the sidebar; empty shows every repo. */
  repoFilter: string[];
  /** Repo ids whose sidebar section is folded. */
  collapsed: string[];
  /** Light, dark, or follow the system. */
  theme: "system" | "light" | "dark";
  /** Font for diffs and code in chat ("" is SF Mono) and its size in px. */
  codeFont: string;
  codeSize: number;
  /** Font for the terminal ("" is SF Mono) and its size in px. */
  termFont: string;
  termSize: number;
  /** The "Made with nekotrail" line at the end of session exports. */
  exportFooter: boolean;
};

const KEY = "nekotrail.settings";
const DEFAULTS: Settings = { notify: "away", sound: false, editor: "cursor", sendKey: "enter", archiveOnMerge: true, groupBy: "repo", needsSound: true, needsNotify: true, needsBounce: true,
  snippets: [
    { name: "tests", text: "Write tests for what you just changed. Cover the edge cases, run them, and fix anything that fails." },
    { name: "tidy", text: "Before I review: remove debug code and dead code, make names clear, and keep the diff as small as it can be." },
  ], repoFilter: [], collapsed: [],
  theme: "system", codeFont: "", codeSize: 12, termFont: "", termSize: 13, exportFooter: true };

export function getSettings(): Settings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) || "{}") };
  } catch {
    return DEFAULTS;
  }
}

export function saveSettings(s: Settings) {
  localStorage.setItem(KEY, JSON.stringify(s));
  window.dispatchEvent(new Event("nekotrail-settings"));
}

let ctx: AudioContext | null = null;

/** Two soft notes, rising when the turn went well and falling when it needs you. No audio files. */
export function chime(ok = true) {
  try {
    ctx ??= new AudioContext();
    const t = ctx.currentTime;
    const notes = ok ? [659.25, 987.77] : [523.25, 392.0]; // E5→B5, or C5→G4
    notes.forEach((f, i) => {
      const o = ctx!.createOscillator();
      const g = ctx!.createGain();
      o.type = "sine";
      o.frequency.value = f;
      const at = t + i * 0.11;
      g.gain.setValueAtTime(0, at);
      g.gain.linearRampToValueAtTime(0.12, at + 0.015);
      g.gain.exponentialRampToValueAtTime(0.0001, at + 0.5);
      o.connect(g).connect(ctx!.destination);
      o.start(at);
      o.stop(at + 0.55);
    });
  } catch {
    /* audio unavailable */
  }
}

/** "Ting ting": two bright, quick pings, clearly different from the done chime. Means "come back, you're needed". */
export function ting() {
  try {
    ctx ??= new AudioContext();
    const t = ctx.currentTime;
    [0, 0.16].forEach((offset) => {
      const o = ctx!.createOscillator();
      const g = ctx!.createGain();
      o.type = "triangle";
      o.frequency.value = 1318.5; // E6
      const at = t + offset;
      g.gain.setValueAtTime(0, at);
      g.gain.linearRampToValueAtTime(0.16, at + 0.008);
      g.gain.exponentialRampToValueAtTime(0.0001, at + 0.35);
      o.connect(g).connect(ctx!.destination);
      o.start(at);
      o.stop(at + 0.4);
    });
  } catch {
    /* audio unavailable */
  }
}
