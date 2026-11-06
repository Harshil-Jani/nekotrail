import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { AlertCircle, Download, Gauge, Info, Plus, Trash2 } from "lucide-react";
import { api, errText, type Repo } from "../api";
import { agentName } from "../agents";
import {
  capField, costText, DEFAULT_RUNAWAY, dayOf, duration, hasCap, keyName, limitText, meters, money, NO_CAP, parseCap, rollup, tokenText, tokens, total,
  type Budgets, type Cap, type Caps, type GroupBy, type Runaway, type Spent, type UsageReport,
} from "../usage";

const RANGES = [{ days: 1, label: "Today" }, { days: 7, label: "7 days" }, { days: 30, label: "30 days" }, { days: 90, label: "90 days" }];
const GROUPS: { by: GroupBy; label: string }[] = [{ by: "day", label: "Day" }, { by: "repo", label: "Repo" }, { by: "agent", label: "Agent" }, { by: "model", label: "Model" }];
const KINDS = ["usd", "turns", "tokens"] as const;
type Kind = (typeof KINDS)[number];
type CapText = Record<Kind, string>;
type CapsText = { day: CapText; task: CapText };
type Draft = { all: CapsText; repos: Record<string, CapsText>; runaway: Runaway };

const toText = (c: Cap): CapText => ({ usd: capField(c.usd, "usd"), turns: capField(c.turns, "turns"), tokens: capField(c.tokens, "tokens") });
const capsText = (c: Caps): CapsText => ({ day: toText(c.day), task: toText(c.task) });
const draftOf = (b: Budgets): Draft => ({
  all: capsText(b.all),
  repos: Object.fromEntries(Object.entries(b.repos).map(([id, c]) => [id, capsText(c)])),
  runaway: { ...DEFAULT_RUNAWAY, ...b.runaway },
});
const emptyText = (): CapsText => ({ day: toText(NO_CAP), task: toText(NO_CAP) });

/** Read a draft back into caps, or the first thing that's wrong. */
function parseDraft(d: Draft, repoName: (id: string) => string): { budgets: Budgets } | { error: string } {
  const cap = (t: CapText, where: string): Cap | string => {
    const out: Cap = { ...NO_CAP };
    for (const k of KINDS) {
      const r = parseCap(t[k], k);
      if ("error" in r) return `${where}: ${r.error}`;
      out[k] = r.value;
    }
    return out;
  };
  const caps = (c: CapsText, where: string): Caps | string => {
    const day = cap(c.day, `${where}, daily cap`);
    if (typeof day === "string") return day;
    const task = cap(c.task, `${where}, cap per task`);
    if (typeof task === "string") return task;
    return { day, task };
  };
  const all = caps(d.all, "Every repo");
  if (typeof all === "string") return { error: all };
  const repos: Record<string, Caps> = {};
  for (const [id, c] of Object.entries(d.repos)) {
    const r = caps(c, repoName(id));
    if (typeof r === "string") return { error: r };
    repos[id] = r;
  }
  return { budgets: { all, repos, runaway: d.runaway } };
}

/**
 * Usage: what your agents did and what it would cost on the API, per day, repo, agent and model,
 * with the spend caps that hold the queue and autopilot back, and each agent's usage limit. All
 * of it is read from this Mac; nothing is sent anywhere.
 */
export function Usage({ repos, onSay }: { repos: Repo[]; onSay: (text: string, bad?: boolean) => void }) {
  const [days, setDays] = useState(7);
  const [by, setBy] = useState<GroupBy>("day");
  const [report, setReport] = useState<UsageReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  /** The saved caps as the form shows them, to tell when you changed something. */
  const saved = useRef("");
  const [saving, setSaving] = useState(false);
  const [capError, setCapError] = useState<string | null>(null);
  const repoName = useCallback((id: string) => repos.find((r) => r.id === id)?.name ?? "Removed repo", [repos]);

  const load = useCallback(() => {
    api.usageReport(days).then((r) => {
      setReport(r);
      setError(null);
      // The caps form follows what's saved, unless you are editing it.
      const prev = saved.current;
      saved.current = JSON.stringify(draftOf(r.budgets));
      setDraft((d) => (!d || JSON.stringify(d) === prev ? draftOf(r.budgets) : d));
    }).catch((e) => setError(errText(e)));
  }, [days]);
  useEffect(() => {
    load();
    const uns = ["running", "task-queue"].map((ev) => listen(ev, load));
    const t = setInterval(load, 60_000);
    return () => { clearInterval(t); uns.forEach((u) => u.then((f) => f())); };
  }, [load]);

  const dirty = !!draft && JSON.stringify(draft) !== saved.current;
  const saveCaps = async () => {
    if (!draft) return;
    const r = parseDraft(draft, repoName);
    if ("error" in r) {
      setCapError(r.error);
      // Take you to the field that's wrong; it may be far above the Save button.
      document.querySelector<HTMLInputElement>('.usage [aria-invalid="true"]')?.focus();
      return;
    }
    setCapError(null);
    setSaving(true);
    try {
      const b = await api.budgetsSet(r.budgets);
      saved.current = JSON.stringify(draftOf(b));
      setDraft(draftOf(b));
      onSay("Caps saved.");
      load();
    } catch (e) {
      setCapError(errText(e));
    } finally {
      setSaving(false);
    }
  };

  const exportCsv = async () => {
    try {
      const path = await saveDialog({ defaultPath: `nekotrail-usage-${dayOf(new Date())}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (!path) return;
      const n = await api.usageExport(path, days);
      onSay(`Saved ${path.replace(/^.*\//, "")} (${n} ${n === 1 ? "row" : "rows"}).`);
    } catch (e) {
      onSay(errText(e), true);
    }
  };

  const now = new Date();
  const today = dayOf(now);
  const yesterday = dayOf(new Date(now.getTime() - 86_400_000));
  const lines = report?.lines ?? [];
  const sum = useMemo(() => total(lines), [lines]);
  const todaySum = useMemo(() => total(lines.filter((l) => l.day === today)), [lines, today]);
  const rows = useMemo(() => rollup(lines, by), [lines, by]);
  const agents = useMemo(() => {
    const seen = new Set<string>(Object.keys(report?.limits ?? {}));
    for (const l of lines) seen.add(l.agent);
    return [...seen].sort((a, b) => agentName(a).localeCompare(agentName(b)));
  }, [lines, report]);
  const range = RANGES.find((r) => r.days === days)?.label ?? `${days} days`;
  /** Each saved daily cap that today's spend has reached, in one sentence each. */
  const reached = useMemo(() => {
    if (!report) return [];
    const scopes: [string, Caps, Spent][] = [["every repo", report.budgets.all, report.today]];
    for (const [id, c] of Object.entries(report.budgets.repos)) scopes.push([repoName(id), c, report.today_repos[id] ?? { usd: 0, turns: 0, tokens: 0 }]);
    return scopes.flatMap(([name, c, spent]) => meters(c.day, spent).filter((m) => m.reached).map((m) => `Today's cap for ${name} is reached: ${m.text}${m.label === "API-equivalent" ? " (API-equivalent)" : ` ${m.label.toLowerCase()}`}.`));
  }, [report, repoName]);

  return (
    <div className="tq usage">
      <div className="board-head tq-head" data-tauri-drag-region>
        <h1>Usage</h1>
        <span className="muted">Kept on this Mac. Nothing is sent anywhere.</span>
        <span className="spacer" />
        <div className="seg" role="radiogroup" aria-label="Period">
          {RANGES.map((r) => <button key={r.days} role="radio" aria-checked={days === r.days} className={days === r.days ? "on" : ""} onClick={() => setDays(r.days)}>{r.label}</button>)}
        </div>
        <button onClick={exportCsv} disabled={!report} title="Save one row per day, repo, agent and model as a CSV file"><Download size={13} />Export CSV</button>
      </div>
      <div className="tq-body">
        <div className="tq-banner usage-note">
          <Info size={14} aria-hidden />
          <span>Costs are API-equivalent: what each agent reports at API prices. On a subscription such as Claude Pro or Max you pay a flat fee, so this is not your bill. Agents that report no cost show turns and tokens only.</span>
        </div>
        {reached.length > 0 && (
          <div className="tq-banner cap-hit" role="status">
            <Gauge size={14} aria-hidden />
            <span>{reached.join(" ")} The queue starts no new task and autopilot no new turn until midnight. Raise the cap below to carry on.</span>
          </div>
        )}
        {error && <div className="tq-banner cap-hit"><AlertCircle size={14} aria-hidden /><span>Couldn't load usage. {error}</span><span className="spacer" /><button onClick={load}>Try again</button></div>}

        {report && (
          <div className="usage-stats" role="list" aria-label="Totals">
            <Stat label="Today, API-equivalent" value={todaySum.turns ? costText(todaySum).text : money(0)} sub={costSub(todaySum, "today")} partial={todaySum.priced_turns > 0 && todaySum.priced_turns < todaySum.turns} />
            {days > 1 && <Stat label={`${range}, API-equivalent`} value={sum.turns ? costText(sum).text : money(0)} sub={costSub(sum, `in ${range}`)} partial={sum.priced_turns > 0 && sum.priced_turns < sum.turns} />}
            <Stat label="Turns" value={sum.turns.toLocaleString()} sub={range} />
            <Stat label="Tokens in and out" value={sum.turns ? tokenText(sum) : "0"} sub={sum.token_turns ? `${tokens(sum.cached)} read from cache` : sum.turns ? "No agent reported tokens" : range} />
            <Stat label="Agent time" value={sum.turns ? duration(sum.duration_ms) : "No turns yet"} sub={range} />
          </div>
        )}

        {report && draft && (
          <section className="tq-section" aria-label="Spend caps">
            <h2>Spend caps</h2>
            <div className="usage-card">
              <p className="muted usage-explain">
                When a cap is reached, the queue starts no new task and autopilot starts no new turn. A turn that is running finishes, so the total can go a little over. Messages you send yourself always go through. The daily cap resets at midnight. A task is one workspace: everything its agents spent there.
              </p>
              <CapRows title="Every repo" caps={draft.all} spent={report.today} onChange={(c) => setDraft({ ...draft, all: c })} />
              {Object.entries(draft.repos).map(([id, c]) => (
                <CapRows key={id} title={repoName(id)} caps={c} spent={report.today_repos[id] ?? { usd: 0, turns: 0, tokens: 0 }}
                  onChange={(n) => setDraft({ ...draft, repos: { ...draft.repos, [id]: n } })}
                  onRemove={() => { const r = { ...draft.repos }; delete r[id]; setDraft({ ...draft, repos: r }); }} />
              ))}
              <div className="usage-cap-foot">
                {repos.some((r) => !(r.id in draft.repos)) && (
                  <label className="usage-add">
                    <Plus size={13} aria-hidden />
                    <select value="" aria-label="Add caps for one repo" onChange={(e) => e.target.value && setDraft({ ...draft, repos: { ...draft.repos, [e.target.value]: emptyText() } })}>
                      <option value="">Add caps for one repo</option>
                      {repos.filter((r) => !(r.id in draft.repos)).map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
                    </select>
                  </label>
                )}
              </div>
              <h3 className="usage-sub">When an agent goes in circles</h3>
              <p className="muted usage-explain">Checked after each turn the queue or autopilot runs. nekotrail stops giving it turns and the task needs you, with what it saw.</p>
              <div className="usage-runaway">
                <label className="check"><input type="checkbox" checked={draft.runaway.on} onChange={(e) => setDraft({ ...draft, runaway: { ...draft.runaway, on: e.target.checked } })} />Stop an agent that goes in circles</label>
                <Threshold disabled={!draft.runaway.on} before="The same command fails" after="times in a row" min={2} value={draft.runaway.same_failure} onChange={(v) => setDraft({ ...draft, runaway: { ...draft.runaway, same_failure: v } })} />
                <Threshold disabled={!draft.runaway.on} before="A file goes back to an earlier version" after={draft.runaway.back_and_forth === 1 ? "time" : "times"} min={1} value={draft.runaway.back_and_forth} onChange={(v) => setDraft({ ...draft, runaway: { ...draft.runaway, back_and_forth: v } })} />
                <Threshold disabled={!draft.runaway.on} before="" after="turns in a row change no file" min={2} value={draft.runaway.no_change} onChange={(v) => setDraft({ ...draft, runaway: { ...draft.runaway, no_change: v } })} />
              </div>
              {capError && <p className="bad-text usage-error" role="alert">{capError}</p>}
              <div className="row end">
                {dirty && <button className="quiet" onClick={() => { setDraft(draftOf(report.budgets)); setCapError(null); }}>Revert</button>}
                <button className="primary" disabled={!dirty || saving} onClick={saveCaps}>Save caps</button>
              </div>
            </div>
          </section>
        )}

        {report && (
          <section className="tq-section" aria-label="Usage limits">
            <h2>Usage limits</h2>
            <div className="tq-list usage-limits" role="list">
              {agents.length === 0 && <p className="muted usage-empty-line">No agent has run yet.</p>}
              {agents.map((a) => {
                const l = report.limits[a];
                const out = !!l && l.until > report.now;
                return (
                  <div key={a} className="usage-limit" role="listitem" title={l?.text || undefined}>
                    <span className="agent-tag">{agentName(a)}</span>
                    <span className={out ? "wait-text" : "muted"}>{limitText(a, l, report.now)}</span>
                  </div>
                );
              })}
            </div>
            <p className="muted usage-explain usage-under">From what each agent said when it ran out, in any chat. nekotrail only knows a limit once an agent says it hit one, so an agent shown without one may still be close.</p>
          </section>
        )}

        {report && (
          <section className="tq-section" aria-label="Breakdown">
            <h2>Breakdown<span className="spacer" />
              <div className="seg" role="radiogroup" aria-label="Group by">
                {GROUPS.map((g) => <button key={g.by} role="radio" aria-checked={by === g.by} className={by === g.by ? "on" : ""} onClick={() => setBy(g.by)}>{g.label}</button>)}
              </div>
            </h2>
            {rows.length === 0 ? (
              <div className="usage-card"><p className="muted usage-empty-line">No finished turns {days === 1 ? "today" : `in the last ${range}`}. Each turn an agent finishes, in any chat or queued task, shows up here.</p></div>
            ) : (
              <div className="usage-card flush">
                <table className="usage-table">
                  <thead>
                    <tr><th scope="col">{GROUPS.find((g) => g.by === by)!.label}</th><th scope="col" className="num">Turns</th><th scope="col" className="num">Tokens in and out</th><th scope="col" className="num">From cache</th><th scope="col" className="num">API-equivalent</th><th scope="col" className="num">Time</th></tr>
                  </thead>
                  <tbody>
                    {rows.map((r) => {
                      const c = costText(r);
                      return (
                        <tr key={r.key}>
                          <th scope="row" className={by === "model" && r.key ? "mono" : undefined}>{keyName(by, r.key, repoName, today, yesterday)}</th>
                          <td className="num">{r.turns.toLocaleString()}</td>
                          <td className="num">{tokenText(r)}</td>
                          <td className="num">{r.token_turns ? tokens(r.cached) : <span className="muted">Not reported</span>}</td>
                          <td className="num" title={c.note || undefined}>{r.priced_turns ? c.text : <span className="muted">{c.text}</span>}{c.note && r.priced_turns ? <span className="usage-partial" aria-label={c.note}>*</span> : null}</td>
                          <td className="num">{duration(r.duration_ms)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                {rows.some((r) => r.priced_turns > 0 && r.priced_turns < r.turns) && <p className="muted usage-foot">* Some turns here reported no cost, so this covers only the turns that did.</p>}
              </div>
            )}
          </section>
        )}
      </div>
    </div>
  );
}

/** Under a cost total: how many turns it covers, and how many reported no cost and so aren't in it. */
function costSub(r: { turns: number; priced_turns: number }, when: string): string {
  if (r.turns === 0) return `No turns ${when}`;
  const left = r.turns - r.priced_turns;
  const turns = `${r.turns} ${r.turns === 1 ? "turn" : "turns"} ${when}`;
  return left > 0 ? `${turns}. ${left} reported no cost.` : turns;
}

function Stat({ label, value, sub, partial }: { label: string; value: string; sub: string; partial?: boolean }) {
  return (
    <div className="usage-stat" role="listitem">
      <span className="usage-stat-label">{label}</span>
      <span className="usage-stat-value num">{value}{partial && <span className="usage-partial" aria-hidden>*</span>}</span>
      <span className="muted usage-stat-sub">{sub}</span>
    </div>
  );
}

/** One scope's caps: a daily row (with today's spend against it) and a per-task row. */
function CapRows({ title, caps, spent, onChange, onRemove }: { title: string; caps: CapsText; spent: Spent; onChange: (c: CapsText) => void; onRemove?: () => void }) {
  const field = (which: "day" | "task", k: Kind) => {
    const bad = parseCap(caps[which][k], k);
    const msg = "error" in bad ? bad.error : null;
    return (
    <label className={`usage-field${msg ? " bad" : ""}`} title={msg ?? undefined}>
      {k === "usd" && <span className="usage-prefix" aria-hidden>$</span>}
      <input
        inputMode={k === "usd" ? "decimal" : "numeric"}
        value={caps[which][k]}
        placeholder="No cap"
        aria-label={`${title}, ${which === "day" ? "daily cap" : "cap per task"}, ${k === "usd" ? "API-equivalent dollars" : k}`}
        aria-invalid={!!msg}
        onChange={(e) => onChange({ ...caps, [which]: { ...caps[which], [k]: e.target.value } })}
      />
      {k !== "usd" && <span className="usage-suffix" aria-hidden>{k}</span>}
    </label>
    );
  };
  const parsed = (t: CapText): Cap => {
    const out: Cap = { ...NO_CAP };
    for (const k of KINDS) { const r = parseCap(t[k], k); if ("value" in r) out[k] = r.value; }
    return out;
  };
  const rowError = (which: "day" | "task") => {
    for (const k of KINDS) { const r = parseCap(caps[which][k], k); if ("error" in r) return r.error; }
    return null;
  };
  const errLine = (which: "day" | "task") => {
    const e = rowError(which);
    return e ? <p className="bad-text usage-field-error">{e}</p> : null;
  };
  const day = parsed(caps.day);
  const m = meters(day, spent);
  return (
    <div className="usage-caps" role="group" aria-label={title}>
      <div className="usage-caps-head">
        <span className="usage-caps-title">{title}</span>
        {onRemove && <button className="icon" onClick={onRemove} aria-label={`Remove the caps for ${title}`} title="Remove these caps"><Trash2 size={13} /></button>}
      </div>
      <div className="usage-cap-row">
        <span className="usage-cap-label">Each day</span>
        {KINDS.map((k) => <span key={k}>{field("day", k)}</span>)}
      </div>
      {errLine("day")}
      {hasCap(day) && (
        <div className="usage-meters">
          {m.map((x) => (
            <span key={x.label} className={`usage-meter${x.reached ? " hit" : ""}`}>
              <span className="usage-bar" aria-hidden><span style={{ width: `${Math.round(x.share * 100)}%` }} /></span>
              <span className="num">Today {x.text}{x.reached ? ", reached" : ""}</span>
            </span>
          ))}
        </div>
      )}
      <div className="usage-cap-row">
        <span className="usage-cap-label">Each task</span>
        {KINDS.map((k) => <span key={k}>{field("task", k)}</span>)}
      </div>
      {errLine("task")}
    </div>
  );
}

function Threshold({ before, after, value, min, disabled, onChange }: { before: string; after: string; value: number; min: number; disabled: boolean; onChange: (v: number) => void }) {
  return (
    <label className={`usage-threshold${disabled ? " off" : ""}`}>
      {before && <span>{before}</span>}
      <input type="number" min={min} max={50} value={value} disabled={disabled} onChange={(e) => onChange(Math.min(50, Math.max(min, Math.round(Number(e.target.value) || min))))} />
      <span>{after}</span>
    </label>
  );
}
