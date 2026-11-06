/**
 * The prompt library, frontend side. The backend (src-tauri/src/prompts.rs) reads the prompts,
 * splits their text into plain runs and blanks, and decides which one `/name` means. This file
 * only matches names as you type and fills the blanks in the chat box, one tab-stop at a time.
 */
import { api } from "./api";
import { getSettings } from "./settings";

export type Seg = { t: "text"; text: string } | { t: "field"; name: string; default: string | null; builtin: boolean };
export type PromptScope = "personal" | "repo" | "claude";
export type PromptEntry = {
  name: string; description: string; text: string; segs: Seg[];
  scope: PromptScope; source: string; read_only: boolean; hidden_by: PromptScope | null;
};
export type PersonalPrompt = { name: string; description: string; text: string };

export const SCOPE_LABEL: Record<PromptScope, string> = { personal: "Yours", repo: "Repo", claude: "Claude Code" };

/**
 * Loads the library. The first time, the old snippets from Settings become personal prompts, so
 * nobody loses theirs. Prompts hidden by another of the same name are left out unless `all`.
 */
export async function loadPrompts(where: { id?: string; repo?: string }, all = false): Promise<PromptEntry[]> {
  let r = await api.promptsList(where.id, where.repo);
  if (!r.saved) {
    const old = getSettings().snippets.filter((s) => s.name.trim() && s.text.trim());
    try {
      await api.promptsSave(old.map((s) => ({ name: s.name, description: "", text: s.text })));
    } catch {
      await api.promptsSave([]); // an old name clashes with a command now: start clean rather than ask every time
    }
    r = await api.promptsList(where.id, where.repo);
  }
  return all ? r.entries : r.entries.filter((e) => !e.hidden_by);
}

/** Personal prompts only, in the shape the backend saves. */
export const personalOf = (list: PromptEntry[]): PersonalPrompt[] =>
  list.filter((e) => e.scope === "personal").map(({ name, description, text }) => ({ name, description, text }));

/**
 * How well `q` matches a name: 0 exact, 1 prefix, 2 word prefix, 3 substring, 4 letters in
 * order (the description counts at this level too), -1 no match. Lower is better.
 */
export function fuzzy(name: string, q: string, extra = ""): number {
  const n = name.toLowerCase().replace(/^\//, "");
  const s = q.toLowerCase().replace(/^\//, "");
  if (!s) return 1;
  if (n === s) return 0;
  if (n.startsWith(s)) return 1;
  if (n.split(/[-_:\s]+/).some((w) => w.startsWith(s))) return 2;
  if (n.includes(s)) return 3;
  const hay = `${n} ${extra.toLowerCase()}`;
  let i = 0;
  for (const ch of hay) if (ch === s[i]) i++;
  return i === s.length ? 4 : -1;
}

// ---- Filling the blanks -----------------------------------------------------------------------

/** A prompt being filled in: its pieces, a value per blank, and which tab-stop has focus. */
export type Fill = {
  segs: Seg[];
  values: Record<string, string>;
  /** Blanks you typed into (or that started with a value). The rest still show their name. */
  filled: string[];
  /** Built-ins the app filled in at the start. They are not tab-stops. */
  auto: string[];
  /** Index into stops(). */
  at: number;
};

/** Prompts New workspace can offer: not Claude's own commands, and not ones built on a Changes selection, which a new workspace can't have. */
export const forNewWorkspace = (list: PromptEntry[]): PromptEntry[] =>
  list.filter((p) => p.scope !== "claude" && !p.segs.some((s) => s.t === "field" && s.builtin && s.name === "selection"));

/** Built-in values the app knows right now. Missing or empty ones become tab-stops like any blank. */
export type Builtins = Partial<Record<"branch" | "base" | "files_changed" | "selection", string>>;

/** The tab-stops: the first place each blank appears, except built-ins that already have a value. */
export function stops(f: Pick<Fill, "segs" | "auto">): number[] {
  const seen = new Set<string>();
  const out: number[] = [];
  f.segs.forEach((s, i) => {
    if (s.t !== "field" || seen.has(s.name)) return;
    seen.add(s.name);
    if (f.auto.includes(s.name)) return;
    out.push(i);
  });
  return out;
}

/** Starts filling a prompt. No stops left (no blanks, or only built-ins the app knows): plain text. */
export function startFill(segs: Seg[], known: Builtins = {}): { fill: Fill | null; text: string } {
  const values: Record<string, string> = {};
  const filled: string[] = [];
  const auto: string[] = [];
  for (const s of segs) {
    if (s.t !== "field" || s.name in values) continue;
    const b = s.builtin ? known[s.name as keyof Builtins] : undefined;
    if (b) { values[s.name] = b; filled.push(s.name); auto.push(s.name); }
    else if (s.default !== null && s.default !== "") { values[s.name] = s.default; filled.push(s.name); }
    else values[s.name] = s.name; // the name stands in until you type over it
  }
  const fill: Fill = { segs, values, filled, auto, at: 0 };
  const text = render(fill).text;
  return { fill: stops(fill).length ? fill : null, text };
}

/** The text and where each piece sits in it. */
export function render(f: Pick<Fill, "segs" | "values">): { text: string; ranges: [number, number][] } {
  let text = "";
  const ranges: [number, number][] = [];
  for (const s of f.segs) {
    const piece = s.t === "text" ? s.text : f.values[s.name] ?? "";
    ranges.push([text.length, text.length + piece.length]);
    text += piece;
  }
  return { text, ranges };
}

/** Where the active tab-stop sits in the text. */
export function activeRange(f: Fill): [number, number] | null {
  const st = stops(f);
  const i = st[f.at];
  return i === undefined ? null : render(f).ranges[i];
}

/**
 * The chat box changed from `before` to `after` while filling. If the change stayed inside the
 * active blank, that blank takes the new value (and every other place it appears follows).
 * Anything else, such as typing outside the blank, ends filling: returns null, keep the text as typed.
 */
export function applyEdit(f: Fill, before: string, after: string): Fill | null {
  const r = activeRange(f);
  if (!r) return null;
  const [start, end] = r;
  const head = before.slice(0, start);
  const tail = before.slice(end);
  if (!after.startsWith(head) || !after.endsWith(tail) || after.length < head.length + tail.length) return null;
  const value = after.slice(start, after.length - tail.length);
  const st = stops(f);
  const seg = f.segs[st[f.at]];
  if (seg.t !== "field") return null;
  return { ...f, values: { ...f.values, [seg.name]: value }, filled: f.filled.includes(seg.name) ? f.filled : [...f.filled, seg.name] };
}

/** Moves to the next (or previous) tab-stop. Null when you Tab past the last one: filling is done. */
export function step(f: Fill, by: 1 | -1): Fill | null {
  const n = stops(f).length;
  const at = f.at + by;
  if (at >= n) return null;
  return { ...f, at: Math.max(0, at) };
}

/** Blanks never filled in, so the text would go out with a placeholder name in it. */
export function unfilled(f: Fill): string[] {
  return stops(f).map((i) => f.segs[i]).flatMap((s) => (s.t === "field" && !f.filled.includes(s.name) ? [s.name] : []));
}

/** Text selected in the Changes tab, kept until another selection there replaces or clears it. */
let changesSelection = "";
export const setChangesSelection = (s: string) => { changesSelection = s; };
export const getChangesSelection = () => changesSelection;
