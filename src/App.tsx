import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { Archive, ArchiveRestore, Check, ChevronDown, Columns2, Copy, Link, Crown, Kanban, SquareStack, PanelLeft, X, Ellipsis, ExternalLink, Pencil, Square, Code2, Download, FilePlus2, FolderOpen, GitPullRequest, MessageSquare, Moon, NotebookPen, Pin, PinOff, Play, Plus, Settings as Gear, SquareTerminal, Sparkles, AppWindow, Heart } from "lucide-react";
import { api, constellationOf, displayName, errText, nameInText, starFor, starName, subName, STATUS_LABEL, type Agent, type ChatEvent, type Collision, type GithubUser, type Repo, type Section, type Sky, type Star, type Workspace, type WorkspaceScripts, type ScriptTrust, type AgentConfigTrust, type WsStatus, type PrView, prStateOf, setBeforeTurn } from "./api";
import { Sidebar } from "./components/Sidebar";
import { Chat } from "./components/Chat";
import { Diff } from "./components/Diff";
import { openUrl } from "@tauri-apps/plugin-opener";
import { PrBar } from "./components/PrBar";
import { useFit, overflows } from "./useFit";
import { RepoSettings } from "./components/RepoSettings";
import { NightSky } from "./components/NightSky";
import { NewWorkspace, type NewWorkspaceRequest, type Source as NewSource } from "./components/NewWorkspace";
import { Mascot, type Mood } from "./components/Mascot";
import { Palette, type Command } from "./components/Palette";
import { CloneSheet, Welcome } from "./components/Welcome";
import { Compare } from "./components/Compare";
import { Board } from "./components/Board";
import { Scorecard } from "./components/Scorecard";
import { Storage } from "./components/Storage";
import { CommandsLog } from "./components/CommandsLog";
import { Settings } from "./components/Settings";
import { TrustSheet } from "./components/TrustSheet";
import { UpdateSheet } from "./components/UpdateSheet";
import { useUpdater } from "./useUpdater";
import { ask, ConfirmHost } from "./components/Confirm";
import { chime, getSettings, saveSettings, ting } from "./settings";
import { getZoom, setZoom, stepZoom } from "./appearance";
import { AGENTS, agentName } from "./agents";
// Loaded the first time someone opens the Preview tab, so it costs nothing at startup.
const Preview = lazy(() => import("./components/Preview").then((m) => ({ default: m.Preview })));
import { UserAttentionType } from "@tauri-apps/api/window";

const NAP_AFTER_MS = 5 * 60_000; // the cat naps after 5 quiet minutes

// xterm is a third of the app's JavaScript: load it after first paint instead of before.
const loadTerminal = () => import("./components/Terminal");
const TerminalPanel = lazy(() => loadTerminal().then((m) => ({ default: m.TerminalPanel })));

async function notify(title: string, body: string) {
  try {
    let ok = await isPermissionGranted();
    if (!ok) ok = (await requestPermission()) === "granted";
    if (ok) sendNotification({ title, body });
  } catch {
    /* not in Tauri (preview) */
  }
}

/** A notification's second line: the chat's name, unless it only repeats the workspace title (both come from the first message). */
function chatLine(w: Workspace | undefined, chat?: string): string {
  if (!chat || chat === "New chat") return "";
  const flat = (t: string) => t.toLowerCase().replace(/…$/, "").replace(/[^\p{L}\p{N}]+/gu, "");
  const a = flat(chat), b = flat(w?.title ?? "");
  return b && (a.startsWith(b.slice(0, 24)) || b.startsWith(a.slice(0, 24))) ? "" : chat;
}
import "./App.css";

type Tab = "chat" | "changes" | "terminal" | "preview" | "notes";
const TAB_KEYS: Record<Tab, string> = { chat: "⌘1", changes: "⌘2", terminal: "⌘3", preview: "⌘4", notes: "" };
const TABS: { id: Tab; label: string }[] = [
  { id: "chat", label: "Chat" },
  { id: "changes", label: "Changes" },
  { id: "terminal", label: "Terminal" },
  { id: "preview", label: "Preview" },
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
      const name = w ? nameInText(skyRef.current, w) : "A workspace";
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
  const [trustAsk, setTrustAsk] = useState<{ repo: string; trust?: ScriptTrust; agent?: AgentConfigTrust; answer: (run: boolean) => void } | null>(null);
  const [toast, setToast] = useState<{ text: string; error?: boolean; action?: { label: string; run: () => void } } | null>(null);
  const [discovery, setDiscovery] = useState<{ star: Star; kind: "lit" | "shining" } | null>(null);
  const [canImport, setCanImport] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [wsRefreshKey, setWsRefreshKey] = useState(0); // bumps only for the selected workspace's turns
  const [inject, setInject] = useState<{ text: string; nonce: number; send?: boolean } | undefined>();
  const [append, setAppend] = useState<{ text: string; nonce: number } | undefined>();
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
      const rows = Array.from(document.querySelectorAll<HTMLButtonElement>(".sidebar .ws:not(.smart)"));
      if (!rows.length) return;
      e.preventDefault();
      const i = rows.findIndex((r) => r.classList.contains("selected"));
      rows[Math.max(0, Math.min(rows.length - 1, i + (e.key === "j" ? 1 : -1)))].click();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // A repo's named action ("test", "lint"): typed into the workspace terminal so you see it run.
  const runAction = (name: string) => {
    const cmd = repo?.prefs?.actions?.find(([n]) => n === name)?.[1];
    if (!cmd) return say(`No action named "${name}". Add it in the repo's settings.`, true);
    setTab("terminal");
    window.dispatchEvent(new CustomEvent("nekotrail-terminal", { detail: cmd }));
  };
  // Slash commands that change the view: /run, /diff, /terminal.
  const actionRef = useRef<(a: string) => void>(() => {});
  actionRef.current = (a: string) => {
    if (a.startsWith("run:")) runAction(a.slice(4));
    else if (a === "run") runScript();
    else if (a === "diff") setTab("changes");
    else if (a === "terminal") setTab("terminal");
    else if (a === "preview") setTab("preview");
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
  /** A repo's own nekotrail.json commands run only after you approve exactly those commands. */
  const trusted = async (w: Workspace, s: WorkspaceScripts, kind: "setup" | "run") => {
    const trust = s.trust;
    if (!trust?.[kind]) return true;
    const repo = repos.find((r) => r.id === w.repo_id)?.name ?? "this repo";
    const run = await new Promise<boolean>((answer) => setTrustAsk({ repo, trust, answer }));
    setTrustAsk(null);
    if (!run) return false;
    try {
      await api.trustScripts(w.id, trust.fingerprint);
      return true;
    } catch (e) {
      say(errText(e), true);
      return false;
    }
  };
  /**
   * Before a message goes out: if the repo commits agent config that would run commands (hooks, MCP
   * servers, env) and you haven't trusted those exact files, ask once. "Not now" is remembered until
   * the app restarts; the backend runs the agent with that config off either way until it's trusted.
   */
  const agentAsks = useRef(new Map<string, Promise<void>>());
  useEffect(() => {
    setBeforeTurn(async (id) => {
      const t = await api.agentConfigTrust(id).catch(() => null);
      if (!t) return;
      let asking = agentAsks.current.get(t.fingerprint);
      if (!asking) {
        // One sheet at a time, and parallel sends for the same config share it.
        const before = Promise.all(agentAsks.current.values()).then(() => {});
        asking = before.then(async () => {
          const run = await new Promise<boolean>((answer) => setTrustAsk({ repo: t.repo || "this repo", agent: t, answer }));
          setTrustAsk(null);
          if (run) await api.trustAgentConfig(id, t.fingerprint).catch((e) => say(errText(e), true));
          // Gemini can't run with these off, so its "Not now" isn't kept: the next send asks again.
          else if (t.items.some((it) => it.agent === "gemini")) agentAsks.current.delete(t.fingerprint);
        });
        agentAsks.current.set(t.fingerprint, asking);
      }
      await asking;
    });
    return () => setBeforeTurn(async () => {});
  }, []);
  const startPty = async (id: string, script: "setup" | "run", tabName: string) => {
    await api.ptyRun(id, script);
    setLivePtys((s) => new Set(s).add(id));
    setTermFocus({ tab: tabName, nonce: Date.now() });
  };
  const [notes, setNotes] = useState("");
  // Workspaces whose Preview tab has been opened; only those keep a live page.
  const [previewOn, setPreviewOn] = useState<Set<string>>(new Set());
  useEffect(() => {
    if (tab === "preview" && selected) setPreviewOn((s) => (s.has(selected) ? s : new Set(s).add(selected)));
  }, [tab, selected]);
  const [leaving, setLeaving] = useState<string | null>(null);
  const [sidebarW, setSidebarW] = useState(() => Math.max(244, Number(localStorage.getItem("nekotrail.sidebar")) || 264));
  const [resizing, setResizing] = useState(false);
  const [newFor, setNewFor] = useState<string | null>(params.get("new")); // repo id the new-workspace panel opens on
  const [newSource, setNewSource] = useState<NewSource>((params.get("from") as NewSource) || "task");
  const [newStack, setNewStack] = useState<string | undefined>(params.get("stack") ?? undefined); // workspace to stack the new one on
  const shiningLoaded = useRef(false);
  const [purring, setPurring] = useState(false);
  // A ref, not state: every keypress counts as activity, and a re-render per key would redraw the whole app.
  const lastActive = useRef(Date.now());
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
  // Every workspace's PR, pushed by the one poller in the backend (`pr` events). Nothing here polls GitHub.
  const [prs, setPrs] = useState<Record<string, PrView>>({});
  const [lines, setLines] = useState<Record<string, [number, number]>>({});
  const [openTargets, setOpenTargets] = useState<{ id: string; name: string }[]>([]);
  useEffect(() => { api.openInTargets().then(setOpenTargets).catch(() => {}); }, []);
  // Fetch the terminal code once launch has settled, so it's ready before you open a workspace.
  useEffect(() => { const t = setTimeout(() => { loadTerminal().catch(() => {}); }, 1000); return () => clearTimeout(t); }, []);
  // Sidebar facts that cost a git or gh call per workspace: refreshed after turns and every 5 minutes.
  const [usage, setUsage] = useState<Awaited<ReturnType<typeof api.usageSummary>> | null>(null);
  useEffect(() => {
    api.lineCounts().then(setLines).catch(() => {});
    api.usageSummary().then(setUsage).catch(() => {});
  }, [refreshKey, workspaces.length]);
  useEffect(() => {
    const un = listen<PrView>("pr", ({ payload }) => setPrs((m) => ({ ...m, [payload.workspace_id]: payload })));
    api.prCache().then((all) => setPrs((m) => ({ ...all, ...m }))).catch(() => {});
    // The poller pauses while the window is hidden and looks again when it comes back.
    const vis = () => { api.prVisible(!document.hidden).catch(() => {}); };
    const focus = () => { api.prVisible(true).catch(() => {}); };
    vis();
    document.addEventListener("visibilitychange", vis);
    window.addEventListener("focus", focus);
    return () => { un.then((f) => f()); document.removeEventListener("visibilitychange", vis); window.removeEventListener("focus", focus); };
  }, []);
  // A new workspace has no PR yet: ask for a look so its bar settles quickly.
  useEffect(() => { api.prRefresh().catch(() => {}); }, [workspaces.length]);
  // OPEN, DRAFT, MERGED or CLOSED per workspace, for the sidebar, the board and the radar.
  const prStates = useMemo(() => {
    const out: Record<string, string> = {};
    for (const [id, v] of Object.entries(prs)) { const st = prStateOf(v.pr ?? undefined); if (st) out[id] = st; }
    return out;
  }, [prs]);
  const prStatesKey = JSON.stringify(prStates);
  // Collision radar: reads the file lists the line counts just cached, so it adds no git work.
  const [collisions, setCollisions] = useState<Collision[]>([]);
  useEffect(() => { api.collisions(prStates).then(setCollisions).catch(() => {}); }, [lines, prStatesKey]);
  const [quickOpen, setQuickOpen] = useState<string[] | null>(null);
  const [focusFile, setFocusFile] = useState<{ path: string; nonce: number } | undefined>();
  const [editingTitle, setEditingTitle] = useState<string | null>(null);
  const [cloning, setCloning] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [scorecard, setScorecard] = useState(params.has("scorecard"));
  const [storage, setStorage] = useState(params.has("storage"));
  const [commandsFor, setCommandsFor] = useState<string | null>(null);
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
  const updater = useUpdater();

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
  // The backend renamed a workspace (its title from the first message or the PR, or its branch).
  useEffect(() => {
    const un = listen("workspaces-changed", () => { refresh(); });
    return () => { un.then((f) => f()); };
  }, [refresh]);

  useEffect(() => {
    api.conductorAvailable().then(setCanImport);
    api.sky().then(setSky);
    api.githubProfile().then(setMe);
  }, []);

  useEffect(() => {
    const un = listen<{ running: boolean; workspace_id?: string; ok?: boolean; title?: string; more_queued?: boolean; needs?: string | null }>("running", ({ payload }) => {
      refresh();
      setRefreshKey((k) => k + 1); // a turn ended: reload line counts and the collision radar
      // The agent may have pushed or opened a PR: have the poller look at that repo now.
      if (!payload.running && payload.workspace_id) api.prRefresh(payload.workspace_id, true).catch(() => {});
      // Changes and PR state only move for the workspace whose agent ran.
      if (!payload.workspace_id || payload.workspace_id === selectedRef.current) setWsRefreshKey((k) => k + 1);
      lastActive.current = Date.now();
      if (payload.running || !payload.workspace_id || payload.more_queued) return;
      if (payload.ok === false) setWorriedUntil(Date.now() + 15_000);
      const prefs = getSettings();
      // Human in the loop: the agent stopped because it needs you. Ting ting, a notification, a Dock bounce.
      if (payload.needs) {
        const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
        const name = w ? nameInText(skyRef.current, w) : "A workspace";
        const why: Record<string, string> = {
          plan: "has a plan for you to approve",
          question: "has a question for you",
          login: "needs you to sign in",
          limit: "hit its usage limit",
          install: "can't find its agent CLI",
          error: "stopped with an error",
          approval: "wants your OK",
          protected: "changed a protected file",
        };
        if (prefs.needsSound) ting();
        if (prefs.needsNotify) notify(`${name} ${why[payload.needs] ?? "needs you"}`, chatLine(w, payload.title) || "Open nekotrail to continue.");
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
        const name = w ? nameInText(skyRef.current, w) : "A workspace";
        notify(payload.ok === false ? `${name} needs you` : `${name} is done`, chatLine(w, payload.title) || "The agent finished its turn.");
      }
    });
    // Mid-turn requests (approve a tool call) arrive while the turn is still running.
    const unNeeds = listen<{ workspace_id: string; needs: string; title?: string }>("needs", ({ payload }) => {
      const prefs = getSettings();
      const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
      const name = w ? nameInText(skyRef.current, w) : "A workspace";
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

  // ⌘+ / ⌘- / ⌘0 zoom the whole app (80% to 150%), remembered between launches. Also in Settings, Appearance.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      if (e.key === "=" || e.key === "+") setZoom(stepZoom(getZoom(), 1));
      else if (e.key === "-") setZoom(stepZoom(getZoom(), -1));
      else if (e.key === "0") setZoom(1);
      else return;
      e.preventDefault();
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
    const wake = () => {
      const napping = Date.now() - lastActive.current > NAP_AFTER_MS;
      lastActive.current = Date.now();
      if (napping) setNow(Date.now()); // the cat wakes up right away
    };
    window.addEventListener("keydown", wake);
    window.addEventListener("pointerdown", wake);
    return () => { window.removeEventListener("keydown", wake); window.removeEventListener("pointerdown", wake); };
  }, []);

  const mood: Mood =
    runningWs.size > 0 ? "working" : worriedUntil > now ? "worried" : now - lastActive.current > NAP_AFTER_MS ? "sleep" : "idle";

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
  // The toolbar folds up instead of clipping (zoomed in, narrow window, wide sidebar):
  // 1 labels become icons and "Open in" moves into ⋯, 2 the tabs tighten, 3 Run and Compare move into ⋯ too.
  const paired = !!ws?.pair_id && workspaces.filter((x) => x.pair_id === ws.pair_id).length > 1;
  const runLive = !!ws && livePtys.has(`${ws.id}:run`);
  const urlShown = !!ws && !!localUrls[ws.id] && runLive;
  const [toolbarRef, fold] = useFit(3, (el) => overflows(el) || (el.querySelector<HTMLElement>(".ws-title")?.clientWidth ?? 999) < 120,
    [ws?.id, ws?.state, paired, runLive, urlShown, repo?.prefs?.actions?.length ?? 0, tab, openTargets.length]);

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
      const who = agentName(last.agent);
      const t = last.ms ? (last.ms >= 60_000 ? `${Math.floor(last.ms / 60_000)}m ${Math.round((last.ms % 60_000) / 1000)}s` : `${Math.round(last.ms / 1000)}s`) : "";
      parts.push(`${who} ${last.error ? "stopped with an error" : "finished"}${t ? ` in ${t}` : ""}${last.cost ? ` for $${last.cost.toFixed(2)}` : ""}`);
    }
    const l = lines[w.id];
    if (l && (l[0] || l[1])) parts.push(`+${l[0]} -${l[1]} lines changed`);
    const pr = prStates[w.id];
    if (pr) parts.push(pr === "OPEN" ? "PR open" : pr === "DRAFT" ? "draft PR" : pr === "MERGED" ? "PR merged" : "PR closed");
    if (parts.length) setAway({ id: w.id, text: parts.join(" · ") });
  };
  // Workspaces you opened, most recent first, for the palette's Recent group. Kept between launches.
  const recentIds = (): string[] => { try { return JSON.parse(localStorage.getItem("nekotrail.recent") || "[]"); } catch { return []; } };
  useEffect(() => {
    if (!selected) return;
    localStorage.setItem("nekotrail.recent", JSON.stringify([selected, ...recentIds().filter((x) => x !== selected)].slice(0, 20)));
  }, [selected]);
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
          await Promise.all(made.map((w, i) => api.setWorkspaceTitle(w.id, `${base} (${agentName(agents[i])})`)));
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
      const setup = !!scripts.setup && await trusted(w, scripts, "setup");
      if (scripts.setup && !setup) say("Setup skipped. Run it later from the Terminal tab.");
      await Promise.all(made.map(async (mw, i) => {
        if (setup) {
          // Setup (installs, .env) first, so the agent's first test run has what it needs.
          const done = waitForExit(`${mw.id}:setup`);
          await startPty(`${mw.id}:setup`, "setup", "setup");
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
    const name = nameInText(sky, target);
    try {
      await api.archiveWorkspace(target.id);
    } catch (e) {
      if (errText(e) !== "DIRTY") return say(errText(e), true);
      if (opts?.auto) return say(`The PR for ${name} merged, but it has uncommitted changes, so it stays open. Commit or discard them, then archive it.`);
      const go = await ask({
        title: `Archive ${name} with uncommitted changes?`,
        body: `The branch ${target.branch} is kept, but uncommitted work in its folder is lost.`,
        ok: "Archive anyway",
        danger: true,
      });
      if (!go) return;
      try {
        await api.archiveWorkspace(target.id, true);
      } catch (e2) {
        return say(errText(e2), true);
      }
    }
    // Open the next row in the sidebar (or the one above), not the empty welcome.
    const rows = Array.from(document.querySelectorAll<HTMLElement>(".sidebar .ws:not(.smart)[data-id]")).map((r) => r.dataset.id!);
    const at = rows.indexOf(target.id);
    const next = at >= 0 ? (rows[at + 1] ?? rows[at - 1] ?? null) : null;
    setLeaving(target.id);
    await new Promise((r) => setTimeout(r, 320));
    setLeaving(null);
    if (selectedRef.current === target.id) setSelected(next);
    await refresh();
    say(`Archived ${name}. Branch ${target.branch} is kept.`, false, {
      label: "Undo",
      run: () => api.restoreWorkspace(target.id).then(refresh).then(() => select(target.id)).catch((e) => say(errText(e), true)),
    });
  };

  // ⌘R: start the run script, or stop it if it is running.
  const runScript = async (opts?: { stay?: boolean }) => {
    if (!ws) return;
    const id = `${ws.id}:run`;
    if (livePtys.has(id)) {
      await api.ptyClose(id);
      setLivePtys((s) => { const n = new Set(s); n.delete(id); return n; });
      return;
    }
    const s = await api.workspaceScripts(ws.id);
    if (!s.run) return setSettingsFor(ws.repo_id);
    if (!await trusted(ws, s, "run")) return;
    if (!opts?.stay) setTab("terminal");
    await startPty(id, "run", "run");
  };
  const rerunSetup = async () => {
    if (!ws) return;
    const s = await api.workspaceScripts(ws.id);
    if (!s.setup) return setSettingsFor(ws.repo_id);
    if (!await trusted(ws, s, "setup")) return;
    await startPty(`${ws.id}:setup`, "setup", "setup");
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
  const openNew = (source: NewSource = "task", stackOn?: string) => {
    // "On a workspace" starts with the open workspace picked: stacking on what you're looking at is the common case.
    setNewStack(stackOn ?? ws?.id);
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
      ...AGENTS.map((a) => `${a}: ${d?.[a]?.version || "missing"}`),
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
    // The star goes in the hint and the search words, so "mochi" still finds a titled workspace.
    const hintOf = (w: Workspace) => [subName(sky, w), repos.find((x) => x.id === w.repo_id)?.name].filter(Boolean).join(" · ");
    // Recent: the last places you went, unread first. Only shown before you type, so it never duplicates results.
    const others = workspaces.filter((w) => w.state === "active" && w.id !== ws?.id);
    const order = recentIds();
    const rank = (id: string) => (order.indexOf(id) + 1) || 99;
    const recent = others
      .filter((w) => w.unread || order.includes(w.id))
      .sort((a, b) => Number(b.unread) - Number(a.unread) || rank(a.id) - rank(b.id))
      .slice(0, 5);
    for (const w of recent) {
      c.push({
        id: `recent-${w.id}`, group: "Recent", label: nameOf(w), emptyOnly: true,
        hint: `${hintOf(w)}${w.unread ? " · new" : runningWs.has(w.id) ? " · working" : ""}`,
        icon: runningWs.has(w.id) ? <span className="spinner" /> : w.unread ? <span className="unread" /> : <Sparkles size={15} />, run: () => select(w.id),
      });
    }
    if (ws && ws.state === "active") {
      const here = nameOf(ws);
      c.push(
        { id: "tab-chat", group: here, label: "Show chat", hint: "⌘1", icon: <MessageSquare size={15} />, run: () => setTab("chat") },
        { id: "tab-changes", group: here, label: "Show changes", hint: "⌘2", icon: <FilePlus2 size={15} />, run: () => setTab("changes") },
        { id: "tab-term", group: here, label: "Show terminal", hint: "⌘3", icon: <SquareTerminal size={15} />, run: () => setTab("terminal") },
        { id: "tab-preview", group: here, label: "Show preview", hint: "⌘4", keywords: "browser localhost app web annotate screenshot", icon: <AppWindow size={15} />, run: () => setTab("preview") },
        { id: "tab-notes", group: here, label: "Show notes", icon: <NotebookPen size={15} />, run: () => setTab("notes") },
        ...((repo?.prefs?.actions ?? []).map(([n, c]) => ({ id: `act-${n}`, group: here, label: `Run ${n}`, hint: c, keywords: `action script ${c}`, icon: <Play size={15} />, run: () => runAction(n) }))),
        { id: "run", group: here, label: "Run the app", keywords: "script dev start", icon: <Play size={15} />, run: () => runScript() },
        { id: "open-editor", group: here, label: "Open in editor", hint: "⌘O", keywords: "cursor vscode code", icon: <Code2 size={15} />, run: () => openInEditor() },
        { id: "commands", group: here, label: "Show commands the agent ran", keywords: "log history shell bash terminal audit", icon: <SquareTerminal size={15} />, run: () => setCommandsFor(ws.id) },
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
      { id: "new-stack", group: "Create", label: "Create workspace stacked on another", keywords: "new stacked pr branch depends", icon: <SquareStack size={15} />, run: () => openNew("stack", ws?.id) },
      { id: "clone", group: "Create", label: "Clone repo from GitHub", keywords: "add repo", icon: <Download size={15} />, run: () => setCloning(true) },
      { id: "update", group: "App", label: "Check for updates", keywords: "upgrade new version release download", icon: <Download size={15} />, run: () => updater.checkNow(true) },
        { id: "sponsor", group: "App", label: "Sponsor nekotrail", keywords: "donate support github sponsors coffee fund", icon: <Heart size={15} />, run: () => openUrl("https://github.com/sponsors/Harshil-Jani").catch((e) => say(errText(e), true)) },
      { id: "doctor", group: "App", label: "Check setup (git, gh, Claude, Codex, Gemini, OpenCode)", keywords: "doctor install login", icon: <Gear size={15} />, run: () => { setSelected(null); setView("workspace"); } },
      { id: "add-repo", group: "Create", label: "Add local repo", icon: <FolderOpen size={15} />, run: addRepo },
    );
    const active = workspaces.filter((w) => w.state === "active");
    for (const w of [...active].sort((a, b) => Number(b.unread) - Number(a.unread))) {
      const r = repos.find((x) => x.id === w.repo_id);
      c.push({
        id: `ws-${w.id}`, group: "Workspaces", label: nameOf(w), keywords: `${w.branch} ${r?.name ?? ""} ${starName(sky, w)}`,
        hint: `${hintOf(w)}${w.unread ? " · new" : runningWs.has(w.id) ? " · working" : ""}`,
        icon: runningWs.has(w.id) ? <span className="spinner" /> : <Sparkles size={15} />, run: () => select(w.id),
      });
    }
    c.push(
      { id: "next", group: "Go", label: "Go to next workspace with news", hint: "⌘⌥L", icon: <Sparkles size={15} />, run: nextUnread },
      { id: "diagnostics", group: "App", label: "Copy diagnostics for a bug report", keywords: "debug support version", icon: <Gear size={15} />, run: copyDiagnostics },
      { id: "storage", group: "App", label: "Open storage and clean build folders", keywords: "disk space clean node_modules venv", icon: <FolderOpen size={15} />, run: () => setStorage(true) },
      { id: "scorecard", group: "Go", label: "Open agent scorecard", keywords: "stats compare claude codex gemini success cost", icon: <Crown size={15} />, run: () => setScorecard(true) },
      { id: "board", group: "Go", label: "Open board", hint: "⌘⇧K", keywords: "kanban dashboard status columns", icon: <Kanban size={15} />, run: () => setView("board") },
      { id: "new-section", group: "Create", label: "Add sidebar section", keywords: "new group folder", icon: <Plus size={15} />, run: () => { setSidebarHidden(() => false); window.dispatchEvent(new Event("nekotrail-new-section")); } },
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

  // Shortcuts: ⌘1 / ⌘2 / ⌘3 show chat, changes, terminal (like Finder's view modes); ⌃1 to ⌃9 open
  // the sidebar's workspaces. Conductor's other keys still work, so muscle memory carries over: ⌘K palette,
  // ⌘N / ⌘⇧N new (from PR), ⌘⌥↑/↓ previous/next, ⌘⌥L next with news, ⌘J terminal, ⌘⇧D changes,
  // ⌘L chat, ⌘B sidebar, ⌘O open in, ⌘P files, ⌘R run, ⌘⇧A archive, ⌘, settings.
  useEffect(() => {
    const sidebarRows = () => Array.from(document.querySelectorAll<HTMLButtonElement>(".sidebar .ws:not(.smart)"));
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && /^[1-9]$/.test(e.key)) {
        if ((e.target as HTMLElement).closest?.(".xterm")) return; // the shell owns ⌃ keys in the terminal
        e.preventDefault();
        sidebarRows()[Number(e.key) - 1]?.click();
        return;
      }
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
      if (e.altKey && e.code === "KeyA") return go(addRepo);
      if (!ws) return;
      if (/^[1-4]$/.test(e.key) && !e.altKey && !e.shiftKey && ws.state === "active") return go(() => setTab((["chat", "changes", "terminal", "preview"] as const)[Number(e.key) - 1]));
      if (k === "j" && !e.shiftKey) return go(() => setTab((t) => (t === "terminal" ? "chat" : "terminal")));
      if (k === "d" && e.shiftKey) return go(() => setTab("changes"));
      if (e.altKey && !e.shiftKey && e.code === "KeyP") return go(() => setTab((t) => (t === "preview" ? "chat" : "preview")));
      if (k === "l" && !e.shiftKey && !e.altKey) return go(() => { setTab("chat"); requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus()); });
      if (k === "a" && e.shiftKey) return go(() => archive());
      if (e.key === "Backspace" && e.shiftKey) return go(() => { for (const id of runningSessions) api.stop(id); });
      if (k === "o" && !e.shiftKey) return go(() => openInEditor());
      if (k === "r" && !e.shiftKey) return go(() => runScript({ stay: tab === "preview" }));
      if (k === "b" && e.shiftKey) return go(openLocal);
      if (k === "p" && !e.shiftKey) return go(() => api.workspaceFiles(ws.id).then(setQuickOpen).catch((er) => say(errText(er), true)));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ws, repos, lit, sky, workspaces, selected, livePtys, localUrls, runningSessions, tab]);

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
        collisions={collisions}
        usage={usage}
        sections={sections}
        onChanged={refresh}
        onRename={(id) => { select(id); const w = workspaces.find((x) => x.id === id); if (w) setEditingTitle(displayName(sky, w)); }}
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
            <header className={`ws-header${fold >= 1 ? " tb-icons" : ""}${fold >= 2 ? " tb-compact" : ""}${fold >= 3 ? " tb-min" : ""}`} ref={toolbarRef} data-tauri-drag-region>
              <div className="ws-title" data-tauri-drag-region>
                {editingTitle !== null ? (
                  <input
                    className="title-edit"
                    autoFocus
                    value={editingTitle}
                    aria-label="Workspace name"
                    placeholder="Name it after its task"
                    title="Leave it empty to name it after the PR or the first message again"
                    onChange={(e) => setEditingTitle(e.target.value)}
                    onBlur={() => {
                      // Only a real change counts as naming it by hand; empty goes back to automatic.
                      if (editingTitle.trim() !== displayName(sky, ws)) api.setWorkspaceTitle(ws.id, editingTitle).then(refresh).catch((e) => say(errText(e), true));
                      setEditingTitle(null);
                    }}
                    onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") setEditingTitle(null); }}
                  />
                ) : (
                  <h1 onDoubleClick={() => setEditingTitle(displayName(sky, ws))} title={`${ws.title_source === "auto" ? "Named after the first message. " : ws.title_source === "pr" ? "Named after the PR. " : ""}Double-click to rename.`}>
                    {displayName(sky, ws)}
                    {ws.pinned && <Pin size={16} className="muted" aria-label="Pinned" />}
                  </h1>
                )}
                <div className="ws-sub">{subName(sky, ws) && <><span className="ws-sub-star">{subName(sky, ws)}</span> · </>}{repo?.name} · {ws.branch} → {ws.target_branch}</div>
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
                    {fold < 3 && urlShown && (
                      <button className="quiet tb-url" onClick={openLocal} title={`Open ${localUrls[ws.id]} in your browser (⌘⇧B)`} aria-label="Open in browser">
                        <ExternalLink size={14} />{fold < 1 && <span className="tb-label">{localUrls[ws.id].replace(/^https?:\/\//, "")}</span>}
                      </button>
                    )}
                    {fold < 3 && paired && (
                      <button onClick={() => setComparing(ws.pair_id)} title="Compare both agents' results" aria-label="Compare"><Columns2 size={14} />{fold < 1 && "Compare"}</button>
                    )}
                    {fold < 3 && (runLive ? (
                      <button className="danger" onClick={() => runScript()} title="Stop the run script (⌘R)" aria-label="Stop"><Square size={13} />{fold < 1 && "Stop"}</button>
                    ) : (
                      <button onClick={() => runScript({ stay: tab === "preview" })} title="Run the repo's run script (⌘R)" aria-label="Run"><Play size={14} />{fold < 1 && "Run"}</button>
                    ))}
                    {(repo?.prefs?.actions?.length ?? 0) > 0 && (
                      <span className="git-menu">
                        <button className="icon" aria-label="More actions" title="More actions" aria-expanded={actionsOpen} onClick={() => setActionsOpen((o) => !o)}><ChevronDown size={14} /></button>
                        {actionsOpen && (
                          <span className="menu" role="menu" onMouseLeave={() => setActionsOpen(false)}>
                            {repo!.prefs.actions.map(([n, c]) => (
                              <button key={n} role="menuitem" onClick={() => { setActionsOpen(false); runAction(n); }} title={c}><Play size={14} /><span>{n}</span><kbd>{c.length > 24 ? c.slice(0, 24) + "…" : c}</kbd></button>
                            ))}
                          </span>
                        )}
                      </span>
                    )}
                    {fold < 1 && <select aria-label="Open workspace in" value="" onChange={(e) => e.target.value && api.openIn(ws.id, e.target.value).catch((er) => say(errText(er), true))}>
                      <option value="">Open in</option>
                      {openTargets.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
                    </select>}
                    <span className="git-menu">
                      <button className="icon" aria-label="More actions" title="More actions" aria-expanded={moreOpen} onClick={() => setMoreOpen((o) => !o)}><Ellipsis size={18} /></button>
                      {moreOpen && (
                        <span className="menu" role="menu" onMouseLeave={() => setMoreOpen(false)}>
                          {/* What the folded toolbar has no room for. */}
                          {fold >= 3 && <>
                            {urlShown && <button role="menuitem" onClick={() => { setMoreOpen(false); openLocal(); }}><ExternalLink size={15} /><span>Open {localUrls[ws.id].replace(/^https?:\/\//, "")}</span><kbd>⌘⇧B</kbd></button>}
                            {paired && <button role="menuitem" onClick={() => { setMoreOpen(false); setComparing(ws.pair_id); }}><Columns2 size={15} /><span>Compare</span></button>}
                            {runLive
                              ? <button role="menuitem" onClick={() => { setMoreOpen(false); runScript(); }}><Square size={15} /><span>Stop the Run script</span><kbd>⌘R</kbd></button>
                              : <button role="menuitem" onClick={() => { setMoreOpen(false); runScript({ stay: tab === "preview" }); }}><Play size={15} /><span>Run</span><kbd>⌘R</kbd></button>}
                            <div className="menu-sep" />
                          </>}
                          {fold >= 1 && openTargets.length > 0 && <>
                            <div className="menu-label">Open in</div>
                            {openTargets.map((t) => (
                              <button key={t.id} role="menuitem" onClick={() => { setMoreOpen(false); api.openIn(ws.id, t.id).catch((er) => say(errText(er), true)); }}><AppWindow size={15} /><span>{t.name}</span></button>
                            ))}
                            <div className="menu-sep" />
                          </>}
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setEditingTitle(displayName(sky, ws)); }}><Pencil size={15} /><span>Rename</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); api.setPinned(ws.id, !ws.pinned).then(refresh); }}>{ws.pinned ? <PinOff size={15} /> : <Pin size={15} />}<span>{ws.pinned ? "Unpin" : "Pin to top"}</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setTab("notes"); }}><NotebookPen size={15} /><span>Open notes</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setCommandsFor(ws.id); }}><SquareTerminal size={15} /><span>Commands the agent ran</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); navigator.clipboard.writeText(ws.branch); say(`Copied ${ws.branch}`); }}><Copy size={15} /><span>Copy branch name</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); const pr = prs[ws.id]?.pr; if (pr?.url) { navigator.clipboard.writeText(pr.url); say("Copied the PR link."); } else say("This workspace has no PR yet."); }}><Link size={15} /><span>Copy PR link</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); openNew("stack", ws.id); }}><SquareStack size={15} /><span>Stack a new workspace on this one</span></button>
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
                name={nameInText(sky, ws)}
                star={star?.name ?? starName(sky, ws)}
                refreshKey={wsRefreshKey}
                collision={collisions.find((c) => c.order.includes(ws.id))}
                prStates={prStates}
                prView={prs[ws.id]}
                parentPr={ws.parent_id ? prs[ws.parent_id]?.pr : undefined}
                lines={lines}
                onOpenWorkspace={select}
                stackedOnThis={workspaces.filter((w) => w.state === "active" && w.parent_id === ws.id).map((w) => w.id)}
                onRestacked={refresh}
                onArchive={(auto, id) => archive({ auto, id })}
                onSay={say}
                autopilot={autopilot[ws.id]}
                nameOf={(id) => { const w = workspaces.find((x) => x.id === id); return w ? displayName(sky, w) : "another workspace"; }}
                textNameOf={(id) => { const w = workspaces.find((x) => x.id === id); return w ? nameInText(sky, w) : "another workspace"; }}
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
                    <Chat workspaceId={ws.id} running={runningSessions} onRunningChange={refresh} inject={inject} append={append} />
                  </div>
                  <div className="pane" hidden={tab !== "changes"}>
                    {tab === "changes" && <Diff workspaceId={ws.id} refreshKey={wsRefreshKey} focusFile={focusFile} onSendReview={(text, send) => { setTab("chat"); setInject({ text, nonce: Date.now(), send }); }} />}
                  </div>
                  <div className="pane" hidden={tab !== "terminal"}>
                    <Suspense fallback={null}>
                      <TerminalPanel key={ws.id} wsId={ws.id} cwd={ws.path} running={livePtys} focus={termFocus} onRun={() => runScript()} onSetup={rerunSetup} />
                    </Suspense>
                  </div>
                  <div className="pane" hidden={tab !== "preview"}>
                    {previewOn.has(ws.id) && (
                      <Suspense fallback={null}>
                        <Preview
                          key={ws.id}
                          workspaceId={ws.id}
                          detected={localUrls[ws.id]}
                          running={livePtys.has(`${ws.id}:run`)}
                          active={tab === "preview"}
                          onRun={() => runScript({ stay: true })}
                          onSetup={() => setSettingsFor(ws.repo_id)}
                          onAttach={(text) => { setTab("chat"); setAppend({ text, nonce: Date.now() }); say("Screenshot attached. Add anything else and send."); }}
                          onSay={say}
                        />
                      </Suspense>
                    )}
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
        <NewWorkspace
          repos={repos} initialRepo={newFor} initialSource={newSource} onCreate={newWorkspace}
          stackTargets={workspaces.filter((w) => w.state === "active").map((w) => ({ id: w.id, repo_id: w.repo_id, name: displayName(sky, w), text: nameInText(sky, w), star: subName(sky, w), branch: w.branch }))}
          initialStack={newStack}
          onClose={() => { setNewFor(null); setNewSource("task"); setNewStack(undefined); }}
        />
      )}
      {trustAsk && <TrustSheet repo={trustAsk.repo} trust={trustAsk.trust} agent={trustAsk.agent} onAnswer={trustAsk.answer} />}
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
      {settingsOpen && <Settings home="~/nekotrail/workspaces" onClose={() => setSettingsOpen(false)} update={updater.phase} onCheckUpdates={() => updater.checkNow(false)} />}
      <ConfirmHost />
      {updater.open && updater.phase && (
        <UpdateSheet
          phase={updater.phase}
          onUpdate={updater.startUpdate}
          onNotNow={updater.notNow}
          onRestartNow={updater.restartNow}
          onRestartWhenIdle={updater.restartWhenIdle}
        />
      )}
      {toast && (
        <div role="status" className={`toast ${toast.error ? "error" : ""}`}>
          {toast.text}
          {toast.action && <button className="toast-action" onClick={() => { toast.action!.run(); setToast(null); }}>{toast.action.label}</button>}
        </div>
      )}
      {scorecard && <Scorecard onClose={() => setScorecard(false)} />}
      {commandsFor && (() => {
        const w = workspaces.find((x) => x.id === commandsFor);
        return <CommandsLog workspaceId={commandsFor} name={w ? displayName(sky, w) : "this workspace"} onClose={() => setCommandsFor(null)} />;
      })()}
      {storage && <Storage names={(id) => { const w = workspaces.find((x) => x.id === id); return w ? displayName(sky, w) : id; }} onClose={() => setStorage(false)} onSay={say} />}
      {comparing && (
        <Compare
          workspaces={workspaces.filter((w) => w.pair_id === comparing && w.state === "active")}
          names={(w) => starName(sky, w)}
          task={workspaces.find((w) => w.pair_id === comparing && w.title)?.title ?? ""}
          lines={lines}
          prStates={prStates}
          onOpen={(id) => { setComparing(null); select(id); setTab("changes"); }}
          onKeep={async (id) => {
            const others = workspaces.filter((w) => w.pair_id === comparing && w.id !== id && w.state === "active");
            for (const o of others) await api.archiveWorkspace(o.id, true).catch((e) => say(errText(e), true));
            setComparing(null);
            await refresh();
            select(id);
            say(`Kept ${starName(sky, workspaces.find((w) => w.id === id)!)}. The other workspace is archived, and its branch is kept.`);
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
