import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { Archive, ArchiveRestore, FolderOpen, Pin, PinOff, Play } from "lucide-react";
import { api, constellationOf, errText, starFor, type GithubUser, type Repo, type Sky, type Star, type Workspace } from "./api";
import { Sidebar } from "./components/Sidebar";
import { Chat } from "./components/Chat";
import { Diff } from "./components/Diff";
import { Terminal } from "./components/Terminal";
import { PrBar } from "./components/PrBar";
import { RepoSettings } from "./components/RepoSettings";
import { NightSky } from "./components/NightSky";
import { NewWorkspace, type NewWorkspaceRequest } from "./components/NewWorkspace";
import { Mascot, type Mood } from "./components/Mascot";

const NAP_AFTER_MS = 5 * 60_000; // the cat naps after 5 quiet minutes

async function notify(title: string, body: string) {
  try {
    let ok = await isPermissionGranted();
    if (!ok) ok = (await requestPermission()) === "granted";
    if (ok) sendNotification({ title, body });
  } catch {
    /* not in Tauri (preview) */
  }
}
import "./App.css";

type Tab = "chat" | "changes" | "terminal" | "notes";
const TABS: { id: Tab; label: string }[] = [
  { id: "chat", label: "Chat" },
  { id: "changes", label: "Changes" },
  { id: "terminal", label: "Terminal" },
  { id: "notes", label: "Notes" },
];

export default function App() {
  const [repos, setRepos] = useState<Repo[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [runningSessions, setRunningSessions] = useState<Set<string>>(new Set());
  const [runningWs, setRunningWs] = useState<Set<string>>(new Set());
  const [lit, setLit] = useState<Set<string>>(new Set());
  const [shining, setShining] = useState<Set<string>>(new Set());
  const [sky, setSky] = useState<Sky>({ stars: [], constellations: [] });
  const [me, setMe] = useState<GithubUser>(null);
  // Deep link: ?ws=<id>&tab=<tab>&view=sky (used by workspace links and the preview page).
  const params = new URLSearchParams(window.location.search);
  const [selected, setSelected] = useState<string | null>(params.get("ws"));
  const [view, setView] = useState<"workspace" | "sky">(params.get("view") === "sky" ? "sky" : "workspace");
  const [tab, setTab] = useState<Tab>((params.get("tab") as Tab) || "chat");
  const [showArchived, setShowArchived] = useState(false);
  const [settingsFor, setSettingsFor] = useState<string | null>(null);
  const [toast, setToast] = useState<{ text: string; error?: boolean } | null>(null);
  const [discovery, setDiscovery] = useState<{ star: Star; kind: "lit" | "shining" } | null>(null);
  const [canImport, setCanImport] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [inject, setInject] = useState<{ text: string; nonce: number } | undefined>();
  const [run, setRun] = useState<{ cmd: string; nonce: number } | undefined>();
  const [notes, setNotes] = useState("");
  const [leaving, setLeaving] = useState<string | null>(null);
  const [sidebarW, setSidebarW] = useState(() => Number(localStorage.getItem("nekotrail.sidebar")) || 264);
  const [resizing, setResizing] = useState(false);
  const [newFor, setNewFor] = useState<string | null>(params.get("new")); // repo id the new-workspace panel opens on
  const shiningLoaded = useRef(false);
  const [purring, setPurring] = useState(false);
  const [lastActive, setLastActive] = useState(Date.now());
  const [worriedUntil, setWorriedUntil] = useState(0);
  const [now, setNow] = useState(Date.now());

  const refresh = useCallback(async () => {
    const s = await api.snapshot(showArchived);
    setRepos(s.repos);
    setWorkspaces(s.workspaces);
    setRunningSessions(new Set(s.running));
    setRunningWs(new Set(s.running_workspaces));
    setLit(new Set(s.lit));
    setShining((prev) => {
      const next = new Set(s.shining);
      // A PR merged since we last looked: that star starts to shine.
      const newly = [...next].filter((x) => !prev.has(x));
      if (prev.size + newly.length > 0 && newly.length === 1 && shiningLoaded.current) celebrate(newly[0]);
      shiningLoaded.current = true;
      return next;
    });
  }, [showArchived]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    api.conductorAvailable().then(setCanImport);
    api.sky().then(setSky);
    api.githubProfile().then(setMe);
  }, []);

  useEffect(() => {
    const un = listen<{ running: boolean; workspace_id?: string; ok?: boolean; title?: string; more_queued?: boolean }>("running", ({ payload }) => {
      refresh();
      setRefreshKey((k) => k + 1); // a turn ended: reload changes and PR state
      setLastActive(Date.now());
      if (payload.running || !payload.workspace_id || payload.more_queued) return;
      if (payload.ok === false) setWorriedUntil(Date.now() + 15_000);
      // The cat brings you something: tell the user when they're looking elsewhere.
      const away = document.hidden || !document.hasFocus() || selectedRef.current !== payload.workspace_id;
      if (away) {
        const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
        const name = (w && starFor(skyRef.current, w.name)?.name) || w?.name || "A workspace";
        notify(payload.ok === false ? `${name} needs you` : `${name} is done`, payload.title || "The agent finished its turn.");
      }
    });
    return () => { un.then((f) => f()); };
  }, [refresh]);

  // Refs so the event listener above always sees current state.
  const selectedRef = useRef(selected);
  const workspacesRef = useRef(workspaces);
  const skyRef = useRef(sky);
  selectedRef.current = selected;
  workspacesRef.current = workspaces;
  skyRef.current = sky;

  // ⌘+ / ⌘- / ⌘0 zoom the whole app, remembered between launches.
  useEffect(() => {
    let zoom = Number(localStorage.getItem("nekotrail.zoom")) || 1;
    const apply = () => {
      localStorage.setItem("nekotrail.zoom", String(zoom));
      try {
        getCurrentWebview().setZoom(zoom).catch(() => {});
      } catch {
        document.documentElement.style.zoom = String(zoom); // preview in a browser
      }
    };
    if (zoom !== 1) apply();
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const step = e.key === "=" || e.key === "+" ? 0.1 : e.key === "-" ? -0.1 : 0;
      if (step) zoom = Math.round(Math.min(2, Math.max(0.6, zoom + step)) * 10) / 10;
      else if (e.key === "0") zoom = 1;
      else return;
      e.preventDefault();
      apply();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Drag the sidebar's edge to resize it; double-click the edge to reset.
  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    setResizing(true);
    const startX = e.clientX;
    const startW = sidebarW;
    let w = startW;
    const move = (ev: PointerEvent) => {
      w = Math.min(520, Math.max(200, startW + ev.clientX - startX));
      document.documentElement.style.setProperty("--sidebar-w", `${w}px`); // no React render per pixel
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setResizing(false);
      setSidebarW(w);
      localStorage.setItem("nekotrail.sidebar", String(w));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  useEffect(() => {
    document.documentElement.style.setProperty("--sidebar-w", `${sidebarW}px`);
  }, [sidebarW]);

  // Dock badge: how many workspaces have news you haven't looked at.
  useEffect(() => {
    const n = workspaces.filter((w) => w.unread && w.state === "active").length;
    try {
      getCurrentWindow().setBadgeCount(n || undefined).catch(() => {});
    } catch {
      /* no native window (preview) */
    }
  }, [workspaces]);

  // Clock for the nap and worried moods.
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(t);
  }, []);
  useEffect(() => {
    if (worriedUntil > Date.now()) {
      const t = setTimeout(() => setNow(Date.now()), worriedUntil - Date.now() + 50);
      return () => clearTimeout(t);
    }
  }, [worriedUntil]);
  useEffect(() => {
    const wake = () => setLastActive(Date.now());
    window.addEventListener("keydown", wake);
    window.addEventListener("pointerdown", wake);
    return () => { window.removeEventListener("keydown", wake); window.removeEventListener("pointerdown", wake); };
  }, []);

  const mood: Mood =
    runningWs.size > 0 ? "working" : worriedUntil > now ? "worried" : now - lastActive > NAP_AFTER_MS ? "sleep" : "idle";

  function celebrate(slug: string) {
    const st = skyRef.current.stars.find((x) => x.slug === slug);
    if (!st) return;
    setDiscovery({ star: st, kind: "shining" });
    setTimeout(() => setDiscovery(null), 8000);
  }

  const pet = () => {
    setPurring(true);
    setTimeout(() => setPurring(false), 1000);
    const mine = sky.constellations.filter((c) => sky.stars.some((x) => x.constellation === c.slug && lit.has(x.slug)));
    const pool = mine.length ? mine : sky.constellations;
    const c = pool[Math.floor(Math.random() * pool.length)];
    if (c) say(`Purr. ${c.name}: ${c.story}`);
  };

  const ws = workspaces.find((w) => w.id === selected) ?? null;
  const star = ws ? starFor(sky, ws.name) : null;
  const starLine = (st: Star) => {
    const c = constellationOf(sky, st);
    const all = sky.stars.filter((x) => x.constellation === c.slug);
    return `${c.name} · ${all.filter((x) => lit.has(x.slug)).length} of ${all.length} stars lit`;
  };
  const repo = ws ? repos.find((r) => r.id === ws.repo_id) : null;

  useEffect(() => {
    if (!ws) return;
    setNotes(ws.notes);
    if (ws.unread) api.setUnread(ws.id, false).then(refresh);
  }, [ws?.id]);

  const say = (text: string, error = false) => {
    setToast({ text, error });
    setTimeout(() => setToast(null), error ? 8000 : 4000);
  };

  const select = (id: string) => {
    setSelected(id);
    setView("workspace");
  };

  const addRepo = async () => {
    const dir = await openDialog({ directory: true, title: "Choose a git repo" });
    if (typeof dir !== "string") return;
    try {
      await api.addRepo(dir);
      await refresh();
    } catch (e) {
      say(errText(e), true);
    }
  };

  const newWorkspace = async ({ repoId, prompt, agent, model, mode }: NewWorkspaceRequest) => {
    try {
      const w = await api.createWorkspace(repoId);
      if (prompt) {
        // Start the agent right away in the new worktree.
        const session = await api.newSession(w.id, agent);
        await api.updateSession(session.id, "New chat", model, mode);
        await api.send(session.id, prompt);
      }
      setNewFor(null);
      const isNew = !lit.has(w.name.replace(/-\d+$/, ""));
      await refresh();
      select(w.id);
      const found = starFor(sky, w.name);
      if (found && isNew) {
        setDiscovery({ star: found, kind: "lit" });
        setTimeout(() => setDiscovery(null), 7000);
      }
      const scripts = await api.workspaceScripts(w.id);
      if (scripts.setup) setRun({ cmd: scripts.setup, nonce: Date.now() }); // runs in the workspace terminal
      setTab(scripts.setup && !prompt ? "terminal" : "chat");
    } catch (e) {
      say(`Couldn't create the workspace. ${errText(e)}`, true);
    }
  };

  const archive = async () => {
    if (!ws) return;
    const name = star?.name ?? ws.name;
    try {
      await api.archiveWorkspace(ws.id);
    } catch (e) {
      if (errText(e) !== "DIRTY") return say(errText(e), true);
      if (!confirm(`${name} has uncommitted changes. Archive anyway? The branch ${ws.branch} is kept, but uncommitted work in the folder is lost.`)) return;
      try {
        await api.archiveWorkspace(ws.id, true);
      } catch (e2) {
        return say(errText(e2), true);
      }
    }
    setLeaving(ws.id);
    await new Promise((r) => setTimeout(r, 320));
    setLeaving(null);
    setSelected(null);
    await refresh();
    say(`Archived ${name}. Branch ${ws.branch} is kept.`);
  };

  const runScript = async () => {
    if (!ws) return;
    const s = await api.workspaceScripts(ws.id);
    if (!s.run) return setSettingsFor(ws.repo_id);
    setTab("terminal");
    setRun({ cmd: s.run, nonce: Date.now() });
  };

  const importConductor = async () => {
    try {
      const r = await api.importConductor();
      await refresh();
      say(`Imported ${r.repos} repos, ${r.workspaces} workspaces and ${r.sessions} chats${r.skipped ? `. Skipped ${r.skipped} already here or missing on disk` : ""}.`);
    } catch (e) {
      say(errText(e), true);
    }
  };

  // ⌘N new workspace, ⌘K search, ⌘1-4 tabs.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      if (e.key === "k") { e.preventDefault(); document.getElementById("workspace-search")?.focus(); }
      if (e.key === "n" && (ws || repos[0])) { e.preventDefault(); setNewFor(ws?.repo_id ?? repos[0].id); }
      if (e.key === "j") {
        // Next workspace with news, wrapping around.
        e.preventDefault();
        const list = workspaces.filter((w) => w.state === "active");
        const start = list.findIndex((w) => w.id === selected);
        const next = [...list.slice(start + 1), ...list.slice(0, start + 1)].find((w) => w.unread);
        if (next) select(next.id);
        else say("Nothing new. Every cat is asleep or still working.");
      }
      const i = Number(e.key) - 1;
      if (ws && i >= 0 && i < TABS.length) { e.preventDefault(); setTab(TABS[i].id); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ws, repos, lit, sky, workspaces, selected]);

  const settingsRepo = repos.find((r) => r.id === settingsFor);

  return (
    <div className={`app ${resizing ? "resizing" : ""}`}>
      <Sidebar
        repos={repos}
        workspaces={workspaces}
        sky={sky}
        lit={lit.size}
        me={me}
        running={runningWs}
        selected={view === "workspace" ? selected : null}
        showArchived={showArchived}
        onSelect={select}
        onNewWorkspace={setNewFor}
        onAddRepo={addRepo}
        onRepoSettings={setSettingsFor}
        onToggleArchived={() => setShowArchived((v) => !v)}
        onSky={() => setView("sky")}
        mood={mood}
        purring={purring}
        onPet={pet}
        leaving={leaving}
      />
      <div
        className="sidebar-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        title="Drag to resize, double-click to reset"
        onPointerDown={startResize}
        onDoubleClick={() => { setSidebarW(264); localStorage.removeItem("nekotrail.sidebar"); }}
      />
      <main className="main">
        {view === "sky" && <NightSky sky={sky} lit={lit} shining={shining} workspaces={workspaces} onOpen={select} />}

        {view === "workspace" && !ws && (
          <div className="welcome">
            <Mascot size={88} mood={mood === "working" ? "working" : "happy"} follow purring={purring} onClick={pet} title="Pet the cat" />
            <h1>{me?.name ? `Hi ${me.name.split(" ")[0]}.` : "Hi there."}</h1>
            <p className="lede">Run many coding agents at once. Each one gets its own branch and folder, and each one ends as a pull request.</p>
            {repos.length > 0 ? (
              <p className="muted">Pick a workspace on the left, press ⌘N to start a new one in {repos[0].name}, or ⌘J to jump to the next one with news.</p>
            ) : (
            <ol className="steps">
              <li><b className="n">1</b><span>Add a repo with the <b>+</b> at the top left.</span></li>
              <li><b className="n">2</b><span>Press <b>+</b> next to the repo, or ⌘N, to start a workspace. Each one lights a star in a cat constellation; merged PRs make it shine.</span></li>
              <li><b className="n">3</b><span>Tell Claude or Codex what to build, check the changes, open the PR.</span></li>
            </ol>
            )}
            {repos.length === 0 && (
              <button className="primary" onClick={addRepo}><FolderOpen size={16} />Add a repo</button>
            )}
            {canImport && !workspaces.some((w) => w.imported_from === "conductor") && (
              <div className="import-card" style={{ marginTop: 24 }}>
                <h2>Coming from Conductor?</h2>
                <p>Bring over your repos, active workspaces and chats. nekotrail only reads Conductor's data; nothing is moved or deleted.</p>
                <button className="accent" onClick={importConductor}>Import from Conductor</button>
              </div>
            )}
          </div>
        )}

        {view === "workspace" && ws && (
          <>
            <header className="ws-header">
              <div className="ws-title">
                <h1>
                  {star?.name ?? ws.name}
                  {ws.pinned && <Pin size={16} className="muted" aria-label="Pinned" />}
                </h1>
                <div className="ws-sub">
                  {repo?.name}
                  {star ? ` · ${starLine(star)}` : ws.imported_from ? " · imported from Conductor" : ""}
                </div>
              </div>
              <span className="spacer" />
              <div className="ws-actions">
                {ws.state === "archived" ? (
                  <button className="primary" onClick={() => api.restoreWorkspace(ws.id).then(refresh).catch((e) => say(errText(e), true))}>
                    <ArchiveRestore size={16} />Restore
                  </button>
                ) : (
                  <>
                    <button onClick={runScript} title="Run the repo's run script in the terminal"><Play size={14} />Run</button>
                    <select aria-label="Open workspace in" value="" onChange={(e) => e.target.value && api.openIn(ws.id, e.target.value as "finder").catch((er) => say(errText(er), true))}>
                      <option value="">Open in…</option>
                      <option value="cursor">Cursor</option>
                      <option value="vscode">VS Code</option>
                      <option value="finder">Finder</option>
                      <option value="terminal">Terminal</option>
                    </select>
                    <button className="icon" title={ws.pinned ? "Unpin" : "Pin to top"} aria-label={ws.pinned ? "Unpin" : "Pin"} onClick={() => api.setPinned(ws.id, !ws.pinned).then(refresh)}>
                      {ws.pinned ? <PinOff size={18} /> : <Pin size={18} />}
                    </button>
                    <button className="icon" title="Archive (keeps the branch)" aria-label="Archive" onClick={archive}><Archive size={18} /></button>
                  </>
                )}
              </div>
            </header>

            {ws.state === "active" && (
              <PrBar ws={ws} name={star?.name ?? ws.name} refreshKey={refreshKey} onArchive={archive} onAskAgent={(text) => { setTab("chat"); setInject({ text, nonce: Date.now() }); }} />
            )}

            {ws.state === "active" && (
              <nav className="tabs" role="tablist" aria-label="Workspace">
                {TABS.map((t, i) => (
                  <button key={t.id} role="tab" aria-selected={tab === t.id} className={`tab ${tab === t.id ? "active" : ""}`} onClick={() => setTab(t.id)} title={`⌘${i + 1}`}>
                    {t.label}
                  </button>
                ))}
              </nav>
            )}

            <div className="panel">
              {ws.state === "archived" ? (
                <div className="empty">
                  <Mascot size={88} mood="sleep" />
                  <h2>Archived</h2>
                  <p>Restore it to bring the folder back on branch {ws.branch}.</p>
                </div>
              ) : (
                <>
                  <div className="pane" hidden={tab !== "chat"}>
                    <Chat workspaceId={ws.id} running={runningSessions} onRunningChange={refresh} inject={inject} />
                  </div>
                  <div className="pane" hidden={tab !== "changes"}>
                    {tab === "changes" && <Diff workspaceId={ws.id} refreshKey={refreshKey} />}
                  </div>
                  <div className="pane" hidden={tab !== "terminal"}>
                    <Terminal key={ws.id} ptyId={`${ws.id}:term`} cwd={ws.path} run={run} />
                  </div>
                  <div className="pane" hidden={tab !== "notes"}>
                    <textarea
                      className="notes"
                      aria-label="Workspace notes"
                      placeholder="Plan, links, what's left. Saved when you click away."
                      value={notes}
                      onChange={(e) => setNotes(e.target.value)}
                      onBlur={() => api.setNotes(ws.id, notes).then(refresh)}
                    />
                  </div>
                </>
              )}
            </div>
          </>
        )}

      </main>

      {newFor && repos.length > 0 && (
        <NewWorkspace repos={repos} initialRepo={newFor} onCreate={newWorkspace} onClose={() => setNewFor(null)} />
      )}
      {settingsRepo && <RepoSettings repo={settingsRepo} onClose={() => { setSettingsFor(null); refresh(); }} />}
      {toast && <div role="status" className={`toast ${toast.error ? "error" : ""}`}>{toast.text}</div>}
      {discovery && (
        <div className="discovery" role="status">
          <Mascot size={56} mood="happy" />
          <div>
            <span className="origin">{constellationOf(sky, discovery.star).name}</span>
            <h3>{discovery.kind === "lit" ? `You lit ${discovery.star.name}` : `${discovery.star.name} shines`}</h3>
          </div>
          <p>
            {discovery.kind === "lit"
              ? "Merge this workspace's PR to make the star shine and join it to its neighbours."
              : "Its PR merged. Lines to neighbouring shipped stars are drawn on your night sky."}{" "}
            <span className="num">({starLine(discovery.star)})</span>
          </p>
        </div>
      )}
    </div>
  );
}
