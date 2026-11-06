import type { TerminalPreview, TerminalSession } from "./api";

/** One session's key in a selection. */
export const keyOf = (s: { agent: string; id: string }) => `${s.agent}:${s.id}`;

export type Totals = { sessions: number; messages: number; bytes: number; first: string; last: string };

/** Counts, size and date range of some sessions. */
export function totals(sessions: TerminalSession[]): Totals {
  const t: Totals = { sessions: 0, messages: 0, bytes: 0, first: "", last: "" };
  for (const s of sessions) {
    t.sessions++;
    t.messages += s.messages;
    t.bytes += s.bytes;
    if (s.first && (!t.first || s.first < t.first)) t.first = s.first;
    if (s.last && s.last > t.last) t.last = s.last;
  }
  return t;
}

/** Sessions not in nekotrail yet. */
export const fresh = (sessions: TerminalSession[]) => sessions.filter((s) => !s.imported);

/** Every session in a preview that can still come in: what starts ticked. */
export function newKeys(p: TerminalPreview): Set<string> {
  const keys = new Set<string>();
  for (const r of p.repos) for (const f of r.folders) for (const s of fresh(f.sessions)) keys.add(keyOf(s));
  return keys;
}

/** The picked sessions as the backend takes them. */
export function picks(p: TerminalPreview, selected: Set<string>): { agent: string; id: string }[] {
  const out: { agent: string; id: string }[] = [];
  for (const r of p.repos) for (const f of r.folders) for (const s of f.sessions) if (!s.imported && selected.has(keyOf(s))) out.push({ agent: s.agent, id: s.id });
  return out;
}

export function bytes(n: number): string {
  if (n < 1000) return `${n} B`;
  if (n < 1e6) return `${Math.round(n / 1e3)} KB`;
  if (n < 1e9) return `${(n / 1e6).toFixed(n < 1e7 ? 1 : 0)} MB`;
  return `${(n / 1e9).toFixed(1)} GB`;
}

const day = (iso: string) => new Date(iso).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });

/** "3 Nov 2026" or "12 Oct 2026 to 3 Nov 2026". */
export function range(first: string, last: string): string {
  if (!first && !last) return "";
  const a = day(first || last);
  const b = day(last || first);
  return a === b ? a : `${a} to ${b}`;
}

export const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** The command that picks a session up again in a terminal. */
export function resumeCommand(agent: string, id: string, folder: string): string {
  const cd = `cd ${JSON.stringify(folder)} && `;
  if (agent === "codex") return `${cd}codex resume ${id}`;
  if (agent === "gemini") return `${cd}gemini --resume ${id}`;
  return `${cd}claude --resume ${id}`;
}
