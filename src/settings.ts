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
  /** Repo ids to show in the sidebar; empty shows every repo. */
  repoFilter: string[];
  /** Repo ids whose sidebar section is folded. */
  collapsed: string[];
};

const KEY = "nekotrail.settings";
const DEFAULTS: Settings = { notify: "away", sound: false, editor: "cursor", sendKey: "enter", archiveOnMerge: true, groupBy: "repo", repoFilter: [], collapsed: [] };

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
