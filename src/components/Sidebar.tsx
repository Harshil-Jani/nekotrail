import { useEffect, useState } from "react";
import { Archive, ArchiveRestore, Check, ChevronDown, ChevronRight, Filter, FolderGit2, ListChecks, Pin, Plus, Search, Settings2, Star, X } from "lucide-react";
import { getSettings, saveSettings } from "../settings";
import { api, displayName, statusOf, STATUS_LABEL, type GithubUser, type Repo, type RepoGithub, type Sky, type Workspace } from "../api";
import { Mascot, type Mood } from "./Mascot";
import { Avatar } from "./Avatar";

type Props = {
  repos: Repo[];
  workspaces: Workspace[];
  sky: Sky;
  lit: number;
  me: GithubUser;
  running: Set<string>; // workspace ids with a running agent
  selected: string | null;
  showArchived: boolean;
  onSelect: (id: string) => void;
  onNewWorkspace: (repoId: string) => void;
  onAddRepo: () => void;
  onRepoSettings: (repoId: string) => void;
  onToggleArchived: () => void;
  onSky: () => void;
  mood: Mood;
  purring: boolean;
  onPet: () => void;
  /** Workspace being archived; its row animates off before it disappears. */
  leaving: string | null;
  groupBy: "repo" | "status";
  onGroupBy: (g: "repo" | "status") => void;
  /** PR state per workspace id, for status grouping. */
  prStates: Record<string, string>;
  /** Lines added / removed per workspace id. */
  lines: Record<string, [number, number]>;
};

function RepoHead({ repo, onNew, onSettings, collapsed, onToggle, count }: { repo: Repo; onNew: () => void; onSettings: () => void; collapsed: boolean; onToggle: () => void; count: number }) {
  const [gh, setGh] = useState<RepoGithub>(null);
  useEffect(() => { api.repoGithub(repo.id).then(setGh); }, [repo.id]);
  return (
    <div className="repo-head">
      <button className="fold" onClick={onToggle} aria-expanded={!collapsed} aria-label={collapsed ? `Show ${repo.name}` : `Fold ${repo.name}`}>
        {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
      </button>
      {gh?.owner_avatar ? (
        <Avatar src={gh.owner_avatar} size={20} label={gh.nameWithOwner} />
      ) : (
        <span className="avatar" style={{ width: 20, height: 20 }} />
      )}
      <span className="repo-name" title={gh?.description || repo.root_path} onClick={onToggle}>{repo.name}</span>
      {collapsed && count > 0 && <span className="repo-meta num">{count}</span>}
      {gh && gh.stargazerCount > 0 && (
        <span className="repo-meta num" title={`${gh.stargazerCount} stars on ${gh.nameWithOwner}`}>
          <Star size={12} />{gh.stargazerCount >= 1000 ? `${(gh.stargazerCount / 1000).toFixed(1)}k` : gh.stargazerCount}
        </span>
      )}
      <span className="repo-actions">
        <button className="icon" title="Repo scripts" aria-label={`${repo.name} scripts`} onClick={onSettings}><Settings2 size={16} /></button>
        <button className="icon" title="New workspace (⌘N)" aria-label={`New workspace in ${repo.name}`} onClick={onNew}><Plus size={18} /></button>
      </span>
    </div>
  );
}

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  // Project filter and folded sections, remembered between launches.
  const [filter, setFilterState] = useState<string[]>(() => getSettings().repoFilter);
  const [collapsed, setCollapsedState] = useState<string[]>(() => getSettings().collapsed);
  const [filterOpen, setFilterOpen] = useState(false);
  useEffect(() => {
    const sync = () => { setFilterState(getSettings().repoFilter); setCollapsedState(getSettings().collapsed); };
    window.addEventListener("nekotrail-settings", sync);
    return () => window.removeEventListener("nekotrail-settings", sync);
  }, []);
  const setFilter = (f: string[]) => saveSettings({ ...getSettings(), repoFilter: f });
  const toggleCollapsed = (id: string) => {
    const c = getSettings().collapsed;
    saveSettings({ ...getSettings(), collapsed: c.includes(id) ? c.filter((x) => x !== id) : [...c, id] });
  };
  const shownRepos = p.repos.filter((r) => !filter.length || filter.includes(r.id));
  const inFilter = (w: Workspace) => !filter.length || filter.includes(w.repo_id);
  const q = query.trim().toLowerCase();
  const label = (w: Workspace) => displayName(p.sky, w);
  const match = (w: Workspace) => !q || label(w).toLowerCase().includes(q) || w.branch.toLowerCase().includes(q);
  const total = p.sky.stars.length;
  const repoName = (id: string) => p.repos.find((r) => r.id === id)?.name ?? "";
  const row = (w: Workspace) => {
    const l = p.lines[w.id];
    return (
      <button
        key={w.id}
        className={`ws ${p.selected === w.id ? "selected" : ""} ${w.state === "archived" ? "archived" : ""} ${p.leaving === w.id ? "leaving" : ""}`}
        onClick={() => p.onSelect(w.id)}
      >
        <span className="ws-mark">
          {p.running.has(w.id) ? <span className="spinner" aria-label="Agent working" /> : w.unread ? <span className="unread" aria-label="Unread" /> : null}
        </span>
        <span className="ws-text">
          <span className="ws-name">{label(w)}</span>
          <span className="ws-branch">{p.groupBy === "status" ? `${repoName(w.repo_id)} · ` : ""}{w.branch}</span>
        </span>
        <span className="spacer" />
        {l && (l[0] > 0 || l[1] > 0) && <span className="ws-lines num"><span className="add">+{l[0]}</span><span className="del">-{l[1]}</span></span>}
        {w.pinned && <Pin size={14} className="muted" aria-label="Pinned" />}
      </button>
    );
  };
  const pct = total ? Math.round((p.lit / total) * 100) : 0;

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <Mascot size={26} mood={p.mood} follow purring={p.purring} onClick={p.onPet} title="Pet the cat" />
        <span className="wordmark">nekotrail</span>
        {p.mood === "sleep" && <span className="zzz" aria-hidden>z z</span>}
        <span className="spacer" />
        <span className="filter-wrap">
          <button className={`icon ${filter.length ? "on" : ""}`} onClick={() => setFilterOpen((o) => !o)} title="Filter by project" aria-label="Filter by project" aria-expanded={filterOpen}>
            <Filter size={16} />
          </button>
          {filterOpen && (
            <span className="menu filter-menu" role="menu" onMouseLeave={() => setFilterOpen(false)}>
              <button role="menuitemradio" aria-checked={!filter.length} onClick={() => { setFilter([]); setFilterOpen(false); }}>
                {!filter.length ? <Check size={14} /> : <span className="check-space" />}<span>All projects</span>
              </button>
              {p.repos.map((r) => {
                const on = filter.includes(r.id);
                const n = p.workspaces.filter((w) => w.repo_id === r.id && w.state === "active").length;
                return (
                  <button key={r.id} role="menuitemcheckbox" aria-checked={on}
                    onClick={(e) => setFilter(e.metaKey || e.shiftKey ? (on ? filter.filter((x) => x !== r.id) : [...filter, r.id]) : on && filter.length === 1 ? [] : [r.id])}>
                    {on ? <Check size={14} /> : <span className="check-space" />}<span>{r.name}</span><span className="muted num">{n}</span>
                  </button>
                );
              })}
              <p className="muted menu-hint">Click shows one project. ⌘-click adds more.</p>
            </span>
          )}
        </span>
        <button
          className="icon"
          onClick={() => p.onGroupBy(p.groupBy === "repo" ? "status" : "repo")}
          title={p.groupBy === "repo" ? "Group by status" : "Group by repo"}
          aria-label={p.groupBy === "repo" ? "Group by status" : "Group by repo"}
        >
          {p.groupBy === "repo" ? <ListChecks size={17} /> : <FolderGit2 size={17} />}
        </button>
        <button className="icon" onClick={p.onAddRepo} title="Add a local git repo (⌘⌥A)" aria-label="Add repo"><Plus size={18} /></button>
      </div>
      {filter.length > 0 && (
        <div className="filter-chips">
          {filter.map((id) => {
            const r = p.repos.find((x) => x.id === id);
            return r ? <button key={id} className="chip" onClick={() => setFilter(filter.filter((x) => x !== id))} aria-label={`Stop filtering by ${r.name}`}>{r.name}<X size={11} /></button> : null;
          })}
          <button className="quiet chip-clear" onClick={() => setFilter([])}>Show all</button>
        </div>
      )}
      <label className="search">
        <Search size={14} />
        <input placeholder="Search workspaces" value={query} onChange={(e) => setQuery(e.target.value)} id="workspace-search" aria-label="Filter workspaces" />
      </label>
      <div className="sidebar-scroll">
        {p.repos.length === 0 && <p className="muted" style={{ padding: "0 16px" }}>No repos yet. Add one with +.</p>}
        {p.groupBy === "repo"
          ? shownRepos.map((r) => {
              const ws = p.workspaces.filter((w) => w.repo_id === r.id && match(w));
              const folded = collapsed.includes(r.id) && !q;
              return (
                <section key={r.id} className="repo">
                  <RepoHead repo={r} onNew={() => p.onNewWorkspace(r.id)} onSettings={() => p.onRepoSettings(r.id)} collapsed={folded} onToggle={() => toggleCollapsed(r.id)} count={ws.length} />
                  {!folded && ws.map(row)}
                  {!folded && ws.length === 0 && !q && <button className="ws-empty" onClick={() => p.onNewWorkspace(r.id)}><Plus size={13} />New workspace</button>}
                </section>
              );
            })
          : (["in_progress", "in_review", "backlog", "done"] as const).map((st) => {
              const ws = p.workspaces.filter((w) => w.state === "active" && inFilter(w) && match(w) && statusOf(w, p.prStates[w.id]) === st);
              if (!ws.length) return null;
              return (
                <section key={st} className="repo">
                  <div className="repo-head status-head"><span className={`status-dot st-${st}`} /><span className="repo-name">{STATUS_LABEL[st]}</span><span className="repo-meta num">{ws.length}</span></div>
                  {ws.map(row)}
                </section>
              );
            })}
      </div>
      <div className="sidebar-foot">
        <button className="me" onClick={p.onSky} title="Open your night sky">
          {p.me ? <Avatar src={p.me.avatar_url} size={28} label={p.me.login} /> : <Mascot size={28} />}
          <span className="me-text">
            <span className="me-name">{p.me?.name || p.me?.login || "Not signed in to gh"}</span>
            <span className="muted num">{p.me ? `@${p.me.login} · ` : ""}{p.lit}/{total} stars</span>
          </span>
        </button>
        <div className="progress-bar" aria-label={`${pct}% of stars lit`}><span style={{ width: `${pct}%` }} /></div>
        <button className="quiet" onClick={p.onToggleArchived}>
          {p.showArchived ? <ArchiveRestore size={16} /> : <Archive size={16} />}
          {p.showArchived ? "Hide archived" : "Show archived"}
        </button>
      </div>
    </aside>
  );
}
