/**
 * Pure helpers for the update popup: release notes to plain-text highlights, the per-version
 * snooze, and the sentence that says what a restart would stop. No Tauri imports, so they are
 * easy to test (tests/updateNotes.test.ts).
 */

export const RELEASES_URL = "https://github.com/Harshil-Jani/nekotrail/releases";
export const releaseUrl = (version?: string) =>
  version ? `${RELEASES_URL}/tag/v${version.replace(/^v/, "")}` : `${RELEASES_URL}/latest`;

const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'", nbsp: " " };

/** Markdown and any HTML reduced to plain words. The result is rendered as text, never as HTML. */
export function plainText(s: string): string {
  return s
    .replace(/<[^>]*>/g, "")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/(\*\*|__)(.+?)\1/g, "$2")
    .replace(/(^|[\s(])\*(\S(?:.*?\S)?)\*(?=[\s).,!?:;]|$)/g, "$1$2")
    .replace(/~~(.+?)~~/g, "$1")
    .replace(/&(#39|amp|lt|gt|quot|apos|nbsp);/g, (_, k: string) => ENTITIES[k])
    .replace(/\s+/g, " ")
    .trim();
}

/** Shorten at a word boundary. */
export function clip(s: string, max = 120): string {
  if (s.length <= max) return s;
  const cut = s.slice(0, max);
  const at = cut.lastIndexOf(" ");
  return `${(at > max * 0.6 ? cut.slice(0, at) : cut).replace(/[\s,.;:]+$/, "")}…`;
}

/**
 * The top-level bullets of a release's notes, as plain text (at most `max`), plus how many
 * more there were. Stops at an "Install" heading (the GitHub release adds one). Notes with no
 * bullets give their first paragraph.
 */
export function highlights(notes: string | undefined, max = 4): { items: string[]; more: number } {
  const lines = (notes ?? "").split(/\r?\n/);
  const bullets: string[] = [];
  for (const line of lines) {
    if (/^#{1,6}\s*install\b/i.test(line.trim())) break;
    const m = /^[-*+]\s+(.+)$/.exec(line);
    if (m) {
      const t = plainText(m[1]);
      if (t) bullets.push(t);
    }
  }
  if (bullets.length === 0) {
    const para = plainText(lines.filter((l) => !/^#/.test(l.trim())).join("\n").split(/\n\s*\n/)[0] ?? "");
    return { items: para ? [clip(para, 200)] : [], more: 0 };
  }
  return { items: bullets.slice(0, max).map((b) => clip(b)), more: Math.max(0, bullets.length - max) };
}

const SNOOZE_KEY = "nekotrail.update.snooze";
export const SNOOZE_MS = 24 * 60 * 60_000;

/** "Not now" snoozes that one version for a day. A newer version prompts straight away. */
export function snooze(version: string, now = Date.now(), store: Pick<Storage, "setItem"> = localStorage) {
  store.setItem(SNOOZE_KEY, JSON.stringify({ version, until: now + SNOOZE_MS }));
}

export function isSnoozed(version: string, now = Date.now(), store: Pick<Storage, "getItem"> = localStorage): boolean {
  try {
    const s = JSON.parse(store.getItem(SNOOZE_KEY) ?? "null") as { version?: string; until?: number } | null;
    return !!s && s.version === version && typeof s.until === "number" && now < s.until;
  } catch {
    return false;
  }
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "2 agents and 1 terminal", or "" when nothing is running. */
export function busyText(agents: number, terminals: number): string {
  const parts = [];
  if (agents) parts.push(plural(agents, "agent", "agents"));
  if (terminals) parts.push(plural(terminals, "terminal", "terminals"));
  return parts.join(" and ");
}

/** "is" or "are" for busyText's subject. */
export const busyVerb = (agents: number, terminals: number) => (agents + terminals === 1 ? "is" : "are");
