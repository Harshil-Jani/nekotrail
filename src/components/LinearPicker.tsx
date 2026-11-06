import { useEffect, useRef, useState } from "react";
import { Search } from "lucide-react";
import { api, errText } from "../api";
import { DEFAULT_FILTER, STATE_FILTERS, emptyText, issueMeta, type LinearFilter, type LinearIssue, type LinearStatus, type LinearTeam } from "../linear";

/**
 * New workspace, Linear: issues assigned to you (or anyone's), with search, team and state.
 * Picking one fills the message box with the issue; the workspace takes its branch name.
 */
export function LinearPicker({ picked, onPick, onOpenSettings }: {
  picked: LinearIssue | null;
  onPick: (i: LinearIssue) => void;
  onOpenSettings?: () => void;
}) {
  const [status, setStatus] = useState<LinearStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [teams, setTeams] = useState<LinearTeam[]>([]);
  const [filter, setFilter] = useState<LinearFilter>(() => {
    const saved = JSON.parse(localStorage.getItem("nekotrail.linearFilter") || "{}");
    return { ...DEFAULT_FILTER, anyone: !!saved.anyone, team: saved.team ?? null, state: saved.state ?? "open" };
  });
  const [issues, setIssues] = useState<LinearIssue[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  const asked = useRef(0);

  useEffect(() => { api.linearStatus().then(setStatus).catch((e) => setStatusError(errText(e))); }, []);
  const connected = !!status?.connected;
  useEffect(() => { if (connected) api.linearTeams().then(setTeams).catch(() => setTeams([])); }, [connected]);
  useEffect(() => {
    if (!connected) return;
    localStorage.setItem("nekotrail.linearFilter", JSON.stringify({ anyone: filter.anyone, team: filter.team, state: filter.state }));
    const n = ++asked.current;
    setIssues(null); setError(null);
    // Typing waits a moment, so each key isn't a request.
    const t = setTimeout(() => {
      api.linearIssues(filter)
        .then((list) => { if (n === asked.current) setIssues(list); })
        .catch((e) => { if (n === asked.current) { setIssues([]); setError(errText(e)); } });
    }, filter.query ? 300 : 0);
    return () => clearTimeout(t);
  }, [connected, filter, tries]);

  if (statusError) return <div className="nw-list"><p className="bad">{statusError}</p></div>;
  if (!status) return <div className="nw-list"><p className="muted"><span className="spinner" /> Checking Linear…</p></div>;
  if (!status.connected) {
    return (
      <div className="linear-connect">
        <p>Start from a Linear issue: the workspace gets its branch name and title, and the issue as its first message.</p>
        <p className="muted">Connect Linear with a personal API key first. It's kept in your keychain.</p>
        {onOpenSettings && <button className="primary" onClick={onOpenSettings}>Connect Linear…</button>}
      </div>
    );
  }
  const set = (p: Partial<LinearFilter>) => setFilter((f) => ({ ...f, ...p }));
  const team = teams.find((t) => t.id === filter.team);
  return (
    <>
      <div className="linear-filters">
        <label className="linear-search">
          <Search size={13} aria-hidden />
          <input value={filter.query} onChange={(e) => set({ query: e.target.value })} placeholder="Search titles, or type ENG-123" aria-label="Search Linear issues" spellCheck={false} />
        </label>
        <div className="seg linear-who" role="radiogroup" aria-label="Whose issues">
          <button role="radio" aria-checked={!filter.anyone} className={filter.anyone ? "" : "on"} onClick={() => set({ anyone: false })}>Mine</button>
          <button role="radio" aria-checked={filter.anyone} className={filter.anyone ? "on" : ""} onClick={() => set({ anyone: true })}>Anyone</button>
        </div>
        <select value={filter.team ?? ""} onChange={(e) => set({ team: e.target.value || null })} aria-label="Team">
          <option value="">All teams</option>
          {teams.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
        </select>
        <select value={filter.state} onChange={(e) => set({ state: e.target.value as LinearFilter["state"] })} aria-label="State">
          {STATE_FILTERS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}
        </select>
      </div>
      <div className="nw-list" role="listbox" aria-label="Linear issues">
        {issues === null && <p className="muted"><span className="spinner" /> Loading Linear issues…</p>}
        {issues?.length === 0 && (error
          ? <p className="bad" role="alert">Couldn't load issues. {error} <button className="link" onClick={() => setTries((n) => n + 1)}>Try again</button></p>
          : <p className="muted">{emptyText(filter, team?.name)}</p>)}
        {issues?.map((i) => (
          <button key={i.id} role="option" aria-selected={picked?.id === i.id} className={`linear-row ${picked?.id === i.id ? "on" : ""}`} onClick={() => onPick(i)}>
            <span className="num muted linear-id">{i.identifier}</span>
            <span className="nw-pr-title">{i.title}</span>
            <span className="muted linear-meta">{issueMeta(i)}</span>
          </button>
        ))}
      </div>
      {picked && (
        <p className="muted nw-note">
          {picked.branch
            ? <>Linear suggests the branch <code>{picked.branch}</code>. If your repo already has it, the workspace gets the next free name. </>
            : <>The branch is named after {picked.identifier}. </>}
          Its PR will say "Fixes {picked.identifier}", so Linear links it.
        </p>
      )}
    </>
  );
}
