import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { Archive, ArchiveRestore, Check, Columns2, Crown, Kanban, PanelLeft, X, Ellipsis, ExternalLink, Pencil, Square, Code2, Download, FilePlus2, FolderOpen, GitPullRequest, MessageSquare, Moon, NotebookPen, Pin, PinOff, Play, Plus, Settings as Gear, SquareTerminal, Sparkles } from "lucide-react";
import { api, constellationOf, displayName, errText, starFor, STATUS_LABEL, type Agent, type ChatEvent, type GithubUser, type Repo, type Section, type Sky, type Star, type Workspace, type WsStatus } from "./api";
import { Sidebar } from "./components/Sidebar";
import { Chat } from "./components/Chat";
import { Diff } from "./components/Diff";
import { TerminalPanel } from "./components/Terminal";
import { openUrl } from "@tauri-apps/plugin-opener";
import { PrBar } from "./components/PrBar";
import { RepoSettings } from "./components/RepoSettings";
import { NightSky } from "./components/NightSky";
import { NewWorkspace, type NewWorkspaceRequest } from "./components/NewWorkspace";
import { Mascot, type Mood } from "./components/Mascot";
import { Palette, type Command } from "./components/Palette";
import { CloneSheet, Welcome } from "./components/Welcome";
import { Compare } from "./components/Compare";
import { Board } from "./components/Board";
import { Scorecard } from "./components/Scorecard";
import { Storage } from "./components/Storage";
import { Settings } from "./components/Settings";
import { chime, getSettings, saveSettings, ting } from "./settings";
import { UserAttentionType } from "@tauri-apps/api/window";

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
const TAB_KEYS: Record<Tab, string> = { chat: "⌘L", changes: "⌘⇧D", terminal: "⌘J", notes: "" };
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
  const [view, setView] = useState<"workspace" | "sky" | "board">((params.get("view") as "sky" | "board") || "workspace");
  const [sections, setSections] = useState<Section[]>([]);
  const [autopilot, setAutopilotMap] = useState<Record<string, number>>({});
  useEffect(() => {
    const load = () => api.autopilotStatus().then(setAutopilotMap).catch(() => {});
    load();
    // Autopilot finished or needs you: same alerts as an agent that needs you.
    const un = listen<{ workspace_id: string; state: string; detail?: string }>("autopilot", ({ payload }) => {
      load();
      const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
      const name = w ? displayName(skyRef.current, w) : "A workspace";
      if (payload.state === "ready" || payload.state === "gave_up") {
        const prefs = getSettings();
        const title = payload.state === "ready" ? `${name} is ready to merge` : `Autopilot needs you on ${name}`;
        if (prefs.needsSound) ting();
        if (prefs.needsNotify) notify(title, payload.detail ?? "");
        say(`${title}. ${payload.detail ?? ""}`);
      }
    });
    return () => { un.then((f) => f()); };
  }, []);
  const [ram, setRam] = useState<Awaited<ReturnType<typeof api.resourceUsage>> | null>(null);
  useEffect(() => {
    const load = () => { if (!document.hidden) api.resourceUsage().then(setRam).catch(() => {}); };
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);
  const [tab, setTab] = useState<Tab>((params.get("tab") as Tab) || "chat");
  const navRef = useRef<(d: -1 | 1) => void>(() => {});
  // Back / forward through places you've been (workspace, view, tab), like a browser. ⌘[ and ⌘].
  type Place = { view: "workspace" | "sky" | "board"; selected: string | null; tab: Tab };
  const history = useRef<{ stack: Place[]; at: number; moving: boolean }>({ stack: [], at: -1, moving: false });
  const [, setNavTick] = useState(0);
  const [showArchived, setShowArchived] = useState(false);
  const [settingsFor, setSettingsFor] = useState<string | null>(null);
  const [toast, setToast] = useState<{ text: string; error?: boolean; action?: { label: string; run: () => void } } | null>(null);
  const [discovery, setDiscovery] = useState<{ star: Star; kind: "lit" | "shining" } | null>(null);
  const [canImport, setCanImport] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [inject, setInject] = useState<{ text: string; nonce: number } | undefined>();
  // Run and setup scripts that are alive (pty ids), the terminal tab to show, and dev-server URLs.
  const [livePtys, setLivePtys] = useState<Set<string>>(new Set());
  const [termFocus, setTermFocus] = useState<{ tab: string; nonce: number } | undefined>();
  const [localUrls, setLocalUrls] = useState<Record<string, string>>({});
  useEffect(() => {
    const unExit = listen<{ id: string }>("pty-exit", ({ payload }) =>
      setLivePtys((s) => { const n = new Set(s); n.delete(payload.id); return n; }));
    // Spot "http://localhost:3000" in run output, so ⌘⇧B can open it.
    const unData = listen<{ id: string; data: string }>("pty", ({ payload }) => {
      if (!payload.id.endsWith(":run")) return;
      const m = /https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):\d+[^\s"'\x1b]*/.exec(payload.data.replace(/\x1b\[[0-9;]*m/g, ""));
      if (m) setLocalUrls((u) => ({ ...u, [payload.id.split(":")[0]]: m[0].replace("0.0.0.0", "localhost") }));
    });
    return () => { unExit.then((f) => f()); unData.then((f) => f()); };
  }, []);
  /** Resolves when a terminal process ends (or after 20 minutes, whichever is first). */
  const waitForExit = (id: string) => new Promise<void>((resolve) => {
    const t = setTimeout(() => resolve(), 20 * 60_000);
    const un = listen<{ id: string }>("pty-exit", ({ payload }) => {
      if (payload.id === id) { clearTimeout(t); un.then((f) => f()); resolve(); }
    });
  });
  // j / k move through the sidebar when you're not typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || (e.key !== "j" && e.key !== "k")) return;
      if ((e.target as HTMLElement).closest("input, textarea, select, [contenteditable], .xterm")) return;
      const rows = Array.from(document.querySelectorAll<HTMLButtonElement>(".sidebar .ws"));
      if (!rows.length) return;
      e.preventDefault();
      const i = rows.findIndex((r) => r.classList.contains("selected"));
      rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === "j" ? 1 : -1)))].click();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Slash commands that change the view: /run, /diff, /terminal.
  const actionRef = useRef<(a: string) => void>(() => {});
  actionRef.current = (a: string) => {
    if (a === "run") runScript();
    else if (a === "diff") setTab("changes");
    else if (a === "terminal") setTab("terminal");
  };
  useEffect(() => {
    const on = (e: Event) => actionRef.current((e as CustomEvent<string>).detail);
    window.addEventListener("nekotrail-action", on);
    return () => window.removeEventListener("nekotrail-action", on);
  }, []);

  // Split an approved plan: one new workspace (and agent) per step, all in the same repo, in parallel.
  const fanOutRef = useRef<(d: { workspaceId: string; agent: Agent; model: string | null; effort: string | null; plan: string; steps: string[] }) => void>(() => {});
  fanOutRef.current = async ({ workspaceId, agent, model, effort, plan, steps }) => {
    const from = workspaces.find((w) => w.id === workspaceId);
    if (!from) return;
    say(`Starting ${steps.length} workspaces, one per step.`);
    for (const [i, step] of steps.entries()) {
      try {
        const w = await api.createWorkspace(from.repo_id, from.target_branch);
        await api.setWorkspaceTitle(w.id, `${i + 1}. ${step.replace(/[`*_]/g, "").slice(0, 60)}`);
        const s = await api.newSession(w.id, agent);
        await api.updateSession(s.id, "New chat", model, "acceptEdits");
        if (effort) await api.setSessionEffort(s.id, effort);
        // The first line names the branch, so it is the step itself.
        await api.send(s.id, `${step.replace(/[`*_]/g, "")}\n\nThis is step ${i + 1} of ${steps.length} of a plan split across parallel workspaces. Do only this step; other agents are doing the others in their own branches.\n\nThe whole plan, for context:\n${plan}\n\nWhen your step is done and tested, commit it.`);
      } catch (e) {
        say(`Couldn't start step ${i + 1}: ${errText(e)}`, true);
      }
    }
    await refresh();
  };
  useEffect(() => {
    const on = (e: Event) => fanOutRef.current((e as CustomEvent).detail);
    window.addEventListener("nekotrail-fanout", on);
    return () => window.removeEventListener("nekotrail-fanout", on);
  }, []);

  // "Run in terminal" requests switch to the Terminal tab; the panel types the command.
  useEffect(() => {
    const show = () => setTab("terminal");
    window.addEventListener("nekotrail-terminal", show);
    return () => window.removeEventListener("nekotrail-terminal", show);
  }, []);
  const startPty = async (id: string, cwd: string, cmd: string, tabName: string) => {
    await api.ptyRun(id, cwd, cmd);
    setLivePtys((s) => new Set(s).add(id));
    setTermFocus({ tab: tabName, nonce: Date.now() });
  };
  const [notes, setNotes] = useState("");
  const [leaving, setLeaving] = useState<string | null>(null);
  const [sidebarW, setSidebarW] = useState(() => Math.max(244, Number(localStorage.getItem("nekotrail.sidebar")) || 264));
  const [resizing, setResizing] = useState(false);
  const [newFor, setNewFor] = useState<string | null>(params.get("new"));
  const [newSource, setNewSource] = useState<"task" | "pr" | "branch" | "issue">((params.get("from") as "pr") || "task"); // repo id the new-workspace panel opens on
  const shiningLoaded = useRef(false);
  const [purring, setPurring] = useState(false);
  const [lastActive, setLastActive] = useState(Date.now());
  const [worriedUntil, setWorriedUntil] = useState(0);
  const [now, setNow] = useState(Date.now());
  const [costs, setCosts] = useState<Record<string, number>>({});
  navRef.current = (d) => goHistory(d);
  useEffect(() => {
    const h = history.current;
    const here: Place = { view, selected, tab };
    if (h.moving) { h.moving = false; return; }
    const cur = h.stack[h.at];
    if (cur && cur.view === here.view && cur.selected === here.selected && cur.tab === here.tab) return;
    h.stack = [...h.stack.slice(0, h.at + 1), here].slice(-50);
    h.at = h.stack.length - 1;
    setNavTick((t) => t + 1);
  }, [view, selected, tab]);
  const goHistory = (d: -1 | 1) => {
    const h = history.current;
    const next = h.stack[h.at + d];
    if (!next) return;
    h.at += d;
    h.moving = true;
    setView(next.view);
    setSelected(next.selected);
    setTab(next.tab);
    setNavTick((t) => t + 1);
  };
  // ⌘B or the sidebar button next to the traffic lights; remembered between launches.
  const [sidebarHidden, setSidebarHiddenState] = useState(() => localStorage.getItem("nekotrail.sidebarHidden") === "1");
  const setSidebarHidden = (f: (h: boolean) => boolean) =>
    setSidebarHiddenState((h) => { const n = f(h); localStorage.setItem("nekotrail.sidebarHidden", n ? "1" : "0"); return n; });
  const [groupBy, setGroupBy] = useState<"repo" | "status">(() => getSettings().groupBy);
  const [prStates, setPrStates] = useState<Record<string, string>>({});
  const [lines, setLines] = useState<Record<string, [number, number]>>({});
  const [openTargets, setOpenTargets] = useState<{ id: string; name: string }[]>([]);
  useEffect(() => { api.openInTargets().then(setOpenTargets).catch(() => {}); }, []);
  // Sidebar facts that cost a git or gh call per workspace: refreshed after turns and every 5 minutes.
  const [usage, setUsage] = useState<Awaited<ReturnType<typeof api.usageSummary>> | null>(null);
  useEffect(() => {
    api.lineCounts().then(setLines).catch(() => {});
    api.usageSummary().then(setUsage).catch(() => {});
  }, [refreshKey, workspaces.length]);
  useEffect(() => {
    const load = () => api.prStates().then(setPrStates).catch(() => {});
    load();
    const t = setInterval(load, 5 * 60_000);
    return () => clearInterval(t);
  }, [workspaces.length]);
  const [quickOpen, setQuickOpen] = useState<string[] | null>(null);
  const [focusFile, setFocusFile] = useState<{ path: string; nonce: number } | undefined>();
  const [editingTitle, setEditingTitle] = useState<string | null>(null);
  const [cloning, setCloning] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [scorecard, setScorecard] = useState(params.has("scorecard"));
  const [storage, setStorage] = useState(params.has("storage"));
  // Warn when the disk runs low: agents and builds fail in confusing ways on a full disk.
  const [freeMb, setFreeMb] = useState<number | null>(null);
  useEffect(() => {
    const load = () => api.freeDiskMb().then(setFreeMb).catch(() => {});
    load();
    const t = setInterval(load, 120_000);
    return () => clearInterval(t);
  }, []);
  const [comparing, setComparing] = useState<string | null>(params.get("compare")); // pair id
  const [palette, setPalette] = useState(params.has("palette"));
  const [settingsOpen, setSettingsOpen] = useState(params.has("settings"));

  const refresh = useCallback(async () => {
    const s = await api.snapshot(showArchived);
    setRepos(s.repos);
    setWorkspaces(s.workspaces);
    setCosts(s.costs ?? {});
    api.sections().then(setSections).catch(() => {});
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
    const un = listen<{ running: boolean; workspace_id?: string; ok?: boolean; title?: string; more_queued?: boolean; needs?: string | null }>("running", ({ payload }) => {
      refresh();
      setRefreshKey((k) => k + 1); // a turn ended: reload changes and PR state
      setLastActive(Date.now());
      if (payload.running || !payload.workspace_id || payload.more_queued) return;
      if (payload.ok === false) setWorriedUntil(Date.now() + 15_000);
      const prefs = getSettings();
      // Human in the loop: the agent stopped because it needs you. Ting ting, a notification, a Dock bounce.
      if (payload.needs) {
        const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
        const name = w ? displayName(skyRef.current, w) : "A workspace";
        const why: Record<string, string> = {
          plan: "has a plan for you to approve",
          question: "has a question for you",
          login: "needs you to sign in",
          limit: "hit its usage limit",
          install: "can't find its agent CLI",
          error: "stopped with an error",
          approval: "wants your OK",
        };
        if (prefs.needsSound) ting();
        if (prefs.needsNotify) notify(`${name} ${why[payload.needs] ?? "needs you"}`, payload.title || "Open nekotrail to continue.");
        if (prefs.needsBounce && (document.hidden || !document.hasFocus())) {
          try { getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(() => {}); } catch { /* preview */ }
        }
        return;
      }
      if (prefs.sound) chime(payload.ok !== false);
      // The cat brings you something: tell the user when they're looking elsewhere.
      const away = document.hidden || !document.hasFocus() || selectedRef.current !== payload.workspace_id;
      if (prefs.notify === "always" || (prefs.notify === "away" && away)) {
        const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
        const name = w ? displayName(skyRef.current, w) : "A workspace";
        notify(payload.ok === false ? `${name} needs you` : `${name} is done`, payload.title || "The agent finished its turn.");
      }
    });
    // Mid-turn requests (approve a tool call) arrive while the turn is still running.
    const unNeeds = listen<{ workspace_id: string; needs: string; title?: string }>("needs", ({ payload }) => {
      const prefs = getSettings();
      const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
      const name = w ? displayName(skyRef.current, w) : "A workspace";
      refresh();
      if (prefs.needsSound) ting();
      const away = document.hidden || !document.hasFocus() || selectedRef.current !== payload.workspace_id;
      if (prefs.needsNotify && away) notify(`${name} wants your OK`, payload.title || "Open nekotrail to allow or deny.");
      if (prefs.needsBounce && (document.hidden || !document.hasFocus())) {
        try { getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(() => {}); } catch { /* preview */ }
      }
    });
    return () => { un.then((f) => f()); unNeeds.then((f) => f()); };
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
      w = Math.min(520, Math.max(244, startW + ev.clientX - startX));
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
    if (c) say(`${c.name}: ${c.story}`);
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

  const say = (text: string, error = false, action?: { label: string; run: () => void }) => {
    setToast({ text, error, action });
    setTimeout(() => setToast((t) => (t?.text === text ? null : t)), error || action ? 8000 : 4000);
  };

  // "While you were away": opening a workspace with news shows one line on what happened.
  const [away, setAway] = useState<{ id: string; text: string } | null>(null);
  const summarize = async (w: Workspace) => {
    const sessions = await api.sessions(w.id).catch(() => []);
    let last: { agent: string; ms?: number; cost?: number; error?: boolean } | null = null;
    for (const s of sessions) {
      const { events } = await api.sessionEventsTail(s.id, 40).catch(() => ({ events: [] as ChatEvent[], total: 0 }));
      const done = [...events].reverse().find((e) => e.kind === "done") as Extract<ChatEvent, { kind: "done" }> | undefined;
      if (done && !(done as { imported?: boolean }).imported) last = { agent: s.agent, ms: done.duration_ms, cost: done.cost_usd, error: done.is_error };
    }
    const parts: string[] = [];
    if (last) {
      const who = last.agent === "codex" ? "Codex" : "Claude";
      const t = last.ms ? (last.ms >= 60_000 ? `${Math.floor(last.ms / 60_000)}m ${Math.round((last.ms % 60_000) / 1000)}s` : `${Math.round(last.ms / 1000)}s`) : "";
      parts.push(`${who} ${last.error ? "stopped with an error" : "finished"}${t ? ` in ${t}` : ""}${last.cost ? ` for $${last.cost.toFixed(2)}` : ""}`);
    }
    const l = lines[w.id];
    if (l && (l[0] || l[1])) parts.push(`+${l[0]} -${l[1]} lines changed`);
    const pr = prStates[w.id];
    if (pr) parts.push(pr === "OPEN" ? "PR open" : pr === "DRAFT" ? "draft PR" : pr === "MERGED" ? "PR merged" : "PR closed");
    if (parts.length) setAway({ id: w.id, text: parts.join(" · ") });
  };
  const select = (id: string) => {
    const w = workspaces.find((x) => x.id === id);
    if (w?.unread && w.id !== selected) summarize(w); else setAway(null);
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

  const newWorkspace = async (req: NewWorkspaceRequest) => {
    const { repoId, prompt, agent, model, effort, mode, from } = req;
    try {
      // A bake-off makes one workspace per agent, linked so they can be compared.
      const agents: Agent[] = agent === "both" ? ["claude", "codex"] : [agent];
      const made: Workspace[] = [];
      for (let i = 0; i < agents.length; i++) made.push(await api.createWorkspace(repoId, undefined, undefined, from));
      if (made.length > 1) {
        await api.setPair(made.map((w) => w.id));
        if (prompt) {
          const base = prompt.split("\n")[0].slice(0, 40);
          await Promise.all(made.map((w, i) => api.setWorkspaceTitle(w.id, `${base} (${agents[i] === "claude" ? "Claude" : "Codex"})`)));
        }
      }
      const sessions = prompt
        ? await Promise.all(made.map(async (w, i) => {
            const s = await api.newSession(w.id, agents[i]);
            await api.updateSession(s.id, "New chat", agents[i] === agent ? model : null, mode);
            if (effort && agents[i] === agent) await api.setSessionEffort(s.id, effort);
            return s;
          }))
        : [];
      setNewFor(null);
      await refresh();
      const w = made[0];
      select(w.id);
      // Celebrate the first star of each constellation only, so it stays special.
      const found = starFor(sky, w.name);
      if (found && !sky.stars.some((x) => x.constellation === found.constellation && lit.has(x.slug))) {
        setDiscovery({ star: found, kind: "lit" });
        setTimeout(() => setDiscovery(null), 7000);
      }
      if (made.length > 1) say("Claude and Codex are both on it, each in its own workspace. Press Compare when they finish.");
      const scripts = await api.workspaceScripts(w.id);
      await Promise.all(made.map(async (mw, i) => {
        if (scripts.setup) {
          // Setup (installs, .env) first, so the agent's first test run has what it needs.
          const done = waitForExit(`${mw.id}:setup`);
          await startPty(`${mw.id}:setup`, mw.path, scripts.setup, "setup");
          if (sessions[i]) {
            if (i === 0) { setTab("chat"); if (made.length === 1) say("Setup is running in the Terminal tab. The agent starts as soon as it finishes."); }
            await done;
          } else setTab("terminal");
        } else if (i === 0) setTab("chat");
        if (sessions[i]) await api.send(sessions[i].id, prompt);
      }));
    } catch (e) {
      say(`Couldn't create the workspace. ${errText(e)}`, true);
    }
  };

  const archive = async (opts?: { auto?: boolean; id?: string }) => {
    const target = opts?.id ? workspaces.find((w) => w.id === opts.id) : ws;
    if (!target) return;
    const name = displayName(sky, target);
    try {
      await api.archiveWorkspace(target.id);
    } catch (e) {
      if (errText(e) !== "DIRTY") return say(errText(e), true);
      if (opts?.auto) return say(`${name}'s PR merged, but it has uncommitted changes, so it stays open. Commit or discard them, then archive it.`);
      if (!confirm(`${name} has uncommitted changes. Archive anyway? The branch ${target.branch} is kept, but uncommitted work in the folder is lost.`)) return;
      try {
        await api.archiveWorkspace(target.id, true);
      } catch (e2) {
        return say(errText(e2), true);
      }
    }
    setLeaving(target.id);
    await new Promise((r) => setTimeout(r, 320));
    setLeaving(null);
    if (selected === target.id) setSelected(null);
    await refresh();
    say(`Archived ${name}. Branch ${target.branch} is kept.`, false, {
      label: "Undo",
      run: () => api.restoreWorkspace(target.id).then(refresh).then(() => select(target.id)).catch((e) => say(errText(e), true)),
    });
  };

  // ⌘R: start the run script, or stop it if it is running.
  const runScript = async () => {
    if (!ws) return;
    const id = `${ws.id}:run`;
    if (livePtys.has(id)) {
      await api.ptyClose(id);
      setLivePtys((s) => { const n = new Set(s); n.delete(id); return n; });
      return;
    }
    const s = await api.workspaceScripts(ws.id);
    if (!s.run) return setSettingsFor(ws.repo_id);
    setTab("terminal");
    await startPty(id, ws.path, s.run, "run");
  };
  const rerunSetup = async () => {
    if (!ws) return;
    const s = await api.workspaceScripts(ws.id);
    if (!s.setup) return setSettingsFor(ws.repo_id);
    await startPty(`${ws.id}:setup`, ws.path, s.setup, "setup");
  };
  const openLocal = () => {
    if (!ws) return;
    openUrl(localUrls[ws.id] ?? "http://localhost:3000").catch((e) => say(errText(e), true));
  };

  const importConductor = async () => {
    try {
      const r = await api.importConductor();
      await refresh();
      say(`Imported ${r.repos} repos, ${r.workspaces} workspaces and ${r.sessions} chats (${r.chats_with_history} with history)${r.skipped ? `. Skipped ${r.skipped} already here or missing on disk` : ""}.`);
    } catch (e) {
      say(errText(e), true);
    }
  };

  // ⌘N: the current workspace's repo, else the filtered project, else the first repo.
  const openNew = (source: "task" | "pr" | "branch" | "issue" = "task") => {
    const f = getSettings().repoFilter;
    const repoId = ws?.repo_id ?? (f.length === 1 ? f[0] : repos[0]?.id);
    if (!repoId) return addRepo();
    setNewSource(source);
    setNewFor(repoId);
  };

  const nextUnread = () => {
    const list = workspaces.filter((w) => w.state === "active");
    const start = list.findIndex((w) => w.id === selected);
    const next = [...list.slice(start + 1), ...list.slice(0, start + 1)].find((w) => w.unread);
    if (next) select(next.id);
    else say("No workspaces with news. Every agent is idle or still working.");
  };
  const openInEditor = (app = getSettings().editor) => {
    if (ws) api.openIn(ws.id, app).catch((er) => say(errText(er), true));
  };

  // Versions and counts only: no paths, chats, tokens or repo names.
  const copyDiagnostics = async () => {
    const d = await api.doctor().catch(() => null);
    const { getVersion } = await import("@tauri-apps/api/app");
    const v = await getVersion().catch(() => "unknown");
    const text = [
      `nekotrail ${v}`,
      `macOS: ${navigator.userAgent.match(/Mac OS X [\d_]+/)?.[0]?.replace(/_/g, ".") ?? "unknown"}`,
      `git: ${d?.git.version || "missing"}`,
      `gh: ${d?.gh.installed ? (d.gh.ok ? "signed in" : "not signed in") : "missing"}`,
      `claude: ${d?.claude.version || "missing"}`,
      `codex: ${d?.codex.version || "missing"}`,
      `repos: ${repos.length}, workspaces: ${workspaces.filter((w) => w.state === "active").length} active, ${workspaces.filter((w) => w.state === "archived").length} archived`,
      `memory: ${ram ? `${ram.total_mb} MB` : "unknown"}`,
    ].join("\n");
    await navigator.clipboard.writeText(text);
    say("Copied diagnostics. Paste them into your GitHub issue.");
  };

  // Everything the palette can do. Built on open, so it always reflects the current state.
  const commands = (): Command[] => {
    const c: Command[] = [];
    const nameOf = (w: Workspace) => displayName(sky, w);
    if (ws && ws.state === "active") {
      const here = nameOf(ws);
      c.push(
        { id: "tab-chat", group: here, label: "Show chat", hint: "⌘L", icon: <MessageSquare size={15} />, run: () => setTab("chat") },
        { id: "tab-changes", group: here, label: "Show changes", hint: "⌘⇧D", icon: <FilePlus2 size={15} />, run: () => setTab("changes") },
        { id: "tab-term", group: here, label: "Show terminal", hint: "⌘J", icon: <SquareTerminal size={15} />, run: () => setTab("terminal") },
        { id: "tab-notes", group: here, label: "Show notes", icon: <NotebookPen size={15} />, run: () => setTab("notes") },
        { id: "run", group: here, label: "Run the app", keywords: "script dev start", icon: <Play size={15} />, run: runScript },
        { id: "open-editor", group: here, label: "Open in editor", hint: "⌘O", keywords: "cursor vscode code", icon: <Code2 size={15} />, run: () => openInEditor() },
        { id: "open-finder", group: here, label: "Show in Finder", icon: <FolderOpen size={15} />, run: () => openInEditor("finder") },
        { id: "pr", group: here, label: "Go to PR actions", keywords: "github pull request create review", icon: <GitPullRequest size={15} />, run: () => document.querySelector<HTMLElement>(".prbar button, .prbar a")?.focus() },
        { id: "pin", group: here, label: ws.pinned ? "Unpin workspace" : "Pin workspace to top", icon: <Pin size={15} />, run: () => api.setPinned(ws.id, !ws.pinned).then(refresh) },
        { id: "archive", group: here, label: "Archive workspace", hint: "⌘⇧A", keywords: "close done remove", icon: <Archive size={15} />, run: () => archive() },
      );
    }
    c.push(
      { id: "new", group: "Create", label: "Create workspace", hint: "⌘N", keywords: "new worktree branch task", icon: <Plus size={15} />, run: () => openNew() },
      { id: "new-pr", group: "Create", label: "Create workspace from a PR", hint: "⌘⇧N", keywords: "new pull request", icon: <GitPullRequest size={15} />, run: () => openNew("pr") },
      { id: "new-issue", group: "Create", label: "Create workspace from a GitHub issue", keywords: "new", icon: <GitPullRequest size={15} />, run: () => openNew("issue") },
      { id: "clone", group: "Create", label: "Clone repo from GitHub", keywords: "add repo", icon: <Download size={15} />, run: () => setCloning(true) },
      { id: "doctor", group: "App", label: "Check setup (git, gh, Claude, Codex)", keywords: "doctor install login", icon: <Gear size={15} />, run: () => { setSelected(null); setView("workspace"); } },
      { id: "add-repo", group: "Create", label: "Add local repo", icon: <FolderOpen size={15} />, run: addRepo },
    );
    const active = workspaces.filter((w) => w.state === "active");
    for (const w of [...active].sort((a, b) => Number(b.unread) - Number(a.unread))) {
      const r = repos.find((x) => x.id === w.repo_id);
      c.push({
        id: `ws-${w.id}`, group: "Workspaces", label: nameOf(w), keywords: `${w.branch} ${r?.name ?? ""}`,
        hint: `${r?.name ?? ""}${w.unread ? " · new" : runningWs.has(w.id) ? " · working" : ""}`,
        icon: runningWs.has(w.id) ? <span className="spinner" /> : <Sparkles size={15} />, run: () => select(w.id),
      });
    }
    c.push(
      { id: "next", group: "Go", label: "Go to next workspace with news", hint: "⌘⌥L", icon: <Sparkles size={15} />, run: nextUnread },
      { id: "diagnostics", group: "App", label: "Copy diagnostics for a bug report", keywords: "debug support version", icon: <Gear size={15} />, run: copyDiagnostics },
      { id: "storage", group: "App", label: "Open storage and clean build folders", keywords: "disk space clean node_modules venv", icon: <FolderOpen size={15} />, run: () => setStorage(true) },
      { id: "scorecard", group: "Go", label: "Open agent scorecard", keywords: "stats compare claude codex success cost", icon: <Crown size={15} />, run: () => setScorecard(true) },
      { id: "board", group: "Go", label: "Open board", hint: "⌘⇧K", keywords: "kanban dashboard status columns", icon: <Kanban size={15} />, run: () => setView("board") },
      { id: "new-section", group: "Create", label: "Add sidebar section", keywords: "new group folder", icon: <Plus size={15} />, run: async () => { const n = prompt("Section name"); if (n?.trim()) { await api.createSection(n); refresh(); } } },
      { id: "sky", group: "Go", label: "Open night sky", keywords: "stars constellations progress", icon: <Moon size={15} />, run: () => setView("sky") },
      { id: "archived", group: "Go", label: showArchived ? "Hide archived workspaces" : "Show archived workspaces", icon: <ArchiveRestore size={15} />, run: () => setShowArchived((v) => !v) },
      { id: "settings", group: "App", label: "Open settings", hint: "⌘,", keywords: "preferences notifications sound", icon: <Gear size={15} />, run: () => setSettingsOpen(true) },

    );
    for (const r of repos) c.push({ id: `filter-${r.id}`, group: "Go", label: `Show only ${r.name}`, keywords: "filter project repo", icon: <FolderOpen size={15} />, run: () => saveSettings({ ...getSettings(), repoFilter: [r.id] }) });
    if (getSettings().repoFilter.length) c.push({ id: "filter-all", group: "Go", label: "Show all repos", keywords: "filter clear project", icon: <FolderOpen size={15} />, run: () => saveSettings({ ...getSettings(), repoFilter: [] }) });
    for (const r of repos) c.push({ id: `scripts-${r.id}`, group: "App", label: `Edit ${r.name} setup and run scripts`, icon: <Gear size={15} />, run: () => setSettingsFor(r.id) });
    if (canImport) c.push({ id: "import", group: "App", label: "Import from Conductor", icon: <Download size={15} />, run: importConductor });
    return c;
  };

  // Conductor's shortcuts, so muscle memory carries over:
  // ⌘K palette, ⌘N / ⌘⇧N new (from PR), ⌘1-9 workspaces, ⌘⌥↑/↓ previous/next, ⌘⌥L next with news,
  // ⌘J terminal, ⌘⇧D changes, ⌘L chat, ⌘B sidebar, ⌘O open in, ⌘P files, ⌘R run, ⌘⇧A archive, ⌘, settings.
  useEffect(() => {
    const sidebarRows = () => Array.from(document.querySelectorAll<HTMLButtonElement>(".sidebar .ws"));
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      const k = e.key.toLowerCase();
      const go = (fn: () => void) => { e.preventDefault(); fn(); };
      if (k === "k" && !e.shiftKey) return go(() => setPalette((p) => !p));
      if (k === "k" && e.shiftKey) return go(() => setView((v) => (v === "board" ? "workspace" : "board")));
      if (e.key === "[" && !e.shiftKey) return go(() => navRef.current(-1));
      if (e.key === "]" && !e.shiftKey) return go(() => navRef.current(1));
      if (k === ",") return go(() => setSettingsOpen(true));
      if (k === "b" && !e.shiftKey && !e.altKey) return go(() => setSidebarHidden((h) => !h));
      if (k === "n") return go(() => openNew(e.shiftKey ? "pr" : "task"));
      if (e.altKey && e.code === "KeyL") return go(nextUnread);
      if (e.altKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) return go(() => {
        const rows = sidebarRows();
        const i = rows.findIndex((r) => r.classList.contains("selected"));
        rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))]?.click();
      });
      if (/^[1-9]$/.test(e.key) && !e.altKey && !e.shiftKey) return go(() => sidebarRows()[Number(e.key) - 1]?.click());
      if (e.altKey && e.code === "KeyA") return go(addRepo);
      if (!ws) return;
      if (k === "j" && !e.shiftKey) return go(() => setTab((t) => (t === "terminal" ? "chat" : "terminal")));
      if (k === "d" && e.shiftKey) return go(() => setTab("changes"));
      if (k === "l" && !e.shiftKey && !e.altKey) return go(() => { setTab("chat"); requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus()); });
      if (k === "a" && e.shiftKey) return go(() => archive());
      if (e.key === "Backspace" && e.shiftKey) return go(() => { for (const id of runningSessions) api.stop(id); });
      if (k === "o" && !e.shiftKey) return go(() => openInEditor());
      if (k === "r" && !e.shiftKey) return go(runScript);
      if (k === "b" && e.shiftKey) return go(openLocal);
      if (k === "p" && !e.shiftKey) return go(() => api.workspaceFiles(ws.id).then(setQuickOpen).catch((er) => say(errText(er), true)));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ws, repos, lit, sky, workspaces, selected, livePtys, localUrls, runningSessions]);

  const settingsRepo = repos.find((r) => r.id === settingsFor);

  return (
    <div className={`app ${resizing ? "resizing" : ""} ${sidebarHidden ? "no-sidebar" : ""}`}>
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
        groupBy={groupBy}
        onGroupBy={(g) => { setGroupBy(g); saveSettings({ ...getSettings(), groupBy: g }); }}
        prStates={prStates}
        lines={lines}
        usage={usage}
        sections={sections}
        onChanged={refresh}
        onRename={(id) => { select(id); const w = workspaces.find((x) => x.id === id); if (w) setEditingTitle(w.title || displayName(sky, w)); }}
        onArchive={(id) => archive({ id })}
        onBoard={() => setView("board")}
        ram={ram}
        lowDiskMb={freeMb !== null && freeMb < 3072 ? freeMb : null}
        onStorage={() => setStorage(true)}
        canBack={history.current.at > 0}
        canForward={history.current.at < history.current.stack.length - 1}
        onBack={() => goHistory(-1)}
        onForward={() => goHistory(1)}
        onToggleSidebar={() => setSidebarHidden((h) => !h)}
      />
      {sidebarHidden && (
        <button className="icon sidebar-toggle floating" onClick={() => setSidebarHidden(() => false)} title="Show sidebar (⌘B)" aria-label="Show sidebar">
          <PanelLeft size={16} />
        </button>
      )}
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
        {view === "workspace" && !ws && <div className="titlebar-drag" data-tauri-drag-region />}
        {view === "board" && (
          <Board
            workspaces={workspaces} repos={repos} sky={sky} prStates={prStates} lines={lines} running={runningWs} costs={costs}
            repoFilter={getSettings().repoFilter}
            onOpen={select}
            onSetStatus={(id, st) => api.setWorkspaceStatus(id, st).then(refresh)}
            onNew={() => openNew()}
          />
        )}
        {view === "sky" && <NightSky sky={sky} lit={lit} shining={shining} workspaces={workspaces} onOpen={select} />}

        {view === "workspace" && !ws && (
          <Welcome
            me={me} repos={repos} mood={mood} purring={purring} onPet={pet}
            onAddRepo={addRepo} onClone={() => setCloning(true)} onNew={() => openNew()}
            canImport={canImport} onImport={importConductor} onImported={refresh}
          />
        )}

        {view === "workspace" && ws && (
          <>
            <header className="ws-header" data-tauri-drag-region>
              <div className="ws-title" data-tauri-drag-region>
                {editingTitle !== null ? (
                  <input
                    className="title-edit"
                    autoFocus
                    value={editingTitle}
                    aria-label="Workspace name"
                    placeholder={star?.name ?? ws.name}
                    onChange={(e) => setEditingTitle(e.target.value)}
                    onBlur={() => { api.setWorkspaceTitle(ws.id, editingTitle).then(refresh); setEditingTitle(null); }}
                    onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") setEditingTitle(null); }}
                  />
                ) : (
                  <h1 onDoubleClick={() => setEditingTitle(ws.title || (star?.name ?? ws.name))} title="Double-click to rename">
                    {displayName(sky, ws)}
                    {ws.pinned && <Pin size={16} className="muted" aria-label="Pinned" />}
                  </h1>
                )}
                <div className="ws-sub">
                  {repo?.name}
                  {ws.title && star ? ` · ${star.name}` : ""}
                  {star ? ` · ${starLine(star)}` : ws.imported_from ? " · imported from Conductor" : ""}
                  {ws.status ? <span className={`sub-status st-${ws.status}`}> · {STATUS_LABEL[ws.status as Exclude<WsStatus, "">]}</span> : null}
                  {costs[ws.id] ? <span className="num" title="Agent spend in this workspace"> · ${costs[ws.id].toFixed(2)}</span> : null}
                </div>
              </div>
              <span className="spacer" />
              {ws.state === "active" && (
                <nav className="tabs" role="tablist" aria-label="Workspace">
                  {TABS.filter((t) => t.id !== "notes" || tab === "notes" || ws.notes.trim()).map((t) => (
                    <button key={t.id} role="tab" aria-selected={tab === t.id} className={`tab ${tab === t.id ? "active" : ""}`} onClick={() => setTab(t.id)} title={TAB_KEYS[t.id] ? `${t.label} (${TAB_KEYS[t.id]})` : undefined}>
                      {t.label}
                    </button>
                  ))}
                </nav>
              )}
              <span className="spacer" />
              <div className="ws-actions">
                {ws.state === "archived" ? (
                  <button className="primary" onClick={() => api.restoreWorkspace(ws.id).then(refresh).catch((e) => say(errText(e), true))}>
                    <ArchiveRestore size={16} />Restore workspace
                  </button>
                ) : (
                  <>
                    {localUrls[ws.id] && livePtys.has(`${ws.id}:run`) && (
                      <button className="quiet" onClick={openLocal} title="Open in browser (⌘⇧B)"><ExternalLink size={14} />{localUrls[ws.id].replace(/^https?:\/\//, "")}</button>
                    )}
                    {ws.pair_id && workspaces.filter((x) => x.pair_id === ws.pair_id).length > 1 && (
                      <button onClick={() => setComparing(ws.pair_id)} title="Compare both agents' results"><Columns2 size={14} />Compare</button>
                    )}
                    {livePtys.has(`${ws.id}:run`) ? (
                      <button className="danger" onClick={runScript} title="Stop the run script (⌘R)"><Square size={13} />Stop</button>
                    ) : (
                      <button onClick={runScript} title="Run the repo's run script (⌘R)"><Play size={14} />Run</button>
                    )}
                    <select aria-label="Open workspace in" value="" onChange={(e) => e.target.value && api.openIn(ws.id, e.target.value).catch((er) => say(errText(er), true))}>
                      <option value="">Open in…</option>
                      {openTargets.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
                    </select>
                    <span className="git-menu">
                      <button className="icon" aria-label="More actions" title="More actions" aria-expanded={moreOpen} onClick={() => setMoreOpen((o) => !o)}><Ellipsis size={18} /></button>
                      {moreOpen && (
                        <span className="menu" role="menu" onMouseLeave={() => setMoreOpen(false)}>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setEditingTitle(ws.title || displayName(sky, ws)); }}><Pencil size={15} /><span>Rename</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); api.setPinned(ws.id, !ws.pinned).then(refresh); }}>{ws.pinned ? <PinOff size={15} /> : <Pin size={15} />}<span>{ws.pinned ? "Unpin" : "Pin to top"}</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setTab("notes"); }}><NotebookPen size={15} /><span>Open notes</span></button>
                          <div className="menu-label">Status</div>
                          {([["", "Automatic (from the PR)"], ...Object.entries(STATUS_LABEL)] as [WsStatus, string][]).map(([k, v]) => (
                            <button key={k || "auto"} role="menuitemradio" aria-checked={ws.status === k} onClick={() => { setMoreOpen(false); api.setWorkspaceStatus(ws.id, k).then(refresh); }}>
                              {ws.status === k ? <Check size={15} /> : <span className={`status-dot st-${k || "auto"}`} />}<span>{v}</span>
                            </button>
                          ))}
                          <div className="menu-sep" />
                          <button role="menuitem" onClick={() => { setMoreOpen(false); archive(); }}><Archive size={15} /><span>Archive</span><kbd>⌘⇧A</kbd></button>
                        </span>
                      )}
                    </span>
                  </>
                )}
              </div>
            </header>

            {ws.state === "active" && (
              <PrBar
                ws={ws}
                repo={repo ?? undefined}
                name={displayName(sky, ws)}
                refreshKey={refreshKey}
                onArchive={(auto) => archive({ auto })}
                onSay={say}
                autopilot={autopilot[ws.id]}
                nameOf={(id) => { const w = workspaces.find((x) => x.id === id); return w ? displayName(sky, w) : "another workspace"; }}
                onSendAgent={(text) => { setTab("chat"); api.sendToWorkspace(ws.id, text).then(refresh).catch((e) => say(errText(e), true)); }}
              />
            )}

            {away?.id === ws.id && (
              <div className="away-banner" role="status">
                <span className="unread" /><b>While you were away:</b> {away.text}
                <span className="spacer" />
                <button className="icon" aria-label="Dismiss" onClick={() => setAway(null)}><X size={13} /></button>
              </div>
            )}

            <div className="panel">
              {ws.state === "archived" ? (
                <div className="empty">
                  <Mascot size={88} mood="sleep" />
                  <h2>Archived</h2>
                  <p>Its branch {ws.branch} is kept. Restore the workspace to bring its folder back.</p>
                </div>
              ) : (
                <>
                  <div className="pane" hidden={tab !== "chat"}>
                    <Chat workspaceId={ws.id} running={runningSessions} onRunningChange={refresh} inject={inject} />
                  </div>
                  <div className="pane" hidden={tab !== "changes"}>
                    {tab === "changes" && <Diff workspaceId={ws.id} refreshKey={refreshKey} focusFile={focusFile} onSendReview={(text) => { setTab("chat"); setInject({ text, nonce: Date.now() }); }} />}
                  </div>
                  <div className="pane" hidden={tab !== "terminal"}>
                    <TerminalPanel key={ws.id} wsId={ws.id} cwd={ws.path} running={livePtys} focus={termFocus} onRun={runScript} onSetup={rerunSetup} />
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
        <NewWorkspace repos={repos} initialRepo={newFor} initialSource={newSource} onCreate={newWorkspace} onClose={() => { setNewFor(null); setNewSource("task"); }} />
      )}
      {settingsRepo && <RepoSettings repo={settingsRepo} onClose={() => { setSettingsFor(null); refresh(); }} />}
      {palette && <Palette commands={commands()} onClose={() => setPalette(false)} />}
      {quickOpen && (
        <Palette
          placeholder="Search files in this workspace"
          commands={quickOpen.map((f) => ({
            id: f, group: "Files", label: f.slice(f.lastIndexOf("/") + 1), hint: f.slice(0, f.lastIndexOf("/") + 1), keywords: f,
            icon: <FilePlus2 size={15} />, run: () => { setTab("changes"); setFocusFile({ path: f, nonce: Date.now() }); },
          }))}
          onClose={() => setQuickOpen(null)}
        />
      )}
      {settingsOpen && <Settings home="~/nekotrail/workspaces" onClose={() => setSettingsOpen(false)} />}
      {toast && (
        <div role="status" className={`toast ${toast.error ? "error" : ""}`}>
          {toast.text}
          {toast.action && <button className="toast-action" onClick={() => { toast.action!.run(); setToast(null); }}>{toast.action.label}</button>}
        </div>
      )}
      {scorecard && <Scorecard onClose={() => setScorecard(false)} />}
      {storage && <Storage names={(id) => { const w = workspaces.find((x) => x.id === id); return w ? displayName(sky, w) : id; }} onClose={() => setStorage(false)} onSay={say} />}
      {comparing && (
        <Compare
          workspaces={workspaces.filter((w) => w.pair_id === comparing && w.state === "active")}
          names={(w) => displayName(sky, w)}
          lines={lines}
          prStates={prStates}
          onOpen={(id) => { setComparing(null); select(id); setTab("changes"); }}
          onKeep={async (id) => {
            const others = workspaces.filter((w) => w.pair_id === comparing && w.id !== id && w.state === "active");
            for (const o of others) await api.archiveWorkspace(o.id, true).catch((e) => say(errText(e), true));
            setComparing(null);
            await refresh();
            select(id);
            say(`Kept ${displayName(sky, workspaces.find((w) => w.id === id)!)}. The other workspace is archived, and its branch is kept.`);
          }}
          onClose={() => setComparing(null)}
        />
      )}
      {cloning && (
        <CloneSheet
          onClose={() => setCloning(false)}
          onDone={async (r) => { setCloning(false); await refresh(); say(`Cloned ${r.name}.`); setNewSource("task"); setNewFor(r.id); }}
        />
      )}
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
