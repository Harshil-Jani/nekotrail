import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Archive, FileText, MessageSquare, Search, SquareTerminal, User } from "lucide-react";
import { api, errText, type Repo, type SearchGroup, type SearchHit, type SearchProgress } from "../api";
import { agentName } from "../agents";
import { ago, DATE_LABEL, flatten, plainSnippet, snippetParts, type DateRange, sinceFor } from "../search";

type Only = "all" | "user" | "command";
const ONLY: [Only, string][] = [["all", "Everything"], ["user", "My messages"], ["command", "Commands"]];

const KIND_ICON = { user: User, agent: MessageSquare, command: SquareTerminal, file: FileText } as const;
const KIND_LABEL = { user: "You", agent: "Agent", command: "Command", file: "File" } as const;

function Snippet({ text, mono }: { text: string; mono: boolean }) {
  const parts = snippetParts(mono ? text : plainSnippet(text));
  return (
    <span className={`gs-snippet ${mono ? "mono" : ""}`}>
      {parts.map((p, i) => (p.hit ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}
    </span>
  );
}

/**
 * ⌘⇧F: find anything you or an agent said, in every chat of every workspace (archived ones too).
 * Results are grouped by workspace; ↑↓ move, ↵ opens the chat at that message.
 */
export function SearchSheet({ repos, nameOf, onOpen, onClose }: {
  repos: Repo[];
  /** What the sidebar calls a workspace (its task title, else its star). */
  nameOf: (g: SearchGroup) => string;
  onOpen: (hit: SearchHit, group: SearchGroup) => void;
  onClose: () => void;
}) {
  const [q, setQ] = useState(() => sessionStorage.getItem("nekotrail.search") ?? "");
  const [repo, setRepo] = useState("");
  const [agent, setAgent] = useState("");
  const [range, setRange] = useState<DateRange>("any");
  const [only, setOnly] = useState<Only>("all");
  const [workspace, setWorkspace] = useState<SearchGroup | null>(null);
  const [groups, setGroups] = useState<SearchGroup[] | null>(null);
  const [indexing, setIndexing] = useState<SearchProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [sel, setSel] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const seq = useRef(0);

  useEffect(() => { sessionStorage.setItem("nekotrail.search", q); }, [q]);
  useEffect(() => { input.current?.select(); }, []);

  const query = q.trim();
  const run = () => {
    if (!query) { setGroups(null); setError(null); setLoading(false); return; }
    const n = ++seq.current;
    setLoading(true);
    api.searchChats(query + (q.endsWith(" ") ? " " : ""), {
      repo: repo || null, agent: agent || null, since: sinceFor(range), kind: only === "all" ? null : only, workspace: workspace?.workspace_id ?? null,
    })
      .then((r) => { if (n === seq.current) { setGroups(r.groups); setIndexing(r.indexing); setError(null); } })
      .catch((e) => { if (n === seq.current) { setError(errText(e)); setGroups([]); } })
      .finally(() => { if (n === seq.current) setLoading(false); });
  };
  // Search as you type, a beat after the last key.
  useEffect(() => {
    const t = setTimeout(run, 120);
    return () => clearTimeout(t);
  }, [q, repo, agent, range, only, workspace]);
  // Older chats are indexed once in the background: show how far, and search again when it's done.
  const runRef = useRef(run);
  runRef.current = run;
  useEffect(() => {
    const un = listen<SearchProgress>("search-index", ({ payload }) => {
      setIndexing(payload.finished ? null : payload);
      if (payload.finished) runRef.current();
    });
    return () => { un.then((f) => f()); };
  }, []);

  const items = useMemo(() => flatten(groups ?? []), [groups]);
  useEffect(() => setSel(0), [groups]);
  useEffect(() => {
    list.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const activate = (i: number) => {
    const it = items[i];
    if (!it) return;
    if (it.type === "more") { setWorkspace(it.group); return; }
    onClose();
    onOpen(it.hit, it.group);
  };
  const filtered = !!(repo || agent || range !== "any" || only !== "all" || workspace);
  const agents = useMemo(() => {
    const seen = new Set<string>();
    for (const g of groups ?? []) for (const h of g.hits) seen.add(h.agent);
    if (agent) seen.add(agent);
    return [...seen].sort();
  }, [groups, agent]);
  const hitCount = (groups ?? []).reduce((n, g) => n + g.hits.length + g.more, 0);
  const pct = indexing && indexing.total ? Math.floor((indexing.done / indexing.total) * 100) : 0;

  let n = -1;
  return (
    <div className="palette-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="palette gs" role="dialog" aria-modal="true" aria-label="Search every chat">
        <div className="palette-input">
          <Search size={16} />
          <input
            ref={input}
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search every chat: prompts, replies, commands, files"
            aria-label="Search every chat"
            role="combobox"
            aria-expanded="true"
            aria-controls="gs-list"
            aria-activedescendant={items.length ? `gs-${sel}` : undefined}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(items.length - 1, s + 1)); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
              else if (e.key === "Enter") { e.preventDefault(); activate(sel); }
              else if (e.key === "Escape") { e.preventDefault(); if (workspace) setWorkspace(null); else onClose(); }
              else if (e.key === "Backspace" && !q && workspace) { e.preventDefault(); setWorkspace(null); }
            }}
          />
          <kbd>esc</kbd>
        </div>
        <div className="gs-filters" role="group" aria-label="Filters">
          <div className="seg" role="radiogroup" aria-label="Show">
            {ONLY.map(([v, label]) => (
              <button key={v} role="radio" aria-checked={only === v} className={only === v ? "on" : ""} onClick={() => setOnly(v)}>{label}</button>
            ))}
          </div>
          <select aria-label="Repo" value={repo} onChange={(e) => setRepo(e.target.value)} className={repo ? "set" : ""}>
            <option value="">All repos</option>
            {repos.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
          </select>
          <select aria-label="Agent" value={agent} onChange={(e) => setAgent(e.target.value)} className={agent ? "set" : ""}>
            <option value="">Any agent</option>
            {agents.map((a) => <option key={a} value={a}>{agentName(a as never)}</option>)}
          </select>
          <select aria-label="Date" value={range} onChange={(e) => setRange(e.target.value as DateRange)} className={range !== "any" ? "set" : ""}>
            {(Object.keys(DATE_LABEL) as DateRange[]).map((d) => <option key={d} value={d}>{DATE_LABEL[d]}</option>)}
          </select>
          {workspace && (
            <button className="chip gs-chip" onClick={() => setWorkspace(null)} title="Search every workspace again" aria-label={`Only in ${nameOf(workspace)}. Remove.`}>
              Only in {nameOf(workspace)} <span aria-hidden>×</span>
            </button>
          )}
          <span className="spacer" />
          {filtered && (
            <button className="quiet gs-clear" onClick={() => { setRepo(""); setAgent(""); setRange("any"); setOnly("all"); setWorkspace(null); }}>Clear filters</button>
          )}
        </div>
        <div className="palette-list gs-list" id="gs-list" role="listbox" aria-label="Results" ref={list} aria-busy={loading}>
          {error && <p className="gs-empty error">Search didn't work: {error}</p>}
          {!error && !query && (
            <div className="gs-empty">
              <p>Find anything you or an agent said, in every workspace. Archived ones too.</p>
              <p className="muted">Searches your prompts, the agents' replies, the commands they ran and the files they touched.</p>
            </div>
          )}
          {!error && query && groups?.length === 0 && !loading && (
            <div className="gs-empty">
              <p>Nothing matches “{query}”{filtered ? " with these filters" : ""}.</p>
              <p className="muted">{filtered ? "Clear a filter, or try fewer words." : "Try fewer words, or the start of a word."}</p>
            </div>
          )}
          {groups?.map((g) => (
            <section key={g.workspace_id} className="gs-group" aria-label={nameOf(g)}>
              <div className="gs-head">
                <span className="gs-ws">{nameOf(g)}</span>
                <span className="muted gs-where">{g.repo}{g.repo && " · "}{g.branch}</span>
                {g.state === "archived" && <span className="gs-archived"><Archive size={11} />Archived</span>}
              </div>
              {g.hits.map((h) => {
                const i = ++n;
                const Icon = KIND_ICON[h.kind] ?? MessageSquare;
                return (
                  <button
                    key={h.message_id}
                    id={`gs-${i}`}
                    data-i={i}
                    role="option"
                    aria-selected={i === sel}
                    className={`gs-hit ${i === sel ? "on" : ""}`}
                    onMouseMove={() => setSel(i)}
                    onClick={() => activate(i)}
                  >
                    <span className="gs-kind" title={KIND_LABEL[h.kind]} aria-label={KIND_LABEL[h.kind]}><Icon size={14} /></span>
                    <Snippet text={h.snippet} mono={h.kind === "command" || h.kind === "file"} />
                    <span className="gs-meta">{agentName(h.agent)}{h.chat && h.chat !== "New chat" ? ` · ${h.chat}` : ""} · {ago(h.created_at)}</span>
                  </button>
                );
              })}
              {g.more > 0 && (() => {
                const i = ++n;
                return (
                  <button id={`gs-${i}`} data-i={i} role="option" aria-selected={i === sel} className={`gs-more ${i === sel ? "on" : ""}`} onMouseMove={() => setSel(i)} onClick={() => activate(i)}>
                    {g.more} more in this workspace
                  </button>
                );
              })()}
            </section>
          ))}
        </div>
        <div className="gs-foot muted">
          {indexing ? (
            <span className="gs-indexing" role="status"><span className="spinner" />Indexing older chats, {pct}%. Results fill in as it goes.</span>
          ) : query && groups && groups.length > 0 ? (
            <span role="status">{hitCount.toLocaleString()} {hitCount === 1 ? "match" : "matches"} in {groups.length} {groups.length === 1 ? "workspace" : "workspaces"}</span>
          ) : <span />}
          <span className="spacer" />
          <span><kbd>↑</kbd> <kbd>↓</kbd> to move</span>
          <span><kbd>↵</kbd> to open</span>
        </div>
      </div>
    </div>
  );
}
