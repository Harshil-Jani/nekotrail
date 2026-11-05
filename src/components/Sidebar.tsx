import { useEffect, useState } from "react";
import { ArrowLeft, ArrowRight, MemoryStick, PanelLeft, GitBranch, GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, Archive, ArchiveRestore, Check, ChevronDown, ChevronRight, Ellipsis, Filter, FolderGit2, FolderInput, Kanban, ListChecks, Mail, MailOpen, Pencil, Pin, PinOff, Plus, Search, Settings2, Star, Trash2, X } from "lucide-react";
import { getSettings, saveSettings } from "../settings";
import { api, displayName, statusOf, STATUS_LABEL, type Section, type GithubUser, type Repo, type RepoGithub, type Sky, type Workspace } from "../api";
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
  usage: { today: number; week: number; by_agent: Record<string, number> } | null;
  sections: Section[];
  /** Reload workspaces and sections after a change made here. */
  onChanged: () => void;
  onRename: (id: string) => void;
  onArchive: (id: string) => void;
  onBoard: () => void;
  ram: { total_mb: number; top: { name: string; mb: number }[] } | null;
  canBack: boolean;
  canForward: boolean;
  onBack: () => void;
  onForward: () => void;
  onToggleSidebar: () => void;
};

const DRAG = "application/x-nekotrail-workspace";

/** The workspace's PR at a glance: branch only, draft, open, merged or closed. */
function PrIcon({ state }: { state?: string }) {
  switch (state) {
    case "OPEN": return <GitPullRequest size={15} className="pr-open" aria-label="PR open" />;
    case "DRAFT": return <GitPullRequestDraft size={15} className="pr-draft" aria-label="Draft PR" />;
    case "MERGED": return <GitMerge size={15} className="pr-merged" aria-label="PR merged" />;
    case "CLOSED": return <GitPullRequestClosed size={15} className="pr-closed" aria-label="PR closed" />;
    default: return <GitBranch size={15} className="pr-none" aria-label="No PR yet" />;
  }
}

function RepoHead({ repo, onNew, onSettings, collapsed, onToggle, count }: { repo: Repo; onNew: () => void; onSettings: () => void; collapsed: boolean; onToggle: () => void; count: number }) {
  const [gh, setGh] = useState<RepoGithub>(null);
  useEffect(() => { api.repoGithub(repo.id).then(setGh); }, [repo.id]);
  return (
    <div className="repo-head">
      <button className="fold" onClick={onToggle} aria-expanded={!collapsed} aria-label={collapsed ? `Expand ${repo.name}` : `Collapse ${repo.name}`}>
        {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
      </button>
      {gh?.owner_avatar ? (
        <Avatar src={gh.owner_avatar} size={20} label={gh.nameWithOwner} />
      ) : (
        <Avatar src="" size={20} label={repo.name} />
      )}
      <span className="repo-name" title={gh?.description || repo.root_path} onClick={onToggle}>{repo.name}</span>
      {collapsed && count > 0 && <span className="repo-meta num">{count}</span>}
      {gh && gh.stargazerCount > 0 && (
        <span className="repo-meta num" title={`${gh.stargazerCount} stars on ${gh.nameWithOwner}`}>
          <Star size={12} />{gh.stargazerCount >= 1000 ? `${(gh.stargazerCount / 1000).toFixed(1)}k` : gh.stargazerCount}
        </span>
      )}
      <span className="repo-actions">
        <button className="icon" title="Edit repo scripts" aria-label={`Edit ${repo.name} scripts`} onClick={onSettings}><Settings2 size={16} /></button>
        <button className="icon" title="Create workspace (⌘N)" aria-label={`Create workspace in ${repo.name}`} onClick={onNew}><Plus size={18} /></button>
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
  // Right-click menu, drag targets and section editing.
  const [ctx, setCtx] = useState<{ w: Workspace; x: number; y: number } | null>(null);
  const [moveOpen, setMoveOpen] = useState(false);
  const [over, setOver] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: string | null; name: string } | null>(null);
  const [secMenu, setSecMenu] = useState<string | null>(null);
  useEffect(() => {
    if (!ctx) return;
    const close = () => { setCtx(null); setMoveOpen(false); };
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    return () => { window.removeEventListener("mousedown", close); window.removeEventListener("blur", close); };
  }, [ctx]);
  const act = async (fn: () => Promise<unknown>) => { setCtx(null); setMoveOpen(false); await fn(); p.onChanged(); };
  /** Where a dropped workspace goes: "pinned", a section id, or "" (back to its project). */
  const drop = (target: string) => ({
    onDragOver: (e: React.DragEvent) => { if (e.dataTransfer.types.includes(DRAG)) { e.preventDefault(); setOver(target); } },
    onDragLeave: () => setOver((o) => (o === target ? null : o)),
    onDrop: async (e: React.DragEvent) => {
      const id = e.dataTransfer.getData(DRAG);
      setOver(null);
      if (!id) return;
      e.preventDefault();
      if (target === "pinned") await api.setPinned(id, true);
      else {
        await api.setSection(id, target);
        if (p.workspaces.find((w) => w.id === id)?.pinned) await api.setPinned(id, false);
      }
      p.onChanged();
    },
  });
  const saveSection = async () => {
    if (!editing) return;
    const name = editing.name.trim();
    const target = editing.id;
    setEditing(null);
    if (!name) return;
    if (target) await api.renameSection(target, name);
    else await api.createSection(name);
    p.onChanged();
  };
  const sectionHead = (id: string, name: string, count: number, icon: React.ReactNode, menu?: boolean) => {
    const folded = collapsed.includes(`sec:${id}`) && !q;
    return (
      <div className={`repo-head section-head ${over === id ? "drop" : ""}`} {...drop(id === "pinned" ? "pinned" : id)}>
        <button className="fold" onClick={() => toggleCollapsed(`sec:${id}`)} aria-expanded={!folded} aria-label={folded ? `Expand ${name}` : `Collapse ${name}`}>
          {folded ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
        </button>
        {icon}
        {editing?.id === id ? (
          <input className="section-edit" autoFocus value={editing.name} onChange={(e) => setEditing({ id, name: e.target.value })}
            onBlur={saveSection} onKeyDown={(e) => { if (e.key === "Enter") saveSection(); if (e.key === "Escape") setEditing(null); }} />
        ) : (
          <span className="repo-name" onClick={() => toggleCollapsed(`sec:${id}`)} onDoubleClick={() => menu && setEditing({ id, name })}>{name}</span>
        )}
        <span className="repo-meta num">{count || ""}</span>
        {menu && (
          <span className="repo-actions">
            <span className="git-menu">
              <button className="icon" aria-label={`${name} options`} onClick={() => setSecMenu((m) => (m === id ? null : id))}><Ellipsis size={14} /></button>
              {secMenu === id && (
                <span className="menu sec-menu" role="menu" onMouseLeave={() => setSecMenu(null)}>
                  <button role="menuitem" onClick={() => { setSecMenu(null); setEditing({ id, name }); }}><Pencil size={14} /><span>Rename section</span></button>
                  <button role="menuitem" onClick={async () => { setSecMenu(null); await api.deleteSection(id); p.onChanged(); }}><Trash2 size={14} /><span>Delete section</span></button>
                </span>
              )}
            </span>
          </span>
        )}
      </div>
    );
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
        draggable={w.state === "active"}
        onDragStart={(e) => { e.dataTransfer.setData(DRAG, w.id); e.dataTransfer.effectAllowed = "move"; }}
        onContextMenu={(e) => { e.preventDefault(); setCtx({ w, x: e.clientX, y: e.clientY }); }}
      >
        <span className="ws-mark">
          {p.running.has(w.id) ? <span className="spinner" aria-label="Agent working" /> : <PrIcon state={p.prStates[w.id]} />}
          {w.unread && !p.running.has(w.id) && <span className="unread badge" aria-label="Unread" />}
        </span>
        <span className="ws-name" title={`${label(w)}\n${repoName(w.repo_id)} · ${w.branch}`}>{label(w)}</span>
        {l && (l[0] > 0 || l[1] > 0) && <span className="ws-lines num"><span className="add">+{l[0]}</span><span className="del">-{l[1]}</span></span>}
        {w.pinned && <Pin size={14} className="muted" aria-label="Pinned" />}
      </button>
    );
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-head" data-tauri-drag-region>
        <button className="icon sidebar-toggle" onClick={p.onToggleSidebar} title="Hide sidebar (⌘B)" aria-label="Hide sidebar"><PanelLeft size={16} /></button>
        <Mascot size={22} mood={p.mood} follow purring={p.purring} onClick={p.onPet} title="Pet the cat" />
        {p.mood === "sleep" && <span className="zzz" aria-hidden>z z</span>}
        <span className="spacer" />
        {p.ram && (
          <span className="ram num" title={`Memory used by nekotrail, its agents, terminals and dev servers:\n${p.ram.top.map((t) => `${t.name}  ${t.mb} MB`).join("\n")}`}>
            <MemoryStick size={13} />{p.ram.total_mb >= 1024 ? `${(p.ram.total_mb / 1024).toFixed(2)} GB` : `${p.ram.total_mb} MB`}
          </span>
        )}
        <button className="icon" onClick={p.onBack} disabled={!p.canBack} title="Back (⌘[)" aria-label="Back"><ArrowLeft size={16} /></button>
        <button className="icon" onClick={p.onForward} disabled={!p.canForward} title="Forward (⌘])" aria-label="Forward"><ArrowRight size={16} /></button>
      </div>
      {filter.length > 0 && (
        <div className="filter-chips">
          {filter.map((id) => {
            const r = p.repos.find((x) => x.id === id);
            return r ? <button key={id} className="chip" onClick={() => setFilter(filter.filter((x) => x !== id))} aria-label={`Stop filtering by ${r.name}`}>{r.name}<X size={11} /></button> : null;
          })}
          <button className="quiet chip-clear" onClick={() => setFilter([])}>Show all repos</button>
        </div>
      )}
      <div className="search-row">
        <label className="search">
          <Search size={14} />
          <input placeholder="Search" value={query} onChange={(e) => setQuery(e.target.value)} id="workspace-search" aria-label="Filter workspaces" />
        </label>
        <button className="icon" onClick={p.onBoard} title="Open board (⌘⇧K)" aria-label="Open board"><Kanban size={16} /></button>
        <span className="filter-wrap">
          <button className={`icon ${filter.length ? "on" : ""}`} onClick={() => setFilterOpen((o) => !o)} title="Filter by repo" aria-label="Filter by repo" aria-expanded={filterOpen}>
            <Filter size={16} />
          </button>
          {filterOpen && (
            <span className="menu filter-menu" role="menu" onMouseLeave={() => setFilterOpen(false)}>
              <button role="menuitemradio" aria-checked={!filter.length} onClick={() => { setFilter([]); setFilterOpen(false); }}>
                {!filter.length ? <Check size={14} /> : <span className="check-space" />}<span>All repos</span>
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
              <p className="muted menu-hint">Click to show one repo. ⌘-click to add more.</p>
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
        <button className="icon" onClick={p.onAddRepo} title="Add local repo (⌘⌥A)" aria-label="Add repo"><Plus size={18} /></button>
      </div>
      <div className="sidebar-scroll">
        {p.repos.length === 0 && <p className="muted" style={{ padding: "0 16px" }}>No repos yet. Add one with +.</p>}
        {p.groupBy === "repo" && p.repos.length > 0 && (() => {
          const live = p.workspaces.filter((w) => w.state === "active" && inFilter(w) && match(w));
          const pinned = live.filter((w) => w.pinned);
          // Inbox: workspaces whose agent finished while you were elsewhere.
          const news = live.filter((w) => w.unread && !p.running.has(w.id) && !w.pinned);
          const folded = (id: string) => collapsed.includes(`sec:${id}`) && !q;
          return (
            <>
              {(pinned.length > 0 || over === "pinned") && (
                <section className="repo">
                  {sectionHead("pinned", "Pinned", pinned.length, <Pin size={12} className="muted" />)}
                  {!folded("pinned") && pinned.map(row)}
                </section>
              )}
              {news.length > 0 && (
                <section className="repo needs-you">
                  <div className="repo-head status-head"><span className="unread" /><span className="repo-name">Needs you</span><span className="repo-meta num">{news.length}</span></div>
                  {news.map(row)}
                </section>
              )}
              {p.sections.map((sec) => {
                const ws = live.filter((w) => w.section_id === sec.id && !w.pinned);
                return (
                  <section key={sec.id} className="repo">
                    {sectionHead(sec.id, sec.name, ws.length, null, true)}
                    {!folded(sec.id) && ws.map(row)}
                    {!folded(sec.id) && ws.length === 0 && <div className={`drop-hint ${over === sec.id ? "drop" : ""}`} {...drop(sec.id)}>Drag a workspace here</div>}
                  </section>
                );
              })}
              {editing?.id === null ? (
                <div className="repo-head section-head">
                  <span className="check-space" />
                  <input className="section-edit" autoFocus placeholder="Section name" value={editing.name} onChange={(e) => setEditing({ id: null, name: e.target.value })}
                    onBlur={saveSection} onKeyDown={(e) => { if (e.key === "Enter") saveSection(); if (e.key === "Escape") setEditing(null); }} />
                </div>
              ) : (
                <button className="ws-empty new-section" onClick={() => setEditing({ id: null, name: "" })}><Plus size={13} />Add section</button>
              )}
            </>
          );
        })()}
        {p.groupBy === "repo"
          ? shownRepos.map((r) => {
              const ws = p.workspaces.filter((w) => w.repo_id === r.id && w.state === "active" && match(w) && !(w.pinned || (w.section_id && p.sections.some((s) => s.id === w.section_id))));
              const folded = collapsed.includes(r.id) && !q;
              return (
                <section key={r.id} className="repo" {...drop("")}>
                  <RepoHead repo={r} onNew={() => p.onNewWorkspace(r.id)} onSettings={() => p.onRepoSettings(r.id)} collapsed={folded} onToggle={() => toggleCollapsed(r.id)} count={ws.length} />
                  {!folded && ws.map(row)}
                  {!folded && ws.length === 0 && !q && <button className="ws-empty" onClick={() => p.onNewWorkspace(r.id)}><Plus size={13} />Create workspace</button>}
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
        {p.showArchived && (() => {
          const archived = p.workspaces.filter((w) => w.state === "archived" && inFilter(w) && match(w));
          return (
            <section className="repo archived-group">
              <div className="repo-head status-head"><Archive size={12} className="muted" /><span className="repo-name">Archived</span><span className="repo-meta num">{archived.length || ""}</span></div>
              {archived.map(row)}
              {archived.length === 0 && <p className="muted archived-empty">Nothing archived yet. Archiving removes a workspace's folder and keeps its branch, so you can restore it here.</p>}
            </section>
          );
        })()}
      </div>
      <div className="sidebar-foot">
        <button className="me" onClick={p.onSky} title="Open your night sky">
          {p.me ? <Avatar src={p.me.avatar_url} size={28} label={p.me.login} /> : <Mascot size={28} />}
          <span className="me-text">
            <span className="me-name">{p.me?.name || p.me?.login || "Not signed in to GitHub"}</span>
            <span className="muted num">{p.me ? `@${p.me.login} · ` : ""}{p.lit}/{total} stars</span>
          </span>
        </button>
        {p.usage && p.usage.week > 0 && (
          <div className="usage num" title={`API-equivalent prices reported by the CLIs. On a Claude or ChatGPT plan you aren't billed per token.\n7 days by agent: ${Object.entries(p.usage.by_agent).map(([a, c]) => `${a} $${c.toFixed(2)}`).join(", ")}`}>
            Today ${p.usage.today.toFixed(2)} · 7 days ${p.usage.week.toFixed(2)}
          </div>
        )}
        <button className={`quiet ${p.showArchived ? "on" : ""}`} onClick={p.onToggleArchived} aria-pressed={p.showArchived}>
          {p.showArchived ? <ArchiveRestore size={16} /> : <Archive size={16} />}
          {p.showArchived ? "Hide archived" : "Show archived"}
        </button>
      </div>
      {ctx && (
        <div className="menu ctx-menu" role="menu" style={{ left: Math.min(ctx.x, window.innerWidth - 240), top: Math.min(ctx.y, window.innerHeight - 260) }} onMouseDown={(e) => e.stopPropagation()}>
          <button role="menuitem" onClick={() => act(() => api.setPinned(ctx.w.id, !ctx.w.pinned))}>{ctx.w.pinned ? <PinOff size={14} /> : <Pin size={14} />}<span>{ctx.w.pinned ? "Unpin" : "Pin to top"}</span></button>
          <button role="menuitem" onClick={() => setMoveOpen((o) => !o)} aria-expanded={moveOpen}><FolderInput size={14} /><span>Move to section</span><ChevronRight size={13} /></button>
          {moveOpen && (
            <div className="ctx-sub">
              {p.sections.map((sec) => (
                <button key={sec.id} role="menuitemradio" aria-checked={ctx.w.section_id === sec.id} onClick={() => act(() => api.setSection(ctx.w.id, sec.id))}>
                  {ctx.w.section_id === sec.id ? <Check size={14} /> : <span className="check-space" />}<span>{sec.name}</span>
                </button>
              ))}
              {ctx.w.section_id && <button role="menuitem" onClick={() => act(() => api.setSection(ctx.w.id, ""))}><span className="check-space" /><span>Remove from section</span></button>}
              <button role="menuitem" onClick={() => act(async () => { const name = prompt("Section name"); if (name?.trim()) { const sec = await api.createSection(name); await api.setSection(ctx.w.id, sec.id); } })}>
                <Plus size={14} /><span>Add section…</span>
              </button>
            </div>
          )}
          <button role="menuitem" onClick={() => act(() => api.setUnread(ctx.w.id, !ctx.w.unread))}>{ctx.w.unread ? <MailOpen size={14} /> : <Mail size={14} />}<span>{ctx.w.unread ? "Mark as read" : "Mark as unread"}</span></button>
          <button role="menuitem" onClick={() => { setCtx(null); p.onRename(ctx.w.id); }}><Pencil size={14} /><span>Rename</span></button>
          <div className="menu-sep" />
          <button role="menuitem" onClick={() => { setCtx(null); p.onArchive(ctx.w.id); }}><Archive size={14} /><span>Archive</span></button>
        </div>
      )}
    </aside>
  );
}
