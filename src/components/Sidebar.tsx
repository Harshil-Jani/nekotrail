import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useDismiss } from "../useDismiss";
import { MessageSquareText, ArrowLeft, ArrowRight, Eye, History, HardDrive, PanelLeft, GitBranch, GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, Archive, ArchiveRestore, Check, ChevronDown, ChevronRight, Ellipsis, FolderGit2, Gauge, FolderInput, FolderOpen, GitFork, Kanban, ListChecks, ListOrdered, Mail, MailOpen, Pencil, Pin, PinOff, Plus, Search, Settings, Settings2, Trash2, X } from "lucide-react";
import { getSettings, saveSettings } from "../settings";
import { IS_MAC, keyHint, mod } from "../platform";
import { api, displayName, isHistory, nameInText, starName, subName, statusOf, STATUS_LABEL, type Agent, type CheckDot, type Collision, type Section, type GithubUser, type Repo, type RepoGithub, type ReviewRequest, type Sky, type Workspace } from "../api";
import { ReviewRequests } from "./ReviewRequests";
import { Mascot, type Mood } from "./Mascot";
import { Avatar } from "./Avatar";
import "./Sidebar.css";
import { ask } from "./Confirm";

type Props = {
  repos: Repo[];
  /** Without the linked workspaces of multi-repo tasks: a task shows as its main workspace. */
  workspaces: Workspace[];
  /** Multi-repo tasks: main workspace id to the names of its repos, the main one first. */
  taskRepos?: Record<string, string[]>;
  sky: Sky;
  lit: number;
  me: GithubUser;
  running: Set<string>; // workspace ids with a running agent
  selected: string | null;
  showArchived: boolean;
  onSelect: (id: string) => void;
  onNewWorkspace: (repoId: string) => void;
  onAddRepo: () => void;
  /** Clone from GitHub, offered when there are no repos yet. */
  onClone?: () => void;
  /** The first run is showing, with its own ways to add code: the empty list only says where repos go. */
  firstRun?: boolean;
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
  /** Checks per workspace: running, failing or passing. */
  checks?: Record<string, CheckDot>;
  /** Collision radar: groups of workspaces changing the same files, in suggested merge order. */
  collisions: Collision[];
  usage: { today: number; week: number; by_agent: Record<string, number> } | null;
  sections: Section[];
  /** Reload workspaces and sections after a change made here. */
  onChanged: () => void;
  onRename: (id: string) => void;
  onArchive: (id: string) => void;
  onBoard: () => void;
  /** Search inside every chat (the ⌘⇧F sheet), starting from what is typed here. */
  onSearchChats?: (q: string) => void;
  boardOpen: boolean;
  onSettings: () => void;
  /** The task queue at a glance, for its row. */
  queue: { running: number; next: number; needs: number; paused: boolean };
  queueOpen: boolean;
  onQueue: () => void;
  usageOpen: boolean;
  onUsage: () => void;
  canBack: boolean;
  canForward: boolean;
  onBack: () => void;
  onForward: () => void;
  onToggleSidebar: () => void;
  /** Free disk space in MB when it is getting low; null when fine. */
  lowDiskMb: number | null;
  onStorage: () => void;
  /** Open PRs waiting for your review (from the PR poller). */
  reviewRequests: ReviewRequest[];
  /** "repo:number" of a review being set up. */
  reviewBusy: string | null;
  onReview: (r: ReviewRequest, agent?: Agent) => void;
};

const DRAG = "application/x-nekotrail-workspace";

/** The Queue row's tooltip: "2 running, 5 up next", "Paused", or what it is for. */
function queueLine(q: { running: number; next: number; paused: boolean }) {
  const parts = [q.running && `${q.running} running`, q.next && `${q.next} up next`].filter(Boolean).join(", ");
  if (q.paused) return parts ? `Paused · ${parts}` : "Paused";
  return parts || "Line up tasks for later";
}

/** The workspace's PR as the row's trailing mark: draft, open, merged or closed. Nothing without a PR. */
function PrMark({ state }: { state?: string }) {
  switch (state) {
    case "OPEN": return <GitPullRequest size={13} className="pr-open" aria-label="PR open" />;
    case "DRAFT": return <GitPullRequestDraft size={13} className="pr-draft" aria-label="Draft PR" />;
    case "MERGED": return <GitMerge size={13} className="pr-merged" aria-label="PR merged" />;
    case "CLOSED": return <GitPullRequestClosed size={13} className="pr-closed" aria-label="PR closed" />;
    default: return null;
  }
}
const PR_WORD: Record<string, string> = { OPEN: "PR open", DRAFT: "Draft PR", MERGED: "PR merged", CLOSED: "PR closed" };

/** The repo's row: its owner's avatar (cached, so it shows offline), its name, and + for a new workspace.
 * Click the name to fold it; right-click for its scripts. */
function RepoHead({ repo, onNew, onMenu, collapsed, onToggle, count }: { repo: Repo; onNew: () => void; onMenu: (e: React.MouseEvent) => void; collapsed: boolean; onToggle: () => void; count: number }) {
  const [gh, setGh] = useState<RepoGithub>(null);
  const [pic, setPic] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    api.repoAvatar(repo.id).then((u) => live && setPic(u)).catch(() => {});
    api.repoGithub(repo.id).then((g) => live && setGh(g)).catch(() => {});
    return () => { live = false; };
  }, [repo.id]);
  const stars = gh && gh.stargazerCount > 0 ? `, ${gh.stargazerCount} stars` : "";
  return (
    <div className="repo-head" data-tour="new" onContextMenu={onMenu}
      title={`${gh?.nameWithOwner ?? repo.name}${stars}${gh?.description ? `\n${gh.description}` : ""}\n${repo.root_path}`}>
      <button className="fold" tabIndex={-1} onClick={onToggle} aria-expanded={!collapsed} aria-label={collapsed ? `Expand ${repo.name}` : `Collapse ${repo.name}`}>
        <Avatar src={pic ?? gh?.owner_avatar ?? ""} size={20} label={repo.name} />
      </button>
      <span className="repo-name" onClick={onToggle}>{repo.name}</span>
      {/* The arrow folds the group too (it was only a picture, so clicks on it did nothing). */}
      <span className="chev-hit" onClick={onToggle} aria-hidden>
        {collapsed ? <ChevronRight size={12} className="chev" /> : <ChevronDown size={12} className="chev" />}
      </span>
      {collapsed && count > 0 && <span className="repo-meta num">{count}</span>}
      <button className="icon repo-new" tabIndex={-1} title={keyHint("Create workspace (⌘N)")} aria-label={`Create workspace in ${repo.name}`} onClick={onNew}><Plus size={15} /></button>
    </div>
  );
}

const CHECK_LABEL: Record<CheckDot, string> = { passed: "Checks pass", failed: "Checks fail", running: "Checks running" };

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  // Memory use, for the tooltip on your name: read when you point at it, not on a timer (it lists every process).
  const [ram, setRam] = useState<Awaited<ReturnType<typeof api.resourceUsage>> | null>(null);
  const ramAt = useRef(0);
  const loadRam = () => {
    if (Date.now() - ramAt.current < 5000) return;
    ramAt.current = Date.now();
    api.resourceUsage().then(setRam).catch(() => {});
  };
  // Project filter and folded sections, remembered between launches.
  const [filter, setFilterState] = useState<string[]>(() => getSettings().repoFilter);
  const [collapsed, setCollapsedState] = useState<string[]>(() => getSettings().collapsed);
  // The "Workspaces" ⋯ menu (grouping, repo filter, sections, add repo) and the repo row's right-click menu.
  const [listMenu, setListMenu] = useState(false);
  const listRef = useDismiss<HTMLSpanElement>(listMenu, () => setListMenu(false));
  const [repoCtx, setRepoCtx] = useState<{ r: Repo; x: number; y: number } | null>(null);
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
  const [dragging, setDragging] = useState(false);
  /** A section you just added stays up while empty, so you can see it worked and drag into it. */
  const [freshSec, setFreshSec] = useState<string | null>(null);
  // A section being named. `id` null is a new one; `move` puts that workspace into it once it exists.
  const [editing, setEditing] = useState<{ id: string | null; name: string; move?: string } | null>(null);
  // "Add sidebar section" from the palette opens the same inline field.
  useEffect(() => {
    const open = () => { if (p.groupBy !== "repo") p.onGroupBy("repo"); setEditing({ id: null, name: "" }); };
    window.addEventListener("nekotrail-new-section", open);
    return () => window.removeEventListener("nekotrail-new-section", open);
  }, [p.groupBy]);
  const [secMenu, setSecMenu] = useState<string | null>(null);
  const secRef = useDismiss<HTMLSpanElement>(secMenu !== null, () => setSecMenu(null));
  useEffect(() => {
    if (!repoCtx) return;
    const close = () => setRepoCtx(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    return () => { window.removeEventListener("mousedown", close); window.removeEventListener("blur", close); };
  }, [repoCtx]);
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
    else {
      const sec = await api.createSection(name);
      setFreshSec(sec.id);
      if (editing.move) await api.setSection(editing.move, sec.id);
    }
    p.onChanged();
  };
  const sectionHead = (id: string, name: string, count: number, icon: React.ReactNode, menu?: boolean) => {
    const folded = collapsed.includes(`sec:${id}`) && !q;
    return (
      <div className={`repo-head section-head ${over === id ? "drop" : ""}`} {...drop(id === "pinned" ? "pinned" : id)}>
        <button className="fold" tabIndex={-1} onClick={() => toggleCollapsed(`sec:${id}`)} aria-expanded={!folded} aria-label={folded ? `Expand ${name}` : `Collapse ${name}`}>
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
            <span className="git-menu" ref={secMenu === id ? secRef : undefined}>
              <button className="icon" tabIndex={-1} aria-label={`${name} options`} aria-expanded={secMenu === id} onClick={() => setSecMenu((m) => (m === id ? null : id))}><Ellipsis size={14} /></button>
              {secMenu === id && (
                <span className="menu sec-menu" role="menu">
                  <button role="menuitem" onClick={() => { setSecMenu(null); setEditing({ id, name }); }}><Pencil size={14} /><span>Rename section</span></button>
                  <button role="menuitem" onClick={async () => {
                    setSecMenu(null);
                    if (!(await ask({ title: `Delete the section ${name}?`, body: "Its workspaces move back under their repos. Nothing else changes.", ok: "Delete section", danger: true }))) return;
                    await api.deleteSection(id);
                    p.onChanged();
                  }}><Trash2 size={14} /><span>Delete section</span></button>
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
  const match = (w: Workspace) => !q || label(w).toLowerCase().includes(q) || starName(p.sky, w).toLowerCase().includes(q) || w.branch.toLowerCase().includes(q);
  const total = p.sky.stars.length;
  const repoName = (id: string) => p.repos.find((r) => r.id === id)?.name ?? "";
  /** Quiet note for a row that shares files with other workspaces: who, and the merge order. */
  const collisionNote = (w: Workspace) => {
    const g = p.collisions.find((c) => c.order.includes(w.id));
    if (!g) return null;
    const nameById = (id: string) => { const x = p.workspaces.find((y) => y.id === id); return x ? nameInText(p.sky, x) : "another workspace"; };
    const others = g.pairs.filter((x) => x.a === w.id || x.b === w.id).map((x) => nameById(x.a === w.id ? x.b : x.a));
    if (!others.length) return null;
    return `Changes the same files as ${others.join(", ")}.\nSuggested merge order: ${g.order.map(nameById).join(", then ")}.`;
  };
  const row = (w: Workspace) => {
    const l = p.lines[w.id];
    const on = p.selected === w.id;
    const clash = w.state === "active" ? collisionNote(w) : null;
    const ck = w.state === "active" ? p.checks?.[w.id] : undefined;
    const busy = p.running.has(w.id);
    const pr = p.prStates[w.id];
    const sub = subName(p.sky, w);
    const task = p.taskRepos?.[w.id];
    // One trailing mark, the most urgent: working, needs you, checks failing, else the PR.
    const trail = busy ? <span className="spinner" aria-label="Agent working" />
      : w.unread ? <span className="unread" role="img" aria-label="Needs you" />
      : ck === "failed" ? <span className="ws-check ck-failed" role="img" aria-label={CHECK_LABEL.failed} />
      : <PrMark state={pr} />;
    // Everything else is one hover away.
    const tip = [
      label(w),
      sub,
      `${task ? task.join(", ") : repoName(w.repo_id)} · ${w.branch}`,
      [busy && "Agent working", w.unread && !busy && "Needs you", pr && PR_WORD[pr], ck && CHECK_LABEL[ck], l && (l[0] || l[1]) ? `+${l[0]} -${l[1]}` : "", w.pinned && "Pinned"].filter(Boolean).join(" · "),
      clash ? `\n${clash}` : "",
    ].filter(Boolean).join("\n");
    return (
      <button
        key={w.id}
        role="option"
        aria-selected={on}
        tabIndex={-1}
        data-id={w.id}
        title={tip}
        className={`ws ${on ? "selected" : ""} ${w.unread && !busy ? "news" : ""} ${w.state === "archived" ? "archived" : ""} ${p.leaving === w.id ? "leaving" : ""}`}
        onClick={() => p.onSelect(w.id)}
        draggable={w.state === "active"}
        onDragStart={(e) => { e.dataTransfer.setData(DRAG, w.id); e.dataTransfer.effectAllowed = "move"; setDragging(true); }}
        onDragEnd={() => { setDragging(false); setOver(null); }}
        onContextMenu={(e) => { e.preventDefault(); setCtx({ w, x: e.clientX, y: e.clientY }); }}
      >
        <span className="ws-mark">
          {w.review_pr ? <Eye size={14} aria-label={`Reviewing PR #${w.review_pr}`} /> : isHistory(w) ? <History size={14} aria-label="Terminal history, read only" /> : task ? <FolderGit2 size={14} aria-label={`One task across ${task.length} repos`} /> : <GitBranch size={14} aria-hidden />}
        </span>
        <span className="ws-name">{label(w)}</span>
        <span className="ws-trail">{trail}</span>
        {w.state === "active" && !isHistory(w) && (
          // Archive without opening the workspace. The row is itself a button, so this is a span
          // with button semantics; ⌘⇧A does the same for the open workspace.
          <span className="ws-archive" role="button" tabIndex={-1} aria-label={`Archive ${label(w)}`} title={keyHint("Archive workspace (⌘⇧A)")}
            onClick={(e) => { e.stopPropagation(); p.onArchive(w.id); }}
            onPointerDown={(e) => e.stopPropagation()}>
            <Archive size={14} aria-hidden />
          </span>
        )}
      </button>
    );
  };

  // The list is one tab stop (roving tabindex). Arrows move between rows and group headers, Left and
  // Right fold a group, Return opens. The focused item is kept across renders; else the selected row.
  const list = useRef<HTMLDivElement>(null);
  const kb = useRef<HTMLElement | null>(null);
  const ITEMS = ".fold, .ws, .ws-empty";
  useLayoutEffect(() => {
    const items = Array.from(list.current?.querySelectorAll<HTMLElement>(ITEMS) ?? []);
    const want = (kb.current && items.includes(kb.current) ? kb.current : null)
      ?? items.find((x) => x.classList.contains("selected")) ?? items.find((x) => x.classList.contains("ws")) ?? items[0];
    for (const x of items) x.tabIndex = x === want ? 0 : -1;
  });
  const onListKey = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input, textarea, .menu")) return;
    const items = Array.from(list.current?.querySelectorAll<HTMLElement>(ITEMS) ?? []);
    const cur = (e.target as HTMLElement).closest<HTMLElement>(ITEMS);
    const i = cur ? items.indexOf(cur) : -1;
    const focus = (el?: HTMLElement | null) => {
      if (!el) return;
      e.preventDefault();
      kb.current = el;
      for (const x of items) x.tabIndex = x === el ? 0 : -1;
      el.focus();
      el.scrollIntoView({ block: "nearest" });
    };
    const foldOf = (el: HTMLElement | null) => el?.classList.contains("fold") ? el : el?.closest("section")?.querySelector<HTMLElement>(".fold") ?? null;
    if (e.key === "ArrowDown") return focus(items[i + 1]);
    if (e.key === "ArrowUp") return focus(items[Math.max(0, i - 1)]);
    if (e.key === "Home") return focus(items[0]);
    if (e.key === "End") return focus(items[items.length - 1]);
    if (e.key === "ArrowLeft") {
      const f = foldOf(cur);
      if (!f) return;
      if (f.getAttribute("aria-expanded") === "true") f.click();
      return focus(f);
    }
    if (e.key === "ArrowRight" && cur?.classList.contains("fold")) {
      if (cur.getAttribute("aria-expanded") === "false") { e.preventDefault(); cur.click(); return; }
      return focus(items[i + 1]);
    }
  };

  const repoFilterItems = (close: () => void) => (
    <>
      <button role="menuitemradio" aria-checked={!filter.length} onClick={() => { setFilter([]); close(); }}>
        {!filter.length ? <Check size={14} /> : <span className="check-space" />}<span>All repos</span>
      </button>
      {p.repos.map((r) => {
        const on = filter.includes(r.id);
        const n = p.workspaces.filter((w) => w.repo_id === r.id && w.state === "active").length;
        return (
          <button key={r.id} role="menuitemcheckbox" aria-checked={on}
            onClick={(e) => setFilter(mod(e) || e.shiftKey ? (on ? filter.filter((x) => x !== r.id) : [...filter, r.id]) : on && filter.length === 1 ? [] : [r.id])}>
            {on ? <Check size={14} /> : <span className="check-space" />}<span>{r.name}</span><span className="muted num">{n}</span>
          </button>
        );
      })}
    </>
  );
  const queueTip = `Queue: ${queueLine(p.queue)}${p.queue.needs ? `. ${p.queue.needs} need you.` : ""}`;
  const usageTip = p.usage && p.usage.today > 0 && getSettings().showCost ? `Usage: $${p.usage.today.toFixed(2)} today, API-equivalent` : "Usage: spend, caps and limits";
  const ramTip = ram ? `\nMemory: ${ram.total_mb >= 1024 ? `${(ram.total_mb / 1024).toFixed(2)} GB` : `${ram.total_mb} MB`} for NekoTrail, its agents, terminals and dev servers\n${ram.top.map((t) => `  ${t.name}  ${t.mb} MB`).join("\n")}` : "";

  return (
    <aside className="sidebar" data-tour="sidebar">
      <div className="sidebar-head" data-tauri-drag-region>
        <button className="icon sidebar-toggle" onClick={p.onToggleSidebar} title={keyHint("Hide sidebar (⌘B)")} aria-label="Hide sidebar"><PanelLeft size={16} /></button>
        <Mascot size={20} mood={p.mood} follow purring={p.purring} onClick={p.onPet} title="Pet the cat" />
        {p.mood === "sleep" && <span className="zzz" aria-hidden>z z</span>}
        <span className="spacer" />
        <button className="icon" onClick={p.onBack} disabled={!p.canBack} title={keyHint("Back (⌘[)")} aria-label="Back"><ArrowLeft size={16} /></button>
        <button className="icon" onClick={p.onForward} disabled={!p.canForward} title={keyHint("Forward (⌘])")} aria-label="Forward"><ArrowRight size={16} /></button>
      </div>
      <nav className="side-nav" aria-label="Go to">
        <label className="nav-row nav-search">
          <Search size={15} aria-hidden />
          <input placeholder="Search" value={query} onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") { setQuery(""); e.currentTarget.blur(); }
              if (e.key === "Enter" && query.trim() && p.onSearchChats) { e.preventDefault(); p.onSearchChats(query.trim()); }
            }}
            id="workspace-search" aria-label="Search workspaces, or press Enter to search inside every chat" title={keyHint("Type to filter workspaces. Press Enter to search every chat (⌘⇧F)")} />
        </label>
        {query.trim() && p.onSearchChats && (
          <button className="nav-row deep-search" onClick={() => p.onSearchChats!(query.trim())}>
            <MessageSquareText size={15} aria-hidden /><span className="deep-search-label">Search all chats</span><kbd>↵</kbd>
          </button>
        )}
        <button className={`nav-row${p.boardOpen ? " on" : ""}`} onClick={p.onBoard} title={keyHint("Board (⌘⇧K)")} aria-current={p.boardOpen ? "page" : undefined}><Kanban size={15} aria-hidden />Board</button>
        {p.repos.length > 0 && <>
          <button className={`nav-row${p.queueOpen ? " on" : ""}`} onClick={p.onQueue} title={queueTip} aria-current={p.queueOpen ? "page" : undefined}>
            {p.queue.running > 0 ? <span className="spinner" aria-label="Running" /> : <ListOrdered size={15} aria-hidden />}Queue
            {p.queue.needs > 0 && <span className="nav-count num" aria-label={`${p.queue.needs} need you`}>{p.queue.needs}</span>}
          </button>
          <button className={`nav-row${p.usageOpen ? " on" : ""}`} onClick={p.onUsage} title={usageTip} aria-current={p.usageOpen ? "page" : undefined}><Gauge size={15} aria-hidden />Usage</button>
        </>}
      </nav>
      {filter.length > 0 && (
        <div className="filter-chips">
          {filter.map((id) => {
            const r = p.repos.find((x) => x.id === id);
            return r ? <button key={id} className="chip" onClick={() => setFilter(filter.filter((x) => x !== id))} aria-label={`Stop filtering by ${r.name}`}>{r.name}<X size={11} /></button> : null;
          })}
          <button className="quiet chip-clear" onClick={() => setFilter([])}>Show all repos</button>
        </div>
      )}
      <div className="sidebar-scroll" role="listbox" aria-label="Workspaces" ref={list} onKeyDown={onListKey}
        onFocus={(e) => { const el = (e.target as HTMLElement).closest<HTMLElement>(ITEMS); if (el) kb.current = el; }}>
        {p.repos.length === 0 && (
          <div className="sidebar-empty">
            <p className="muted">{p.firstRun ? "Your repos and their workspaces show up here." : "No repos yet. Add one to start your first workspace."}</p>
            {!p.firstRun && <button className="ws-empty" onClick={p.onAddRepo}><FolderOpen size={13} />Add a local repo</button>}
            {!p.firstRun && p.onClone && <button className="ws-empty" onClick={p.onClone}><GitFork size={13} />Clone from GitHub</button>}
          </div>
        )}
        <ReviewRequests
          requests={p.reviewRequests.filter((r) => !filter.length || filter.includes(r.repo_id))}
          workspaces={p.workspaces} repos={p.repos} busy={p.reviewBusy} onReview={p.onReview} onOpen={p.onSelect}
        />
        {p.groupBy === "repo" && p.repos.length > 0 && (() => {
          const live = p.workspaces.filter((w) => w.state === "active" && inFilter(w) && match(w));
          const pinned = live.filter((w) => w.pinned);
          const folded = (id: string) => collapsed.includes(`sec:${id}`) && !q;
          return (
            <>
              {(pinned.length > 0 || over === "pinned") && (
                <section className="repo">
                  {sectionHead("pinned", "Pinned", pinned.length, null)}
                  {!folded("pinned") && pinned.map((w) => row(w))}
                </section>
              )}
              {p.sections.map((sec) => {
                const ws = live.filter((w) => w.section_id === sec.id && !w.pinned);
                // An empty section is noise; it shows while you drag (so you can drop into it), rename it, or just made it.
                if (ws.length === 0 && (q || !(dragging || freshSec === sec.id || editing?.id === sec.id))) return null;
                return (
                  <section key={sec.id} className="repo">
                    {sectionHead(sec.id, sec.name, ws.length, null, true)}
                    {!folded(sec.id) && ws.map((w) => row(w))}
                    {!folded(sec.id) && ws.length === 0 && dragging && <div className={`drop-hint ${over === sec.id ? "drop" : ""}`} {...drop(sec.id)}>Drop here</div>}
                  </section>
                );
              })}
              {editing?.id === null && (
                <div className="repo-head section-head">
                  <input className="section-edit" autoFocus placeholder="Section name" value={editing.name} onChange={(e) => setEditing({ id: null, name: e.target.value })}
                    onBlur={saveSection} onKeyDown={(e) => { if (e.key === "Enter") saveSection(); if (e.key === "Escape") setEditing(null); }} />
                </div>
              )}
            </>
          );
        })()}
        {p.repos.length > 0 && (
          <div className="side-label">
            <span>Workspaces</span>
            <span className="git-menu" ref={listRef}>
              <button className={`icon${filter.length || p.groupBy !== "repo" ? " on" : ""}`} tabIndex={-1} aria-label="Workspace list options" title="Group, filter, sections and repos" aria-expanded={listMenu} onClick={() => setListMenu((o) => !o)}><Ellipsis size={14} /></button>
              {listMenu && (
                <span className="menu list-menu" role="menu">
                  <div className="menu-label">Group by</div>
                  <button role="menuitemradio" aria-checked={p.groupBy === "repo"} onClick={() => { setListMenu(false); p.onGroupBy("repo"); }}>{p.groupBy === "repo" ? <Check size={14} /> : <span className="check-space" />}<span>Repo</span></button>
                  <button role="menuitemradio" aria-checked={p.groupBy === "status"} onClick={() => { setListMenu(false); p.onGroupBy("status"); }}>{p.groupBy === "status" ? <Check size={14} /> : <span className="check-space" />}<span>Status</span></button>
                  <div className="menu-label">Show</div>
                  {repoFilterItems(() => setListMenu(false))}
                  <p className="muted menu-hint">{IS_MAC ? "⌘-click" : "Ctrl-click"} to show more than one repo.</p>
                  <div className="menu-sep" />
                  <button role="menuitem" onClick={() => { setListMenu(false); if (p.groupBy !== "repo") p.onGroupBy("repo"); setEditing({ id: null, name: "" }); }}><Plus size={14} /><span>Add section</span></button>
                  <button role="menuitem" onClick={() => { setListMenu(false); p.onAddRepo(); }}><FolderOpen size={14} /><span>Add local repo</span><kbd>{keyHint("⌘⌥A")}</kbd></button>
                  {p.onClone && <button role="menuitem" onClick={() => { setListMenu(false); p.onClone!(); }}><GitFork size={14} /><span>Clone from GitHub</span></button>}
                  <button role="menuitemcheckbox" aria-checked={p.showArchived} onClick={() => { setListMenu(false); p.onToggleArchived(); }}>{p.showArchived ? <Check size={14} /> : <span className="check-space" />}<span>Show archived</span></button>
                </span>
              )}
            </span>
          </div>
        )}
        {p.groupBy === "repo"
          ? shownRepos.map((r) => {
              const ws = p.workspaces.filter((w) => w.repo_id === r.id && w.state === "active" && match(w) && !(w.pinned || (w.section_id && p.sections.some((s) => s.id === w.section_id))));
              const folded = collapsed.includes(r.id) && !q;
              return (
                <section key={r.id} className="repo repo-group" {...drop("")}>
                  <RepoHead repo={r} onNew={() => p.onNewWorkspace(r.id)} onMenu={(e) => { e.preventDefault(); setRepoCtx({ r, x: e.clientX, y: e.clientY }); }} collapsed={folded} onToggle={() => toggleCollapsed(r.id)} count={ws.length} />
                  {!folded && ws.map((w) => row(w))}
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
                  {ws.map((w) => row(w))}
                </section>
              );
            })}
        {p.showArchived && (() => {
          const archived = p.workspaces.filter((w) => w.state === "archived" && inFilter(w) && match(w));
          return (
            <section className="repo archived-group">
              <div className="repo-head status-head"><span className="repo-name">Archived</span><span className="repo-meta num">{archived.length || ""}</span></div>
              {archived.map((w) => row(w))}
              {archived.length === 0 && <p className="muted archived-empty">Nothing archived yet. Archiving removes a workspace's folder and keeps its branch, so you can restore it here.</p>}
            </section>
          );
        })()}
      </div>
      <div className="sidebar-foot">
        {p.lowDiskMb !== null && (
          <button className="low-disk" onClick={p.onStorage} title="Disk is almost full. Open Storage to clean build folders.">
            <HardDrive size={14} /><span className="muted">{p.lowDiskMb >= 1024 ? `${(p.lowDiskMb / 1024).toFixed(1)} GB` : `${p.lowDiskMb} MB`} free</span><span className="spacer" /><span className="clean-link">Clean up</span>
          </button>
        )}
        <div className="foot-row">
          <button className="me" onClick={p.onSky} onPointerEnter={loadRam} onFocus={loadRam} title={`${p.me ? `@${p.me.login} · ` : ""}${p.lit} of ${total} stars lit. Open your night sky.${ramTip}`}>
            {p.me ? <Avatar src={p.me.avatar_url} size={22} label={p.me.login} /> : <Mascot size={22} />}
            <span className="me-name">{p.me?.name || p.me?.login || "Not signed in to GitHub"}</span>
          </button>
          <button className={`icon${p.showArchived ? " on" : ""}`} onClick={p.onToggleArchived} aria-pressed={p.showArchived} title={p.showArchived ? "Hide archived" : "Show archived"} aria-label="Show archived">
            {p.showArchived ? <ArchiveRestore size={16} /> : <Archive size={16} />}
          </button>
          <button className="icon" onClick={p.onSettings} title={keyHint("Settings (⌘,)")} aria-label="Settings"><Settings size={16} /></button>
        </div>
      </div>
      {repoCtx && (
        <div className="menu ctx-menu" role="menu" style={{ left: Math.min(repoCtx.x, window.innerWidth - 240), top: Math.min(repoCtx.y, window.innerHeight - 160) }} onMouseDown={(e) => e.stopPropagation()}>
          <button role="menuitem" onClick={() => { setRepoCtx(null); p.onNewWorkspace(repoCtx.r.id); }}><Plus size={14} /><span>Create workspace</span><kbd>{keyHint("⌘N")}</kbd></button>
          <button role="menuitem" onClick={() => { setRepoCtx(null); p.onRepoSettings(repoCtx.r.id); }}><Settings2 size={14} /><span>Setup and run scripts</span></button>
          <button role="menuitem" onClick={() => { setRepoCtx(null); setFilter(filter.length === 1 && filter[0] === repoCtx.r.id ? [] : [repoCtx.r.id]); }}><ListChecks size={14} /><span>{filter.length === 1 && filter[0] === repoCtx.r.id ? "Show all repos" : `Show only ${repoCtx.r.name}`}</span></button>
        </div>
      )}
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
              <button role="menuitem" onClick={() => { const move = ctx.w.id; setCtx(null); setMoveOpen(false); if (p.groupBy !== "repo") p.onGroupBy("repo"); setEditing({ id: null, name: "", move }); }}>
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
