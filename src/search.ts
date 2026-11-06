import type { SearchGroup, SearchHit } from "./api";

/** A snippet split into plain and matched runs. The backend wraps matches in \u0002 and \u0003. */
export function snippetParts(s: string): { text: string; hit: boolean }[] {
  const out: { text: string; hit: boolean }[] = [];
  let hit = false;
  let buf = "";
  const flush = () => { if (buf) out.push({ text: buf, hit }); buf = ""; };
  for (const ch of s) {
    if (ch === "\u0002") { flush(); hit = true; }
    else if (ch === "\u0003") { flush(); hit = false; }
    else buf += ch === "\n" || ch === "\t" ? " " : ch;
  }
  flush();
  return out;
}

export type DateRange = "any" | "today" | "week" | "month";
export const DATE_LABEL: Record<DateRange, string> = { any: "Any time", today: "Today", week: "Last 7 days", month: "Last 30 days" };

/** The RFC 3339 instant a date filter starts at, or null for any time. "Today" is your local midnight. */
export function sinceFor(range: DateRange, now = new Date()): string | null {
  if (range === "any") return null;
  if (range === "today") return new Date(now.getFullYear(), now.getMonth(), now.getDate()).toISOString();
  const days = range === "week" ? 7 : 30;
  return new Date(now.getTime() - days * 86_400_000).toISOString();
}

/** One keyboard stop in the results: a hit, or "more in this workspace". */
export type Item = { type: "hit"; group: SearchGroup; hit: SearchHit } | { type: "more"; group: SearchGroup };

export function flatten(groups: SearchGroup[]): Item[] {
  return groups.flatMap((g) => [
    ...g.hits.map((hit) => ({ type: "hit" as const, group: g, hit })),
    ...(g.more > 0 ? [{ type: "more" as const, group: g }] : []),
  ]);
}

/** "now", "5 min ago", "3 h ago", "yesterday", "4 days ago", then the date. */
export function ago(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, (now - t) / 1000);
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86_400);
  if (d === 1) return "yesterday";
  if (d < 7) return `${d} days ago`;
  const date = new Date(t);
  return date.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(date.getFullYear() !== new Date(now).getFullYear() ? { year: "numeric" } : {}) });
}

/** Every place `q` occurs in `text`, case-insensitive, as [start, end) offsets. For ⌘F highlights. */
export function occurrences(text: string, q: string): [number, number][] {
  const needle = q.toLowerCase();
  if (!needle) return [];
  const hay = text.toLowerCase();
  const out: [number, number][] = [];
  // Lower-casing can change length (rare, e.g. "İ"); then offsets would drift, so skip those texts.
  if (hay.length !== text.length) return out;
  for (let i = hay.indexOf(needle); i >= 0; i = hay.indexOf(needle, i + needle.length)) out.push([i, i + needle.length]);
  return out;
}
