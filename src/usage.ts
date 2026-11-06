import { agentName } from "./agents";
import { clockTime } from "./queue";

/**
 * The Usage view's types and words. The numbers come from the backend (src-tauri/src/usage.rs,
 * budget.rs): every finished turn with what its agent reported. Nothing here estimates: a cost or
 * a token count an agent didn't report stays unknown.
 */

/** One day, repo, agent and model. Twin of `usage::Line`. */
export type UsageLine = {
  day: string;
  repo_id: string;
  agent: string;
  model: string;
  turns: number;
  /** Turns whose agent reported a cost; `cost` sums only those. */
  priced_turns: number;
  cost: number;
  /** Turns whose agent reported tokens; the token sums cover only those. */
  token_turns: number;
  input: number;
  cached: number;
  output: number;
  duration_ms: number;
};

export type Cap = { usd: number | null; turns: number | null; tokens: number | null };
export type Caps = { day: Cap; task: Cap };
export type Runaway = { on: boolean; same_failure: number; back_and_forth: number; no_change: number };
export type Budgets = { all: Caps; repos: Record<string, Caps>; runaway: Runaway };
export type Spent = { usd: number; turns: number; tokens: number };
export type AgentLimit = { until: number; reported: boolean; text: string };

export type UsageReport = {
  lines: UsageLine[];
  /** The first day counted (YYYY-MM-DD, local). */
  from: string;
  budgets: Budgets;
  /** Spent since local midnight, overall and per repo. */
  today: Spent;
  today_repos: Record<string, Spent>;
  /** Usage limits the agents reported, as the queue last heard them. */
  limits: Record<string, AgentLimit>;
  now: number;
};

export const NO_CAP: Cap = { usd: null, turns: null, tokens: null };
export const DEFAULT_RUNAWAY: Runaway = { on: true, same_failure: 4, back_and_forth: 2, no_change: 3 };

export type GroupBy = "day" | "repo" | "agent" | "model";

export type Rollup = Omit<UsageLine, "day" | "repo_id" | "agent" | "model"> & { key: string };

/** Sum the lines by one of their names, biggest spend first (days: newest first). */
export function rollup(lines: UsageLine[], by: GroupBy): Rollup[] {
  const m = new Map<string, Rollup>();
  for (const l of lines) {
    const key = by === "day" ? l.day : by === "repo" ? l.repo_id : by === "agent" ? l.agent : l.model;
    const r = m.get(key) ?? { key, turns: 0, priced_turns: 0, cost: 0, token_turns: 0, input: 0, cached: 0, output: 0, duration_ms: 0 };
    r.turns += l.turns; r.priced_turns += l.priced_turns; r.cost += l.cost; r.token_turns += l.token_turns;
    r.input += l.input; r.cached += l.cached; r.output += l.output; r.duration_ms += l.duration_ms;
    m.set(key, r);
  }
  const rows = [...m.values()];
  return by === "day"
    ? rows.sort((a, b) => b.key.localeCompare(a.key))
    : rows.sort((a, b) => b.cost - a.cost || b.turns - a.turns || a.key.localeCompare(b.key));
}

/** Every line added up. */
export function total(lines: UsageLine[]): Rollup {
  return rollup(lines.map((l) => ({ ...l, day: "" })), "day")[0] ?? { key: "", turns: 0, priced_turns: 0, cost: 0, token_turns: 0, input: 0, cached: 0, output: 0, duration_ms: 0 };
}

export const money = (v: number) => `$${v.toFixed(2)}`;

/** 950, 12.4k, 3.1M. */
export function tokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 1 : 0)}M`;
}

/** 45 s, 12 min, 3 h 5 min. */
export function duration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}

/** The cost cell: "$4.20", "Not reported" when no turn had a cost, or a note on the part that did. */
export function costText(r: Pick<Rollup, "cost" | "priced_turns" | "turns">): { text: string; note: string } {
  if (r.priced_turns === 0) return { text: "Not reported", note: "This agent doesn't report a cost. Its turns and tokens still count." };
  const left = r.turns - r.priced_turns;
  return { text: money(r.cost), note: left > 0 ? `${left} ${left === 1 ? "turn" : "turns"} without a reported cost aren't in this number.` : "" };
}

/** The tokens cell: unknown when no turn reported tokens. */
export function tokenText(r: Pick<Rollup, "token_turns" | "input" | "output">): string {
  return r.token_turns === 0 ? "Not reported" : tokens(r.input + r.output);
}

/** How a group-by key reads: a repo's name, an agent's name, "Default model", a day. */
export function keyName(by: GroupBy, key: string, repoName: (id: string) => string, today: string, yesterday: string): string {
  if (by === "repo") return repoName(key);
  if (by === "agent") return agentName(key);
  if (by === "model") return key || "Default model";
  if (key === today) return "Today";
  if (key === yesterday) return "Yesterday";
  const d = new Date(`${key}T12:00:00`);
  return Number.isNaN(d.getTime()) ? key : d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

export type Meter = { label: string; used: number; cap: number; text: string; reached: boolean; share: number };

/** Each limit a cap sets, against what was spent: "$12.40 of $20.00". */
export function meters(cap: Cap, spent: Spent): Meter[] {
  const out: Meter[] = [];
  const add = (label: string, used: number, c: number | null, fmt: (n: number) => string) => {
    if (c === null || c <= 0) return;
    out.push({ label, used, cap: c, text: `${fmt(used)} of ${fmt(c)}`, reached: used >= c, share: Math.min(1, used / c) });
  };
  add("API-equivalent", spent.usd, cap.usd, money);
  add("Turns", spent.turns, cap.turns, (n) => n.toLocaleString("en-US"));
  add("Tokens", spent.tokens, cap.tokens, tokens);
  return out;
}

export const hasCap = (c: Cap) => c.usd !== null || c.turns !== null || c.tokens !== null;

/**
 * What you typed in a cap field: null for empty (no cap), a number, or an error to show. Dollars
 * take cents; turns and tokens are whole numbers. "2M" and "500k" work for tokens.
 */
export function parseCap(text: string, kind: "usd" | "turns" | "tokens"): { value: number | null } | { error: string } {
  const t = text.trim().replace(/[$,\s]/g, "");
  if (!t) return { value: null };
  const m = /^(\d+(?:\.\d+)?)([kKmM]?)$/.exec(t);
  if (!m || (kind !== "tokens" && m[2])) return { error: kind === "usd" ? "Type an amount like 20 or 7.50." : kind === "turns" ? "Type a whole number of turns." : "Type a number like 2000000 or 2M." };
  let n = Number(m[1]) * (m[2].toLowerCase() === "k" ? 1e3 : m[2].toLowerCase() === "m" ? 1e6 : 1);
  if (kind !== "usd") {
    if (!Number.isInteger(n)) return { error: kind === "turns" ? "Type a whole number of turns." : "Type a whole number of tokens." };
  } else n = Math.round(n * 100) / 100;
  if (n <= 0) return { error: "Leave it empty for no cap." };
  return { value: n };
}

/** A cap's value as it shows in its field. */
export function capField(v: number | null, kind: "usd" | "turns" | "tokens"): string {
  if (v === null) return "";
  return kind === "usd" ? (Number.isInteger(v) ? String(v) : v.toFixed(2)) : String(v);
}

/** An agent's usage limit, in one line. */
export function limitText(agent: string, l: AgentLimit | undefined, now: number, locale?: string): string {
  const name = agentName(agent);
  if (!l) return "No usage limit reported";
  if (l.until > now) {
    return l.reported
      ? `Out of usage until ${clockTime(l.until, now, locale)} (${name} said so)`
      : `Out of usage. ${name} didn't say until when, so nekotrail tries again at ${clockTime(l.until, now, locale)}`;
  }
  return `Its limit was due to reset at ${clockTime(l.until, now, locale)}. The next turn shows whether it has`;
}

/** YYYY-MM-DD for a Date, in local time. */
export function dayOf(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}
