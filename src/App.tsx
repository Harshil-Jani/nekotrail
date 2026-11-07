import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { bannerText, isQuiet, loadNoticeMode, showBanner } from "./notice";
import { Variable as VariableIcon, FolderInput, ArrowRightLeft, Archive, ArchiveRestore, Check, ChevronRight, GitBranch, Columns2, Copy, Link, Crown, Kanban, SquareStack, PanelLeft, X, Ellipsis, ExternalLink, Pencil, Square, Code2, Download, FilePlus2, FolderOpen, GitPullRequest, MessageSquare, Moon, NotebookPen, Pin, PinOff, Play, Plus, Settings as Gear, SquareTerminal, Sparkles, AppWindow, Heart, Coffee, Search, Eye, Network, ListChecks, BookText, ScrollText, ListOrdered, ListPlus, Pause, Share, Gauge } from "lucide-react";
import { taskFacts } from "./editorTask";
import { api, constellationOf, displayName, errText, nameInText, starFor, starName, subName, STATUS_LABEL, type Agent, type ChatEvent, type Collision, type GithubUser, type Repo, type Section, type Sky, type Star, type Workspace, type WorkspaceScripts, type ScriptTrust, type AgentConfigTrust, type WsStatus, type PrView, type CheckDot, type RemoteJob, type ReviewRequest, prStateOf, setBeforeTurn, type JumpTo, type SearchHit, isHistory, type TerminalReport } from "./api";
import { reviewAgent, reviewWorkspaceFor } from "./review";
import { applyRunning, DoneWatchdog, Latest, sameSet, type RunningSets } from "./runState";
import { ptyBus } from "./ptyBus";
import { Sidebar } from "./components/Sidebar";
import { Chat } from "./components/Chat";
import { Diff } from "./components/Diff";
import { openUrl } from "@tauri-apps/plugin-opener";
import { PrBar } from "./components/PrBar";
import { ReviewBar } from "./components/ReviewBar";
import { RepoSettings } from "./components/RepoSettings";
import { NightSky } from "./components/NightSky";
import { NewWorkspace, type NewWorkspaceRequest, type Source as NewSource } from "./components/NewWorkspace";
import { Mascot, type Mood } from "./components/Mascot";
import { Palette, type Command } from "./components/Palette";
import { SearchSheet } from "./components/SearchSheet";
import { CloneSheet, Welcome } from "./components/Welcome";
import { FirstRun, type StartRequest } from "./components/FirstRun";
import { Tour } from "./components/Tour";
import { describeFirstRun, firstLook, isAnswer, KEYS, markFirstRun, markTourSeen, readFirstRun, shouldAutoTour, startStep, stepAfterRepos, type Step } from "./onboarding";
import { Compare } from "./components/Compare";
import { Board } from "./components/Board";
import { Queue } from "./components/Queue";
import { Usage } from "./components/Usage";
import { counts as queueCounts } from "./queue";
import { Scorecard } from "./components/Scorecard";
import { Storage } from "./components/Storage";
import { CommandsLog } from "./components/CommandsLog";
import { RulesSheet } from "./components/RulesSheet";
import { EnvSheet } from "./components/EnvSheet";
import { ExportSheet } from "./components/ExportSheet";
import { TerminalImport } from "./components/TerminalImport";
import { ConductorSheet, MoveNotice } from "./components/ConductorSheet";
import { NOTICE_KEY, noticeShown } from "./conductorRescue";
import { Settings, type Pane as SettingsPane } from "./components/Settings";
import { loadPrompts, type PromptEntry } from "./prompts";
import { TrustSheet } from "./components/TrustSheet";
import { UpdateSheet } from "./components/UpdateSheet";
import { useUpdater } from "./useUpdater";
import { ask, ConfirmHost } from "./components/Confirm";
import { PortsPopover } from "./components/PortsPopover";
import { chime, getSettings, saveSettings, ting } from "./settings";
import { getZoom, setZoom, stepZoom } from "./appearance";
import { IS_MAC, isOpenIn, isWorkspaceDigit, keyHint, mod } from "./platform";
import { AGENTS, agentName } from "./agents";
import { isLinked, mainOf, membersOf } from "./multiRepo";
import { GroupPrBar } from "./components/GroupPrBar";
// Loaded the first time someone opens the Preview tab, so it costs nothing at startup.
const Preview = lazy(() => import("./components/Preview").then((m) => ({ default: m.Preview })));
const Checks = lazy(() => import("./components/Checks").then((m) => ({ default: m.Checks })));
import { UserAttentionType } from "@tauri-apps/api/window";

const NAP_AFTER_MS = 5 * 60_000; // the cat naps after 5 quiet minutes

// xterm is a third of the app's JavaScript: load it after first paint instead of before.
const loadTerminal = () => import("./components/Terminal");
const TerminalPanel = lazy(() => loadTerminal().then((m) => ({ default: m.TerminalPanel })));

/** Where a banner comes from: the task as its title, "repo · branch" under it. */
type BannerFrom = { task: string; repo?: string; branch?: string };

/** A system banner: what happened, and an optional detail line. Skipped in e2e runs and unbundled macOS builds (notice.rs). */
function notify(from: BannerFrom, what: string, detail?: string | null, mood: "happy" | "idle" = "idle") {
  void showBanner(bannerText({ ...from, what, detail }), mood);
}

/** Bounce the Dock icon (or flag the taskbar), never during an e2e run. */
function bounce() {
  if (isQuiet() || !(document.hidden || !document.hasFocus())) return;
  try { getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(() => {}); } catch { /* preview */ }
}

/** A notification's second line: the chat's name, unless it only repeats the workspace title (both come from the first message). */
function chatLine(w: Workspace | undefined, chat?: string): string {
  if (!chat || chat === "New chat") return "";
  const flat = (t: string) => t.toLowerCase().replace(/…$/, "").replace(/[^\p{L}\p{N}]+/gu, "");
  const a = flat(chat), b = flat(w?.title ?? "");
  return b && (a.startsWith(b.slice(0, 24)) || b.startsWith(a.slice(0, 24))) ? "" : chat;
}
import "./App.css";

type Tab = "chat" | "changes" | "terminal" | "preview" | "checks" | "notes";
const TAB_KEYS: Record<Tab, string> = { chat: keyHint("⌘1"), changes: keyHint("⌘2"), terminal: keyHint("⌘3"), preview: keyHint("⌘4"), checks: keyHint("⌘5"), notes: "" };
const TABS: { id: Tab; label: string }[] = [
  { id: "chat", label: "Chat" },
  { id: "changes", label: "Changes" },
  { id: "terminal", label: "Terminal" },
  { id: "preview", label: "Preview" },
  { id: "checks", label: "Checks" },
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
  const [view, setView] = useState<"workspace" | "sky" | "board" | "queue" | "usage">((params.get("view") as "sky" | "board" | "queue" | "usage") || "workspace");
  // The task queue: counts for the sidebar, and a nudge that opens its Add form.
  const [queueInfo, setQueueInfo] = useState<{ running: number; next: number; needs: number; paused: boolean }>({ running: 0, next: 0, needs: 0, paused: false });
  const [queueAdd, setQueueAdd] = useState(params.has("queueAdd") ? 1 : 0);
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
      if (payload.state === "ready" || payload.state === "gave_up" || payload.state === "budget" || payload.state === "loop") {
        const prefs = getSettings();
        const title = payload.state === "ready" ? `${name} is ready to merge` : payload.state === "gave_up" ? `Autopilot needs you on ${name}` : `Autopilot stopped on ${name}`;
        const what = payload.state === "ready" ? "Autopilot: ready to merge" : payload.state === "gave_up" ? "Autopilot needs you" : "Autopilot stopped";
        if (prefs.needsSound) ting();
        if (prefs.needsNotify) notify(bannerFrom(w), what, payload.detail, payload.state === "ready" ? "happy" : "idle");
        say(`${title}. ${payload.detail ?? ""}`);
      }
    });
    // A spend cap held the queue back: said once per cap and day, with a way to the caps.
    const unBudget = listen<{ text: string }>("budget", ({ payload }) => {
      const prefs = getSettings();
      if (prefs.needsSound) ting();
      if (prefs.needsNotify) notify({ task: "Task queue" }, "The queue stopped at a spend cap", payload.text);
      say(payload.text, false, { label: "Open usage", run: () => setView("usage") });
    });
    // "Change caps" on a chat's budget stop card.
    const openUsage = () => setView("usage");
    window.addEventListener("nekotrail-open-usage", openUsage);
    return () => { un.then((f) => f()); unBudget.then((f) => f()); window.removeEventListener("nekotrail-open-usage", openUsage); };
  }, []);
  const [tab, setTab] = useState<Tab>((params.get("tab") as Tab) || "chat");
  const navRef = useRef<(d: -1 | 1) => void>(() => {});
  // Back / forward through places you've been (workspace, view, tab), like a browser. ⌘[ and ⌘].
  type Place = { view: "workspace" | "sky" | "board" | "queue" | "usage"; selected: string | null; tab: Tab };
  const history = useRef<{ stack: Place[]; at: number; moving: boolean }>({ stack: [], at: -1, moving: false });
  const [, setNavTick] = useState(0);
  const [showArchived, setShowArchived] = useState(false);
  const [settingsFor, setSettingsFor] = useState<string | null>(params.get("repoSettings")); // repo id
  const [settingsSection, setSettingsSection] = useState<"files" | undefined>(undefined); // which Repo settings tab opens first
  const [trustAsk, setTrustAsk] = useState<{ repo: string; trust?: ScriptTrust; agent?: AgentConfigTrust; answer: (run: boolean) => void } | null>(null);
  const [toast, setToast] = useState<{ text: string; error?: boolean; action?: { label: string; run: () => void } } | null>(null);
  const [discovery, setDiscovery] = useState<{ star: Star; kind: "lit" | "shining" } | null>(null);
  const [canImport, setCanImport] = useState(false);
  // The Conductor sheet: "import" brings work over and moves it out of Conductor's folder; "move"
  // only moves workspaces imported earlier. ?rescue= opens it in the preview (moving, done and
  // partial start the move straight away).
  const rescueParam = params.get("rescue");
  const [conductorSheet, setConductorSheet] = useState<null | "import" | "move">(rescueParam === "move" ? "move" : rescueParam && rescueParam !== "notice" ? "import" : null);
  // Imported Conductor workspaces still in Conductor's folder, and the one-time notice about them.
  const [outside, setOutside] = useState<string[]>([]);
  const [moveNoticeOff, setMoveNoticeOff] = useState(() => localStorage.getItem(NOTICE_KEY));
  // First run: the welcome flow's step (null once done, or for people who had repos already), the
  // coach-mark tour, and whether the first look at the database has happened (both wait for it).
  const [onboardStep, setOnboardStep] = useState<Step | null>(null);
  const [tourOpen, setTourOpen] = useState(params.has("tour"));
  const [looked, setLooked] = useState(false);
  const lookedRef = useRef(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [wsRefreshKey, setWsRefreshKey] = useState(0); // bumps only for the selected workspace's turns
  const [inject, setInject] = useState<{ text: string; nonce: number; send?: boolean } | undefined>();
  const [append, setAppend] = useState<{ text: string; nonce: number } | undefined>();
  // Hand off to another agent (⌘K, `nekotrail handoff`): the workspace, and the chat and agent when they were named.
  const [handoffReq, setHandoffReq] = useState<{ workspaceId: string; nonce: number; sessionId?: string; agent?: Agent | null; model?: string | null } | undefined>(
    params.get("handoff") ? { workspaceId: params.get("handoff")!, nonce: 1, sessionId: params.get("handoffChat") ?? undefined, agent: (params.get("handoffTo") as Agent | null) ?? null } : undefined);
  // Run and setup scripts that are alive (pty ids), the terminal tab to show, and dev-server URLs.
  const [livePtys, setLivePtys] = useState<Set<string>>(new Set());
  const [termFocus, setTermFocus] = useState<{ tab: string; nonce: number } | undefined>();
  const [localUrls, setLocalUrls] = useState<Record<string, string>>({});
  useEffect(() => {
    const unExit = listen<{ id: string }>("pty-exit", ({ payload }) =>
      setLivePtys((s) => { const n = new Set(s); n.delete(payload.id); return n; }));
    // Spot "http://localhost:3000" in run output, so ⌘⇧B can open it (through the terminals' one listener).
    const unData = ptyBus.tap((id, data) => {
      if (!id.endsWith(":run")) return;
      const m = /https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):\d+[^\s"'\x1b]*/.exec(data.replace(/\x1b\[[0-9;]*m/g, ""));
      if (m) setLocalUrls((u) => ({ ...u, [id.split(":")[0]]: m[0].replace("0.0.0.0", "localhost") }));
    });
    return () => { unExit.then((f) => f()); unData.then((f) => f()); };
  }, []);
  /** Resolves when a terminal process ends (or after 20 minutes, whichever is first); never leaves a listener behind. */
  const waitForExit = (id: string) => ptyBus.waitForExit(id, 20 * 60_000);
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
    else if (a === "checks") setTab("checks");
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
      const first = await api.agentConfigTrust(id).catch(() => null);
      if (!first) return;
      // A multi-repo task asks per repo, the main one first.
      for (const t of [first, ...(first.rest ?? [])]) await askAgentTrust(id, t);
    });
    return () => setBeforeTurn(async () => {});
  }, []);
  const askAgentTrust = async (id: string, t: AgentConfigTrust) => {
    let asking = agentAsks.current.get(t.fingerprint);
    if (!asking) {
      // One sheet at a time, and parallel sends for the same config share it.
      const before = Promise.all(agentAsks.current.values()).then(() => {});
      asking = before.then(async () => {
        const run = await new Promise<boolean>((answer) => setTrustAsk({ repo: t.repo || "this repo", agent: t, answer }));
        setTrustAsk(null);
        if (run) await api.trustAgentConfig(id, t.fingerprint, t.workspace_id).catch((e) => say(errText(e), true));
        // Agents that can't run with these off (Gemini, Copilot): "Not now" isn't kept, the next send asks again.
        else if (t.items.some((it) => it.agent === "gemini" || it.agent === "copilot")) agentAsks.current.delete(t.fingerprint);
      });
      agentAsks.current.set(t.fingerprint, asking);
    }
    await asking;
  };
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
  // Each workspace's checks dot (sidebar and the Checks tab), kept current by `checks` events.
  const [checkDots, setCheckDots] = useState<Record<string, CheckDot>>({});
  useEffect(() => {
    const load = () => api.checksStatuses().then((d) => setCheckDots(d ?? {})).catch(() => {});
    load();
    const un = listen("checks", load);
    return () => { un.then((f) => f()); };
  }, []);
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
  // The task queue: counts for the sidebar row, and the summary of what ran while you were away.
  // It shows once, when the app opens or when you come back after a while; seeing the Queue view
  // counts as having seen it.
  const viewRef = useRef(view);
  viewRef.current = view;
  useEffect(() => {
    const loadQueue = () => api.queueView().then((v) => {
      setQueueInfo({ ...queueCounts(v.tasks), paused: v.paused });
      if (viewRef.current === "queue" && document.hasFocus()) api.queueSummarySeen().catch(() => {});
    }).catch(() => {});
    let asking = false;
    const summary = async () => {
      if (asking || viewRef.current === "queue") return;
      const s = await api.queueSummary().catch(() => null);
      if (!s || s.done + s.needs_you + s.failed === 0) return;
      asking = true;
      await api.queueSummarySeen().catch(() => {});
      // A toast, not a sheet: it reports, nothing needs deciding, and the sidebar row keeps what's still going.
      say(`While you were away: ${s.text}.`, false, { label: "Open queue", run: () => setView("queue") });
      asking = false;
    };
    loadQueue();
    summary();
    // Back after 20 minutes or more away: the night's work is the first thing you see.
    let away = 0;
    const onBlur = () => { away = Date.now(); };
    const onFocus = () => { if (away && Date.now() - away > 20 * 60_000) summary(); away = 0; };
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    const un = listen("task-queue", loadQueue);
    return () => { window.removeEventListener("blur", onBlur); window.removeEventListener("focus", onFocus); un.then((f) => f()); };
  }, []);
  useEffect(() => { if (view === "queue") api.queueSummarySeen().catch(() => {}); }, [view]);

  // Open PRs waiting for your review, from the same poller (`review-requests` events).
  const [reviewRequests, setReviewRequests] = useState<ReviewRequest[]>([]);
  const [reviewBusy, setReviewBusy] = useState<string | null>(null);
  useEffect(() => {
    const un = listen<ReviewRequest[]>("review-requests", ({ payload }) => setReviewRequests(payload));
    api.reviewRequests().then((l) => setReviewRequests(l ?? [])).catch(() => {});
    // The agent's findings became draft comments: Changes reloads them, and you hear about it.
    const unF = listen<{ workspace_id: string; count: number }>("review-findings", ({ payload }) => {
      window.dispatchEvent(new CustomEvent("nekotrail-review", { detail: payload.workspace_id }));
      if (payload.workspace_id === selectedRef.current) say(`${payload.count} ${payload.count === 1 ? "finding is" : "findings are"} in Changes as drafts. Edit them, then Submit review.`, false, { label: "Show", run: () => setTab("changes") });
    });
    return () => { un.then((f) => f()); unF.then((f) => f()); };
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
  // ⌘⇧F, and the message it last opened (the chat scrolls to it).
  const [searchOpen, setSearchOpen] = useState(new URLSearchParams(window.location.search).has("search"));
  const [jump, setJump] = useState<JumpTo | undefined>();
  // An archived workspace opened from search while archived ones are hidden in the sidebar.
  const [extraWs, setExtraWs] = useState<Workspace | null>(null);
  const [focusFile, setFocusFile] = useState<{ path: string; nonce: number } | undefined>();
  const [editingTitle, setEditingTitle] = useState<string | null>(null);
  const [cloning, setCloning] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [scorecard, setScorecard] = useState(params.has("scorecard"));
  const [storage, setStorage] = useState(params.has("storage"));
  const [commandsFor, setCommandsFor] = useState<string | null>(params.get("commands")); // workspace id
  const [envFor, setEnvFor] = useState<string | null>(params.get("env")); // workspace id: the Environment sheet
  // The Rules sheet: workspace id and the pane it opens on.
  const [rulesFor, setRulesFor] = useState<{ id: string; pane: "files" | "edit" | "task" } | null>(params.get("rules") ? { id: params.get("rules")!, pane: (params.get("rulesPane") as "files" | "edit" | "task") ?? "files" } : null);
  const [terminalImport, setTerminalImport] = useState(params.has("terminalImport"));
  // Export session: the workspace, and the chat when it was opened from a chat's menu.
  const [exportFor, setExportFor] = useState<{ id: string; chat?: string } | null>(params.get("export") ? { id: params.get("export")!, chat: params.get("exportChat") ?? undefined } : null);
  useEffect(() => {
    const on = (e: Event) => setExportFor((e as CustomEvent<{ id: string; chat?: string }>).detail);
    window.addEventListener("nekotrail-export", on);
    return () => window.removeEventListener("nekotrail-export", on);
  }, []);
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
  const [settingsPane, setSettingsPane] = useState<SettingsPane | undefined>(undefined);
  // Prompts for ⌘K, loaded when it opens so repo prompts are current.
  const [palettePrompts, setPalettePrompts] = useState<PromptEntry[]>([]);
  const updater = useUpdater();

  // Which agents are working. `running` events apply straight away; a snapshot only counts when it
  // answers the newest request made after the last event (see runState.ts), so a late, older answer
  // can't turn a spinner back on or leave it stuck.
  const snapReq = useRef(new Latest());
  const wsOf = useRef(new Map<string, string>());
  const runningRef = useRef<RunningSets>({ sessions: new Set(), workspaces: new Set() });
  const setRunning = (next: RunningSets) => {
    const cur = runningRef.current;
    const sessions = sameSet(cur.sessions, next.sessions) ? cur.sessions : next.sessions;
    const workspaces = sameSet(cur.workspaces, next.workspaces) ? cur.workspaces : next.workspaces;
    runningRef.current = { sessions, workspaces };
    setRunningSessions(sessions);
    setRunningWs(workspaces);
  };
  const refresh = useCallback(async () => {
    const req = snapReq.current.begin();
    const s = await api.snapshot(showArchived);
    if (!snapReq.current.isCurrent(req)) return; // a newer request or a running event came since
    if (!lookedRef.current) {
      // Once per launch: a brand-new install starts the first-run clock and the welcome flow;
      // someone who already has repos skips both, and the tour.
      lookedRef.current = true;
      firstLook(localStorage, { repos: s.repos.length, workspaces: s.workspaces.length }, Date.now());
      setOnboardStep(startStep(localStorage.getItem(KEYS.onboarding), s.repos.length));
      setLooked(true);
    }
    setRepos(s.repos);
    setWorkspaces(s.workspaces);
    setCosts(s.costs ?? {});
    api.sections().then(setSections).catch(() => {});
    setRunning({ sessions: new Set(s.running), workspaces: new Set(s.running_workspaces) });
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

  // A turn said Done but the window never heard it stop: look again (10 s, 30 s, 60 s).
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  const watchdog = useRef<DoneWatchdog | null>(null);
  useEffect(() => {
    const w = new DoneWatchdog((sid) => runningRef.current.sessions.has(sid), () => { refreshRef.current(); });
    watchdog.current = w;
    const un = listen<{ session_id: string; event: ChatEvent }>("agent", ({ payload }) => {
      if (payload.event.kind === "done") w.done(payload.session_id);
    });
    return () => { un.then((f) => f()); w.dispose(); watchdog.current = null; };
  }, []);

  useEffect(() => {
    // Turn end and the next queued turn come a moment apart: one snapshot and one sidebar reload for both.
    let snap = 0, facts = 0;
    const un = listen<{ session_id?: string; running: boolean; workspace_id?: string; ok?: boolean; title?: string; more_queued?: boolean; needs?: string | null }>("running", ({ payload }) => {
      snapReq.current.invalidate();
      setRunning(applyRunning(runningRef.current, payload, wsOf.current));
      if (payload.session_id) watchdog.current?.ended(payload.session_id);
      clearTimeout(snap);
      snap = window.setTimeout(() => { refresh(); }, 250);
      // The first answer of a new install stops the first-run clock (local only, see Copy diagnostics).
      if (payload.workspace_id && isAnswer(payload)) markFirstRun(localStorage, "answer");
      clearTimeout(facts);
      facts = window.setTimeout(() => setRefreshKey((k) => k + 1), 500); // reload line counts and the collision radar
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
        const why: Record<string, string> = {
          plan: "The agent has a plan for you to approve",
          question: "The agent has a question for you",
          login: "The agent needs you to sign in",
          limit: "The agent hit its usage limit",
          install: "NekoTrail can't find this agent's CLI",
          error: "The agent stopped with an error",
          approval: "The agent wants your OK",
          protected: "The agent changed a protected file",
          loop: "The agent was stopped: it was going in circles",
        };
        if (prefs.needsSound) ting();
        if (prefs.needsNotify) notify(bannerFrom(w), why[payload.needs] ?? "The agent needs you", chatLine(w, payload.title));
        if (prefs.needsBounce) bounce();
        return;
      }
      if (prefs.sound && !isQuiet()) chime(payload.ok !== false);
      // The cat brings you something: tell the user when they're looking elsewhere.
      const away = document.hidden || !document.hasFocus() || selectedRef.current !== payload.workspace_id;
      if (prefs.notify === "always" || (prefs.notify === "away" && away)) {
        const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
        notify(bannerFrom(w), payload.ok === false ? "The agent stopped and needs you" : "The agent finished its turn", chatLine(w, payload.title), payload.ok === false ? "idle" : "happy");
      }
    });
    // Mid-turn requests (approve a tool call) arrive while the turn is still running.
    const unNeeds = listen<{ workspace_id: string; needs: string; title?: string }>("needs", ({ payload }) => {
      const prefs = getSettings();
      const w = workspacesRef.current.find((x) => x.id === payload.workspace_id);
      refresh();
      if (prefs.needsSound) ting();
      const away = document.hidden || !document.hasFocus() || selectedRef.current !== payload.workspace_id;
      if (payload.needs === "checks") {
        // A check that passed before fails after the agent's turn.
        if (prefs.needsNotify) notify(bannerFrom(w), "A check that passed before fails now", payload.title);
      } else if (prefs.needsNotify && away) notify(bannerFrom(w), "The agent wants your OK for a step", payload.title);
      if (prefs.needsBounce) bounce();
    });
    return () => { clearTimeout(snap); clearTimeout(facts); un.then((f) => f()); unNeeds.then((f) => f()); };
  }, [refresh]);

  // Refs so the event listener above always sees current state.
  const selectedRef = useRef(selected);
  const workspacesRef = useRef(workspaces);
  const skyRef = useRef(sky);
  const reposRef = useRef(repos);
  reposRef.current = repos;
  /** A banner's title and subtitle: the task (not the star), then "repo · branch". */
  const bannerFrom = (w: Workspace | undefined): BannerFrom => w
    ? { task: displayName(skyRef.current, w), repo: reposRef.current.find((r) => r.id === w.repo_id)?.name, branch: w.branch }
    : { task: "NekoTrail" };
  // Whether this process may alert at all (none in an e2e run, no banners from an unbundled macOS build).
  useEffect(() => { void loadNoticeMode(); }, []);
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
    // Only a change of mood (the cat falls asleep) re-renders the app, not every tick.
    const napping = (t: number) => t - lastActive.current > NAP_AFTER_MS;
    const t = setInterval(() => setNow((prev) => { const n = Date.now(); return napping(prev) === napping(n) ? prev : n; }), 30_000);
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

  const picked = workspaces.find((w) => w.id === selected) ?? (extraWs?.id === selected ? extraWs : null);
  // A linked workspace of a multi-repo task (a deep link, the palette) shows as its task.
  const ws = picked && isLinked(picked) ? mainOf(picked, workspaces) : picked;
  // Multi-repo tasks: every workspace of the open task, and the repo Changes, Checks and Ports show.
  const [repoView, setRepoView] = useState<Record<string, string>>({});
  const members = useMemo(() => (ws ? membersOf(ws, workspaces) : []), [ws?.id, ws?.state, workspaces]);
  const isTask = members.length > 1;
  // The repo picked in the switcher; else the one a link or the palette opened; else the main one.
  const viewWs = (isTask && ws ? members.find((m) => m.id === (repoView[ws.id] ?? picked?.id)) : undefined) ?? ws;
  const repoNameOf = (id: string) => repos.find((r) => r.id === id)?.name ?? "";
  // The sidebar and the board show a task once, as its main workspace, with its repos.
  const listed = useMemo(() => workspaces.filter((w) => !isLinked(w)), [workspaces]);
  const taskRepos = useMemo(() => {
    const out: Record<string, string[]> = {};
    for (const w of workspaces) if (w.group_id && w.group_id === w.id) out[w.id] = membersOf(w, workspaces).map((m) => repoNameOf(m.repo_id));
    return out;
  }, [workspaces, repos]);
  /** Line counts for the sidebar: a task's row adds up every repo. */
  const taskLines = useMemo(() => {
    const out = { ...lines };
    for (const w of workspaces) {
      if (!isLinked(w) || !lines[w.id]) continue;
      const [a, d] = out[w.group_id] ?? [0, 0];
      out[w.group_id] = [a + lines[w.id][0], d + lines[w.id][1]];
    }
    return out;
  }, [lines, workspaces]);
  /** The Checks tab's dot: the worst of the task's repos (failing, then running). */
  const taskDot = ws ? (members.map((m) => checkDots[m.id]).find((d) => d === "failed") ?? members.map((m) => checkDots[m.id]).find((d) => d && d !== "passed") ?? checkDots[ws.id]) : undefined;
  /** Text for the task's agent about another repo's files: it works from the main repo's folder. */
  const inRepo = (w: Workspace, text: string) =>
    ws && w.id !== ws.id ? `In the ${repoNameOf(w.repo_id)} repo of this task (${w.path}):\n\n${text}` : text;
  /** Changes and Checks of a multi-repo task: a repo switcher above the pane. */
  const perRepo = (body: React.ReactNode) => !isTask || !ws ? body : (
    <div className="repo-pane">
      <div className="repo-switch-bar">
        <nav className="seg repo-switch" role="tablist" aria-label="Repo">
          {members.map((m) => {
            const l = lines[m.id];
            const dot = checkDots[m.id];
            return (
              <button key={m.id} role="tab" aria-selected={viewWs?.id === m.id} className={viewWs?.id === m.id ? "on" : ""}
                onClick={() => setRepoView((v) => ({ ...v, [ws.id]: m.id }))}
                title={m.id === ws.id ? `${repoNameOf(m.repo_id)}: the agent works here` : repoNameOf(m.repo_id)}>
                {repoNameOf(m.repo_id)}
                {l && (l[0] > 0 || l[1] > 0) && <span className="num repo-switch-lines">{l[0] > 0 && <span className="add">+{l[0]}</span>}{l[0] > 0 && l[1] > 0 && " "}{l[1] > 0 && <span className="del">-{l[1]}</span>}</span>}
                {dot && dot !== "passed" && <span className={`ws-check ck-${dot}`} role="img" aria-label={dot === "failed" ? "checks failing" : "checks running"} />}
              </button>
            );
          })}
        </nav>
      </div>
      <div className="repo-pane-body">{body}</div>
    </div>
  );

  // A jump belongs to one visit: leaving its workspace forgets it, so coming back doesn't scroll again.
  useEffect(() => { if (jump && jump.session.workspace_id !== selected) setJump(undefined); }, [selected]);

  /** Open a search hit: its workspace (archived ones read-only), its chat, scrolled to the message. */
  const openHit = async (hit: SearchHit) => {
    try {
      const place = await api.searchOpen(hit.message_id);
      let w = workspaces.find((x) => x.id === place.workspace_id);
      if (!w) {
        w = (await api.snapshot(true)).workspaces.find((x) => x.id === place.workspace_id);
        if (!w) { say("That workspace is gone.", true); return; }
        setExtraWs(w);
      }
      select(w.id);
      setTab("chat");
      setJump({ sessionId: place.session.id, session: place.session, index: place.index, total: place.total, nonce: Date.now() });
    } catch (e) {
      say(errText(e), true);
    }
  };
  const star = ws ? starFor(sky, ws.name) : null;
  const starLine = (st: Star) => {
    const c = constellationOf(sky, st);
    const all = sky.stars.filter((x) => x.constellation === c.slug);
    return `${c.name} · ${all.filter((x) => lit.has(x.slug)).length} of ${all.length} stars lit`;
  };
  const repo = ws ? repos.find((r) => r.id === ws.repo_id) : null;
  // Run, Ports, Compare and Open in live in the header's ⋯ menu; only a live run script shows a Stop button.
  const paired = !!ws?.pair_id && workspaces.filter((x) => x.pair_id === ws.pair_id).length > 1;
  const runLive = !!ws && livePtys.has(`${ws.id}:run`);
  const urlShown = !!ws && !!localUrls[ws.id] && runLive;
  // Chat draws its chat tabs into the tab row, next to Changes, Terminal, Preview and Checks.
  const [chatSlot, setChatSlot] = useState<HTMLDivElement | null>(null);
  // The chat tabs are portaled in from Chat, and React sends portal events up the React tree (Chat's pane),
  // not the DOM. So a React onClick on this slot never fires: listen on the DOM node itself.
  useEffect(() => {
    if (!chatSlot) return;
    const onClick = (e: MouseEvent) => { if ((e.target as HTMLElement).closest(".chat-tab")) setTab("chat"); };
    chatSlot.addEventListener("click", onClick, true);
    return () => chatSlot.removeEventListener("click", onClick, true);
  }, [chatSlot]);

  useEffect(() => {
    if (!ws) return;
    setNotes(ws.notes);
    if (ws.unread) api.setUnread(ws.id, false).then(refresh);
  }, [ws?.id]);

  const say = (text: string, error = false, action?: { label: string; run: () => void }) => {
    setToast({ text, error, action });
    setTimeout(() => setToast((t) => (t?.text === text ? null : t)), error || action ? 8000 : 4000);
  };

  // Ports: the toolbar popover, and addresses it asks a workspace's Preview tab to show.
  const [portsOpen, setPortsOpen] = useState(() => params.has("ports"));
  const [previewShow, setPreviewShow] = useState<{ ws: string; url: string; nonce: number } | null>(null);
  const wsLabel = (id: string) => {
    const w = workspaces.find((x) => x.id === id);
    if (!w) return "another workspace";
    // A multi-repo task names its repo: "Export orders, web".
    return w.group_id ? `${displayName(sky, mainOf(w, workspaces))}, ${repoNameOf(w.repo_id)}` : displayName(sky, w);
  };
  const showInPreview = (wsId: string, url: string) => {
    select(wsId);
    setTab("preview");
    setPreviewShow({ ws: wsId, url, nonce: Date.now() });
  };
  // A Run script stopped because its port was taken: say who holds it and how to fix it.
  useEffect(() => {
    const un = listen<{ workspace_id: string; text: string; fix: string }>("port-clash", ({ payload }) => {
      say(`${payload.text} ${payload.fix.replace(/`/g, "")}`, true, { label: "Show ports", run: () => { select(payload.workspace_id); setPortsOpen(true); } });
    });
    return () => { un.then((f) => f()); };
  }, []);

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
  const select = (pickedId: string) => {
    // A multi-repo task opens on its main workspace, wherever it was picked from.
    const p = workspaces.find((x) => x.id === pickedId);
    const id = p && isLinked(p) ? p.group_id : pickedId;
    if (p && isLinked(p)) setRepoView((v) => ({ ...v, [id]: p.id }));
    const w = workspaces.find((x) => x.id === id);
    if (w?.unread && w.id !== selected) summarize(w); else setAway(null);
    setSelected(id);
    setView("workspace");
  };

  /** Review someone's PR: a read-only workspace on its head, and the agent asked for findings. */
  const startReview = async (r: ReviewRequest, agent?: Agent) => {
    const open = reviewWorkspaceFor(r, workspaces);
    if (open) return select(open.id);
    const key = `${r.repo_id}:${r.number}`;
    if (reviewBusy) return;
    setReviewBusy(key);
    try {
      const saved = (() => { try { return JSON.parse(localStorage.getItem("nekotrail.new") || "null"); } catch { return null; } })();
      const w = await api.startReview(r.repo_id, r.number, agent ?? reviewAgent(saved));
      await refresh();
      select(w.id);
      setTab("chat");
    } catch (e) {
      say(errText(e), true);
    } finally {
      setReviewBusy(null);
    }
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

  /** A task across repos: a workspace in each, one chat in the main one, setup in every repo first. */
  const newMultiTask = async (req: NewWorkspaceRequest & { repoIds: string[] }) => {
    const { repoIds, prompt, agent, model, effort, mode } = req;
    if (agent === "both") return;
    try {
      const made = await api.createMultiTask(repoIds);
      const main = made[0];
      let session: Awaited<ReturnType<typeof api.newSession>> | null = null;
      if (prompt) {
        session = await api.newSession(main.id, agent);
        await api.updateSession(session.id, "New chat", model, mode);
        if (effort) await api.setSessionEffort(session.id, effort);
      }
      setNewFor(null);
      await refresh();
      select(main.id);
      setTab("chat");
      const names = made.map((w) => repos.find((r) => r.id === w.repo_id)?.name ?? "a repo");
      say(`One task in ${names.join(", ")}, all on ${main.branch}. The agent works in ${names[0]} and reaches the others from there.`);
      // Each repo's setup runs (once you approve its commands), and the agent starts after all of them.
      const setups = await Promise.all(made.map(async (w) => {
        const scripts = await api.workspaceScripts(w.id);
        if (!scripts.setup || !await trusted(w, scripts, "setup")) return null;
        const done = waitForExit(`${w.id}:setup`);
        await startPty(`${w.id}:setup`, "setup", "setup");
        return done;
      }));
      setTab("chat");
      await Promise.all(setups.filter(Boolean));
      if (session) await api.send(session.id, prompt);
    } catch (e) {
      say(`Couldn't create the task. ${errText(e)}`, true);
    }
  };

  const newWorkspace = async (req: NewWorkspaceRequest) => {
    const { repoId, prompt, agent, model, effort, mode, from } = req;
    if (req.repoIds) return newMultiTask({ ...req, repoIds: req.repoIds });
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
      if (prompt) markFirstRun(localStorage, "sent");
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

  // The `nekotrail` terminal command and nekotrail:// links. The backend queues what they ask for
  // (so nothing is lost while the window loads) and nudges with a "remote" event; we drain the queue.
  const showWorkspace = (id: string) => { setNewFor(null); setSelected(id); setView("workspace"); };
  /** A workspace made outside the window: run its setup (asking first, as usual), then send the task. */
  const startRemote = async (job: Extract<RemoteJob, { kind: "start" }>) => {
    const snap = await api.snapshot(false);
    const w = snap.workspaces.find((x) => x.id === job.workspace_id);
    if (!w) return;
    await refresh();
    showWorkspace(w.id);
    setTab("chat");
    const scripts = await api.workspaceScripts(w.id);
    const setup = !!scripts.setup && await trusted(w, scripts, "setup");
    if (scripts.setup && !setup) say("Setup skipped. Run it later from the Terminal tab.");
    if (setup) {
      const done = waitForExit(`${w.id}:setup`);
      await startPty(`${w.id}:setup`, "setup", "setup");
      setTab("chat");
      say("Setup is running in the Terminal tab. The agent starts as soon as it finishes.");
      await done;
    }
    await api.send(job.session_id, job.prompt);
  };
  const remoteRef = useRef<(job: RemoteJob) => Promise<void>>(async () => {});
  remoteRef.current = async (job) => {
    switch (job.kind) {
      case "error":
        return say(job.text, true);
      case "focus":
        await refresh();
        return showWorkspace(job.workspace_id);
      case "repo": {
        const snap = await api.snapshot(false);
        await refresh();
        const r = snap.repos.find((x) => x.id === job.repo_id);
        if (job.added && r) say(`Added ${r.name}.`);
        // Its newest workspace, or the new-workspace panel when it has none yet.
        const newest = snap.workspaces.filter((w) => w.repo_id === job.repo_id && w.state === "active")
          .sort((a, b) => b.created_at.localeCompare(a.created_at))[0];
        if (newest) return showWorkspace(newest.id);
        setNewSource("task");
        setNewFor(job.repo_id);
        return;
      }
      case "start":
        // Not awaited: setup can take minutes, and other requests shouldn't wait behind it.
        startRemote(job).catch((e) => say(`Couldn't start the agent. ${errText(e)}`, true));
        return;
      case "confirm": {
        // A web page can open a link, so a link never starts an agent without this answer.
        const agent = agentName(job.agent);
        const facts: [string, string][] = [
          ["Repo", job.adds_repo ? `${job.repo}, new to NekoTrail` : job.repo],
          ["Folder", job.repo_path],
          ...(job.base ? [["From", job.base] as [string, string]] : []),
          ["Agent", job.plan ? `${agent}, in plan mode` : agent],
        ];
        // The editor extension asks the same question: you read the task, then start it here.
        const editor = job.from === "editor";
        if (editor) facts.push(...taskFacts(job.prompt));
        const who = editor ? "Your editor" : "A link";
        const go = await ask({
          title: `Start ${agent} from ${editor ? "your editor" : "a link"}?`,
          body: job.adds_repo
            ? `${who} asks for a new workspace, and adds this repo to NekoTrail first. The agent works on the task below.`
            : `${who} asks for a new workspace. The agent works on the task below.`,
          facts,
          quote: job.prompt,
          footer: editor
            ? "Check the task and the code in it. Nothing runs until you press Start agent."
            : "Only start it if you opened this link yourself and you trust the task.",
          ok: "Start agent",
        });
        await api.linkConfirm(job.token, go).catch((e) => say(errText(e), true));
        return;
      }
      case "handoff":
        // `nekotrail handoff`: the brief is shown first; nothing is sent until you press Hand off.
        await refresh();
        showWorkspace(job.workspace_id);
        setTab("chat");
        setHandoffReq({ workspaceId: job.workspace_id, nonce: Date.now(), sessionId: job.session_id, agent: job.agent, model: job.model });
        return;
      case "draft":
        // From the editor extension: the text waits in the message box until you press Send.
        await refresh();
        showWorkspace(job.workspace_id);
        setTab("chat");
        setAppend({ text: job.text, nonce: Date.now() });
        say("Your selection is in the message box. Add anything else and send.");
        return;
    }
  };
  useEffect(() => {
    // One at a time, so link questions never stack on top of each other.
    let chain = Promise.resolve();
    const drain = () => {
      chain = chain.then(async () => {
        const jobs = (await api.remoteTake().catch(() => null)) ?? [];
        for (const job of jobs) await remoteRef.current(job).catch((e) => say(errText(e), true));
      });
    };
    drain();
    const un = listen("remote", drain);
    return () => { un.then((f) => f()); };
  }, []);

  const archive = async (opts?: { auto?: boolean; id?: string }) => {
    const target = opts?.id ? workspaces.find((w) => w.id === opts.id) : ws;
    if (!target) return;
    const name = nameInText(sky, target);
    // A multi-repo task is archived as a whole: every repo's workspace goes, every branch stays.
    const repoCount = membersOf(target, workspaces).length;
    try {
      await api.archiveWorkspace(target.id);
    } catch (e) {
      if (errText(e) !== "DIRTY") return say(errText(e), true);
      if (opts?.auto) return say(`The PR for ${name} merged, but it has uncommitted changes, so it stays open. Commit or discard them, then archive it.`);
      const go = await ask({
        title: `Archive ${name} with uncommitted changes?`,
        body: repoCount > 1
          ? `This task's ${repoCount} workspaces are archived together. The branch ${target.branch} is kept in every repo, but uncommitted work in their folders is lost.`
          : `The branch ${target.branch} is kept, but uncommitted work in its folder is lost.`,
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
    say(repoCount > 1 ? `Archived ${name} in all ${repoCount} repos. Branch ${target.branch} is kept in each.` : `Archived ${name}. Branch ${target.branch} is kept.`, false, {
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

  const importConductor = async () => setConductorSheet("import");
  useEffect(() => { api.importedOutside().then(setOutside).catch(() => {}); }, [workspaces.length, repos.length]);
  const showMoveNotice = noticeShown(outside.length, moveNoticeOff);
  const dismissMoveNotice = () => { localStorage.setItem(NOTICE_KEY, "dismissed"); setMoveNoticeOff("dismissed"); };
  const conductorChanged = () => { refresh(); api.importedOutside().then(setOutside).catch(() => {}); };

  const terminalImported = async (r: TerminalReport) => {
    setTerminalImport(false);
    await refresh();
    const parts = [
      r.repos ? `${r.repos} ${r.repos === 1 ? "repo" : "repos"}` : "",
      r.workspaces ? `${r.workspaces} ${r.workspaces === 1 ? "workspace" : "workspaces"}` : "",
      `${r.chats} ${r.chats === 1 ? "chat" : "chats"}`,
    ].filter(Boolean);
    const last = parts.pop();
    const what = parts.length ? `${parts.join(", ")} and ${last}` : last;
    say(`Imported ${what}.${r.skipped ? ` ${r.skipped} were already here or can't be used.` : ""}${r.problems.length ? ` Not imported: ${r.problems.join(" ")}` : ""}`, r.problems.length > 0);
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
    const ram = await api.resourceUsage().catch(() => null);
    const { getVersion } = await import("@tauri-apps/api/app");
    const v = await getVersion().catch(() => "unknown");
    const text = [
      `NekoTrail ${v}`,
      IS_MAC
        ? `macOS: ${navigator.userAgent.match(/Mac OS X [\d_]+/)?.[0]?.replace(/_/g, ".") ?? "unknown"}`
        : `Linux: ${navigator.platform || "unknown"}`,
      `git: ${d?.git.version || "missing"}`,
      `gh: ${d?.gh.installed ? (d.gh.ok ? "signed in" : "not signed in") : "missing"}`,
      ...AGENTS.map((a) => `${a}: ${d?.[a]?.version || "missing"}`),
      `repos: ${repos.length}, workspaces: ${workspaces.filter((w) => w.state === "active").length} active, ${workspaces.filter((w) => w.state === "archived").length} archived`,
      `memory: ${ram ? `${ram.total_mb} MB` : "unknown"}`,
      describeFirstRun(readFirstRun(localStorage)),
    ].join("\n");
    await navigator.clipboard.writeText(text);
    say("Copied diagnostics. Paste them into your GitHub issue.");
  };

  // ---------- first run ----------
  const goStep = (s: Step | "done") => {
    if (onboardStep === "agents" && s !== "agents") markFirstRun(localStorage, "agents");
    localStorage.setItem(KEYS.onboarding, s);
    setOnboardStep(s === "done" ? null : s);
  };
  // A repo came in (added, cloned, imported): the clock notes it, and "Your code" moves on to "Try it".
  const repoCount = useRef<number | null>(null);
  useEffect(() => {
    if (!looked) return;
    const before = repoCount.current;
    repoCount.current = repos.length;
    if (before === null || repos.length <= before) return;
    markFirstRun(localStorage, "repo");
    const next = stepAfterRepos(onboardStep, before, repos.length);
    if (next !== onboardStep && next) goStep(next);
  }, [repos.length, looked]);
  /** "Try it": a workspace in Plan mode with the suggested task, then the flow is done. */
  const startFirstTask = async ({ repoId, agent, prompt }: StartRequest) => {
    goStep("done");
    await newWorkspace({ repoId, prompt, agent, model: null, effort: null, mode: "plan" });
  };
  // The tour shows by itself once, when the first workspace is open and nothing else is on top.
  // The "You lit a star" card plays first: the delight, then the tour.
  const tourBusy = !!discovery || !!trustAsk || palette || !!newFor || settingsOpen || searchOpen || terminalImport || !!conductorSheet || cloning || !!settingsFor;
  const tourReady = view === "workspace" && !!ws && ws.state === "active" && !isHistory(ws);
  useEffect(() => {
    if (!looked || tourOpen || !shouldAutoTour(localStorage, { workspaceOpen: tourReady, busy: tourBusy })) return;
    // Let the workspace paint first, so the coach marks find what they point at.
    const t = setTimeout(() => { markTourSeen(localStorage); setTab("chat"); setTourOpen(true); }, 600);
    return () => clearTimeout(t);
  }, [looked, tourReady, tourBusy, tourOpen]);
  const showTour = () => {
    const open = ws && ws.state === "active" && !isHistory(ws) ? ws : listed.find((w) => w.state === "active" && !isHistory(w));
    if (!open) return say(keyHint("The tour points at the parts of a workspace. Create one first, then ⌘K, Show the tour."));
    if (open.id !== ws?.id) select(open.id);
    setView("workspace");
    setTab("chat");
    markTourSeen(localStorage);
    setTourOpen(true);
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
    if (ws && ws.state === "active" && !isHistory(ws)) {
      const here = nameOf(ws);
      c.push(
        { id: "tab-chat", group: here, label: "Show chat", hint: keyHint("⌘1"), icon: <MessageSquare size={15} />, run: () => setTab("chat") },
        { id: "tab-changes", group: here, label: "Show changes", hint: keyHint("⌘2"), icon: <FilePlus2 size={15} />, run: () => setTab("changes") },
        { id: "tab-term", group: here, label: "Show terminal", hint: keyHint("⌘3"), icon: <SquareTerminal size={15} />, run: () => setTab("terminal") },
        { id: "ports", group: here, label: "Show ports", keywords: "port localhost listening free kill server clash", icon: <Network size={15} />, run: () => setPortsOpen(true) },
        { id: "tab-preview", group: here, label: "Show preview", hint: keyHint("⌘4"), keywords: "browser localhost app web annotate screenshot", icon: <AppWindow size={15} />, run: () => setTab("preview") },
        { id: "tab-checks", group: here, label: "Show checks", hint: keyHint("⌘5"), keywords: "tests lint typecheck tsc failing ci", icon: <ListChecks size={15} />, run: () => setTab("checks") },
        { id: "run-checks", group: here, label: "Run checks", keywords: "tests lint typecheck", icon: <ListChecks size={15} />, run: () => { setTab("checks"); api.checksRun(ws.id).catch((e) => say(errText(e), true)); } },
        { id: "tab-notes", group: here, label: "Show notes", icon: <NotebookPen size={15} />, run: () => setTab("notes") },
        ...((repo?.prefs?.actions ?? []).map(([n, c]) => ({ id: `act-${n}`, group: here, label: `Run ${n}`, hint: c, keywords: `action script ${c}`, icon: <Play size={15} />, run: () => runAction(n) }))),
        { id: "run", group: here, label: "Run the app", keywords: "script dev start", icon: <Play size={15} />, run: () => runScript() },
        { id: "open-editor", group: here, label: "Open in editor", hint: keyHint("⌘O"), keywords: "cursor vscode code", icon: <Code2 size={15} />, run: () => openInEditor() },
        { id: "environment", group: here, label: "Environment", keywords: "env variables .env port path secrets login shell", icon: <VariableIcon size={15} />, run: () => setEnvFor(ws.id) },
        { id: "rules", group: here, label: "Project rules", keywords: "agents.md claude.md gemini.md copilot instructions cursor rules sync", icon: <ScrollText size={15} />, run: () => setRulesFor({ id: ws.id, pane: "files" }) },
        { id: "rules-edit", group: here, label: "Edit rules", keywords: "agents.md claude.md markdown", icon: <ScrollText size={15} />, run: () => setRulesFor({ id: ws.id, pane: "edit" }) },
        { id: "rules-task", group: here, label: "Rules for this task", keywords: "workspace note first prompt instructions", icon: <ScrollText size={15} />, run: () => setRulesFor({ id: ws.id, pane: "task" }) },
        { id: "handoff", group: here, label: "Hand off to another agent", keywords: "handoff switch agent codex gemini claude continue take over usage limit stuck brief", icon: <ArrowRightLeft size={15} />, run: () => { setTab("chat"); setHandoffReq({ workspaceId: ws.id, nonce: Date.now() }); } },
        { id: "export", group: here, label: "Export session", keywords: "share report markdown html transcript summary pr description", icon: <Share size={15} />, run: () => setExportFor({ id: ws.id }) },
        { id: "commands", group: here, label: "Show commands the agent ran", keywords: "log history shell bash terminal audit", icon: <SquareTerminal size={15} />, run: () => setCommandsFor(ws.id) },
        { id: "open-finder", group: here, label: IS_MAC ? "Show in Finder" : "Show in Files", icon: <FolderOpen size={15} />, run: () => openInEditor("finder") },
        { id: "pr", group: here, label: "Go to PR actions", keywords: "github pull request create review", icon: <GitPullRequest size={15} />, run: () => document.querySelector<HTMLElement>(".prbar button, .prbar a")?.focus() },
        { id: "pin", group: here, label: ws.pinned ? "Unpin workspace" : "Pin workspace to top", icon: <Pin size={15} />, run: () => api.setPinned(ws.id, !ws.pinned).then(refresh) },
        { id: "archive", group: here, label: "Archive workspace", hint: keyHint("⌘⇧A"), keywords: "close done remove", icon: <Archive size={15} />, run: () => archive() },
      );
      // Saved prompts go into this workspace's chat box, blanks first. Claude Code's own commands
      // stay in the / menu of Claude chats.
      for (const p of palettePrompts.filter((x) => x.scope !== "claude")) {
        c.push({
          id: `prompt-${p.name}`, group: "Prompts", label: `Insert /${p.name}`, hint: p.description || (p.scope === "repo" ? "Repo prompt" : undefined),
          keywords: `prompt snippet template ${p.text.slice(0, 120)}`, icon: <BookText size={15} />,
          run: () => { setTab("chat"); requestAnimationFrame(() => window.dispatchEvent(new CustomEvent("nekotrail-insert-prompt", { detail: p.name }))); },
        });
      }
    }
    c.push(
      { id: "new", group: "Create", label: "Create workspace", hint: keyHint("⌘N"), keywords: "new worktree branch task", icon: <Plus size={15} />, run: () => openNew() },
      { id: "new-pr", group: "Create", label: "Create workspace from a PR", hint: keyHint("⌘⇧N"), keywords: "new pull request", icon: <GitPullRequest size={15} />, run: () => openNew("pr") },
      { id: "new-issue", group: "Create", label: "Create workspace from a GitHub issue", keywords: "new", icon: <GitPullRequest size={15} />, run: () => openNew("issue") },
      { id: "new-linear", group: "Create", label: "Create workspace from a Linear issue", keywords: "new linear ticket", icon: <GitPullRequest size={15} />, run: () => openNew("linear") },
      { id: "new-stack", group: "Create", label: "Create workspace stacked on another", keywords: "new stacked pr branch depends", icon: <SquareStack size={15} />, run: () => openNew("stack", ws?.id) },
      { id: "clone", group: "Create", label: "Clone repo from GitHub", keywords: "add repo", icon: <Download size={15} />, run: () => setCloning(true) },
      { id: "update", group: "App", label: "Check for updates", keywords: "upgrade new version release download", icon: <Download size={15} />, run: () => updater.checkNow(true) },
        { id: "sponsor", group: "App", label: "Sponsor NekoTrail", keywords: "donate support github sponsors fund", icon: <Heart size={15} />, run: () => openUrl("https://github.com/sponsors/Harshil-Jani").catch((e) => say(errText(e), true)) },
        { id: "coffee", group: "App", label: "Buy me a coffee", keywords: "donate support sponsor coffee tip buymeacoffee", icon: <Coffee size={15} />, run: () => openUrl("https://buymeacoffee.com/harshiljani").catch((e) => say(errText(e), true)) },
      { id: "doctor", group: "App", label: "Check setup (git, gh, Claude, Codex, Gemini, OpenCode)", keywords: "doctor install login", icon: <Gear size={15} />, run: () => { setSelected(null); setView("workspace"); } },
      { id: "add-repo", group: "Create", label: "Add local repo", icon: <FolderOpen size={15} />, run: addRepo },
    );
    for (const r of reviewRequests) {
      const open = reviewWorkspaceFor(r, workspaces);
      const rn = repos.find((x) => x.id === r.repo_id)?.name ?? "";
      c.push({
        id: `review-${r.repo_id}-${r.number}`, group: "Review requests", label: open ? `Open your review of #${r.number}: ${r.title}` : `Review #${r.number}: ${r.title}`,
        hint: `@${r.author.login}${rn ? ` · ${rn}` : ""}`, keywords: `pr pull request review requested ${r.author.login} ${rn} ${r.headRefName}`,
        icon: <Eye size={15} />, run: () => startReview(r),
      });
    }
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
      { id: "search", group: "Go", label: "Search every chat", hint: keyHint("⌘⇧F"), keywords: "find history messages prompts commands files grep", icon: <Search size={15} />, run: () => setSearchOpen(true) },
      { id: "next", group: "Go", label: "Go to next workspace with news", hint: keyHint("⌘⌥L"), icon: <Sparkles size={15} />, run: nextUnread },
      { id: "tour", group: "App", label: "Show the tour", keywords: "help onboarding guide tips intro walkthrough", icon: <Sparkles size={15} />, run: showTour },
      { id: "diagnostics", group: "App", label: "Copy diagnostics for a bug report", keywords: "debug support version", icon: <Gear size={15} />, run: copyDiagnostics },
      { id: "storage", group: "App", label: "Open storage and clean build folders", keywords: "disk space clean node_modules venv", icon: <FolderOpen size={15} />, run: () => setStorage(true) },
      { id: "scorecard", group: "Go", label: "Open agent scorecard", keywords: "stats compare claude codex gemini success cost", icon: <Crown size={15} />, run: () => setScorecard(true) },
      { id: "board", group: "Go", label: "Open board", hint: keyHint("⌘⇧K"), keywords: "kanban dashboard status columns", icon: <Kanban size={15} />, run: () => setView("board") },
      { id: "queue", group: "Go", label: "Open queue", keywords: "task queue overnight later batch line up sleep", icon: <ListOrdered size={15} />, run: () => setView("queue") },
      { id: "usage", group: "Go", label: "Open usage", keywords: "spend cost tokens budget cap limit money dollars csv export", icon: <Gauge size={15} />, run: () => setView("usage") },
      { id: "queue-add", group: "Create", label: "Add task to queue", keywords: "queue later overnight batch line up sleep", icon: <ListPlus size={15} />, run: () => { setView("queue"); setQueueAdd((n) => n + 1); } },
      queueInfo.paused
        ? { id: "queue-resume", group: "App", label: "Resume the queue", keywords: "task queue start", icon: <Play size={15} />, run: () => api.queueSetPaused(false).catch((e) => say(errText(e), true)) }
        : { id: "queue-pause", group: "App", label: "Pause the queue", keywords: "task queue stop hold", icon: <Pause size={15} />, run: () => api.queueSetPaused(true).then(() => say("Paused the queue. Running tasks finish; nothing new starts.")).catch((e) => say(errText(e), true)) },
      { id: "group-by", group: "Go", label: groupBy === "repo" ? "Group sidebar by status" : "Group sidebar by repo", keywords: "sidebar group sort status repo", icon: <ListChecks size={15} />, run: () => { const g = groupBy === "repo" ? "status" : "repo"; setGroupBy(g); saveSettings({ ...getSettings(), groupBy: g }); } },
      { id: "archived", group: "Go", label: showArchived ? "Hide archived workspaces" : "Show archived workspaces", keywords: "archive restore old", icon: <Archive size={15} />, run: () => { setSidebarHidden(() => false); setShowArchived((v) => !v); } },
      { id: "new-section", group: "Create", label: "Add sidebar section", keywords: "new group folder", icon: <Plus size={15} />, run: () => { setSidebarHidden(() => false); window.dispatchEvent(new Event("nekotrail-new-section")); } },
      { id: "sky", group: "Go", label: "Open night sky", keywords: "stars constellations progress", icon: <Moon size={15} />, run: () => setView("sky") },
      { id: "archived", group: "Go", label: showArchived ? "Hide archived workspaces" : "Show archived workspaces", icon: <ArchiveRestore size={15} />, run: () => setShowArchived((v) => !v) },
      { id: "settings", group: "App", label: "Open settings", hint: keyHint("⌘,"), keywords: "preferences notifications sound", icon: <Gear size={15} />, run: () => { setSettingsPane(undefined); setSettingsOpen(true); } },
      { id: "about", group: "App", label: "About NekoTrail", keywords: "version license licence open source third-party privacy credits", icon: <Gear size={15} />, run: () => { setSettingsPane("about"); setSettingsOpen(true); } },
      { id: "prompts", group: "App", label: "Manage prompts", keywords: "prompt library snippets saved templates import export", icon: <BookText size={15} />, run: () => { setSettingsPane("prompts"); setSettingsOpen(true); } },

    );
    for (const r of repos) c.push({ id: `filter-${r.id}`, group: "Go", label: `Show only ${r.name}`, keywords: "filter project repo", icon: <FolderOpen size={15} />, run: () => saveSettings({ ...getSettings(), repoFilter: [r.id] }) });
    if (getSettings().repoFilter.length) c.push({ id: "filter-all", group: "Go", label: "Show all repos", keywords: "filter clear project", icon: <FolderOpen size={15} />, run: () => saveSettings({ ...getSettings(), repoFilter: [] }) });
    for (const r of repos) c.push({ id: `scripts-${r.id}`, group: "App", label: `Edit ${r.name} setup and run scripts`, icon: <Gear size={15} />, run: () => setSettingsFor(r.id) });
    if (canImport) c.push({ id: "import", group: "App", label: "Import from Conductor", keywords: "move worktrees conductor folder", icon: <Download size={15} />, run: importConductor });
    if (outside.length) c.push({ id: "conductor-move", group: "App", label: `Move ${outside.length === 1 ? "1 workspace" : `${outside.length} workspaces`} out of Conductor's folder`, keywords: "conductor import relocate worktree keep safe", icon: <FolderInput size={15} />, run: () => setConductorSheet("move") });
    c.push({ id: "import-terminal", group: "App", label: "Import terminal sessions", keywords: "claude code codex gemini history cli", icon: <SquareTerminal size={15} />, run: () => setTerminalImport(true) });
    return c;
  };

  // Shortcuts: ⌘1 / ⌘2 / ⌘3 show chat, changes, terminal (like Finder's view modes); ⌃1 to ⌃9 open
  // the sidebar's workspaces. Conductor's other keys still work, so muscle memory carries over: ⌘K palette,
  // ⌘N / ⌘⇧N new (from PR), ⌘⌥↑/↓ previous/next, ⌘⌥L next with news, ⌘J terminal, ⌘⇧D changes,
  // ⌘L chat, ⌘B sidebar, ⌘O open in, ⌘P files, ⌘R run, ⌘⇧A archive, ⌘, settings.
  useEffect(() => {
    const sidebarRows = () => Array.from(document.querySelectorAll<HTMLButtonElement>(".sidebar .ws:not(.smart)"));
    const onKey = (e: KeyboardEvent) => {
      if (isWorkspaceDigit(e)) {
        if ((e.target as HTMLElement).closest?.(".xterm")) return; // the shell owns ⌃ (Linux: Alt) keys in the terminal
        e.preventDefault();
        sidebarRows()[Number(e.key) - 1]?.click();
        return;
      }
      if (!mod(e)) return;
      const k = e.key.toLowerCase();
      const go = (fn: () => void) => { e.preventDefault(); fn(); };
      if (k === "k" && !e.shiftKey) return go(() => setPalette((p) => !p));
      if (k === "f" && e.shiftKey) return go(() => { setPalette(false); setSearchOpen((o) => !o); });
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
      if (/^[1-5]$/.test(e.key) && !e.altKey && !e.shiftKey && ws.state === "active" && !isHistory(ws)) return go(() => setTab((["chat", "changes", "terminal", "preview", "checks"] as const)[Number(e.key) - 1]));
      if (k === "j" && !e.shiftKey) return go(() => setTab((t) => (t === "terminal" ? "chat" : "terminal")));
      if (k === "d" && e.shiftKey) return go(() => setTab("changes"));
      if (e.altKey && !e.shiftKey && e.code === "KeyP") return go(() => setTab((t) => (t === "preview" ? "chat" : "preview")));
      if (k === "l" && !e.shiftKey && !e.altKey) return go(() => { setTab("chat"); requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus()); });
      if (k === "a" && e.shiftKey) return go(() => archive());
      if (e.key === "Backspace" && e.shiftKey) return go(() => { for (const id of runningSessions) api.stop(id); });
      if (k === "o" && isOpenIn(e)) return go(() => openInEditor());
      if (k === "r" && !e.shiftKey) return go(() => runScript({ stay: tab === "preview" }));
      if (k === "b" && e.shiftKey) return go(openLocal);
      if (k === "p" && !e.shiftKey) return go(() => api.workspaceFiles(ws.id).then(setQuickOpen).catch((er) => say(errText(er), true)));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ws, repos, lit, sky, workspaces, selected, livePtys, localUrls, runningSessions, tab]);

  const settingsRepo = repos.find((r) => r.id === settingsFor);
  useEffect(() => {
    if (!palette || !ws || ws.state !== "active") return setPalettePrompts([]);
    loadPrompts({ id: ws.id }).then(setPalettePrompts).catch(() => setPalettePrompts([]));
  }, [palette, ws?.id]);

  return (
    <div className={`app ${resizing ? "resizing" : ""} ${sidebarHidden ? "no-sidebar" : ""}`}>
      <Sidebar
        onSearchChats={(q) => { sessionStorage.setItem("nekotrail.search", q); setPalette(false); setSearchOpen(true); }}
        repos={repos}
        workspaces={listed}
        taskRepos={taskRepos}
        sky={sky}
        lit={lit.size}
        me={me}
        running={runningWs}
        selected={view === "workspace" ? ws?.id ?? selected : null}
        showArchived={showArchived}
        onSelect={select}
        onNewWorkspace={setNewFor}
        onAddRepo={addRepo}
        onClone={() => setCloning(true)}
        firstRun={!!onboardStep}
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
        lines={taskLines}
        checks={checkDots}
        collisions={collisions}
        usage={usage}
        sections={sections}
        onChanged={refresh}
        onRename={(id) => { select(id); const w = workspaces.find((x) => x.id === id); if (w) setEditingTitle(displayName(sky, w)); }}
        onArchive={(id) => archive({ id })}
        onBoard={() => setView("board")}
        boardOpen={view === "board"}
        onSettings={() => setSettingsOpen(true)}
        queue={queueInfo}
        queueOpen={view === "queue"}
        onQueue={() => setView("queue")}
        usageOpen={view === "usage"}
        onUsage={() => setView("usage")}
        lowDiskMb={freeMb !== null && freeMb < 3072 ? freeMb : null}
        onStorage={() => setStorage(true)}
        canBack={history.current.at > 0}
        canForward={history.current.at < history.current.stack.length - 1}
        onBack={() => goHistory(-1)}
        onForward={() => goHistory(1)}
        onToggleSidebar={() => setSidebarHidden((h) => !h)}
        reviewRequests={reviewRequests}
        reviewBusy={reviewBusy}
        onReview={startReview}
      />
      {sidebarHidden && (
        <button className="icon sidebar-toggle floating" onClick={() => setSidebarHidden(() => false)} title={keyHint("Show sidebar (⌘B)")} aria-label="Show sidebar">
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
            workspaces={listed} repos={repos} sky={sky} prStates={prStates} lines={lines} running={runningWs} costs={costs}
            repoFilter={getSettings().repoFilter}
            onOpen={select}
            onSetStatus={(id, st) => api.setWorkspaceStatus(id, st).then(refresh)}
            onNew={() => openNew()}
          />
        )}
        {view === "queue" && (
          <Queue
            repos={repos}
            initialRepo={ws?.repo_id ?? getSettings().repoFilter[0]}
            addNonce={queueAdd}
            onOpen={select}
            onSay={(text, bad) => say(text, bad)}
            onUsage={() => setView("usage")}
          />
        )}
        {view === "usage" && <Usage repos={repos} onSay={(text, bad) => say(text, bad)} />}
        {view === "sky" && <NightSky sky={sky} lit={lit} shining={shining} workspaces={workspaces} onOpen={select} onNew={() => openNew()} hasRepos={repos.length > 0} />}

        {view === "workspace" && !ws && onboardStep && (
          <FirstRun
            step={onboardStep} onStep={goStep} repos={repos} canImport={canImport}
            mood={mood} purring={purring} onPet={pet}
            onAddRepo={addRepo} onClone={() => setCloning(true)} onImportConductor={importConductor}
            onImportTerminal={() => setTerminalImport(true)} onStart={startFirstTask}
          />
        )}
        {view === "workspace" && !ws && looked && !onboardStep && (
          <Welcome
            me={me} repos={repos} mood={mood} purring={purring} onPet={pet}
            onAddRepo={addRepo} onClone={() => setCloning(true)} onNew={() => openNew()}
            canImport={canImport} onImport={importConductor} refreshKey={workspaces.length}
            moveNotice={showMoveNotice ? <MoveNotice count={outside.length} onMove={() => setConductorSheet("move")} onDismiss={dismissMoveNotice} /> : null}
            onImportTerminal={() => setTerminalImport(true)}
          />
        )}

        {view === "workspace" && ws && (
          <>
            <header className="ws-header" data-tauri-drag-region>
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
                // Where you are: repo › branch. The task's name is the selected sidebar row; it and the rest are in the tooltip.
                <nav className="crumbs" aria-label="Workspace" data-tauri-drag-region onDoubleClick={() => setEditingTitle(displayName(sky, ws))}
                  title={`${displayName(sky, ws)}${subName(sky, ws) ? ` (${subName(sky, ws)})` : ""}\n${isTask ? `One task across ${members.map((m) => repoNameOf(m.repo_id)).join(", ")}. The agent works in ${repo?.name}.` : repo?.root_path ?? ""}\n${isHistory(ws) ? `${ws.branch}, in your main checkout` : `${ws.branch} into ${ws.target_branch}`}\n${ws.title_source === "auto" ? "Named after the first message. " : ws.title_source === "issue" ? "Named after the Linear issue. " : ws.title_source === "pr" ? "Named after the PR. " : ""}Double-click to rename.`}>
                  <span className="crumb">{isTask ? members.map((m) => repoNameOf(m.repo_id)).join(", ") : repo?.name}</span>
                  <ChevronRight size={13} className="crumb-sep" aria-hidden />
                  <span className="crumb crumb-branch">{ws.branch}</span>
                </nav>
              )}
              {ws.state === "active" && !isTask && !ws.review_pr && !isHistory(ws) ? (
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
              ) : <span className="spacer" />}
              <div className="ws-actions">
                {ws.state === "active" && isHistory(ws) ? (<>
                  <span className="chip read-only" title="Chats you ran in this repo's main checkout from a terminal. NekoTrail never changes that folder."><SquareTerminal size={11} />History, read only</span>
                  <button onClick={() => openNew()}><Plus size={16} />New workspace</button>
                  <button className="icon" aria-label="Archive" title={`Hide this history (${keyHint("⌘⇧A")}). Your main checkout stays as it is.`} onClick={() => archive()}><Archive size={15} /></button>
                </>) : ws.state === "archived" ? (<>
                  <span className="chip read-only" title="Archived workspaces open read-only. Its branch is kept."><Archive size={11} />Archived, read only</span>
                  <button className="primary" onClick={() => api.restoreWorkspace(ws.id).then(refresh).catch((e) => say(errText(e), true))}>
                    <ArchiveRestore size={16} />Restore workspace
                  </button>
                </>) : (
                  <>
                    {/* A live run script is the one thing worth a button here: it holds a port until you stop it. */}
                    {runLive && <button className="quiet tb-stop" onClick={() => runScript()} title={keyHint("Stop the run script (⌘R)")}><Square size={11} />Stop</button>}
                    <span className="tb-ports-wrap">
                      {portsOpen && <PortsPopover selected={viewWs?.id ?? ws.id} task={members.map((m) => m.id)} label={wsLabel} repoOf={(id) => repoNameOf(workspaces.find((w) => w.id === id)?.repo_id ?? "")} onPreview={showInPreview} onSay={say} onClose={() => setPortsOpen(false)} />}
                    </span>
                    <span className="git-menu">
                      <button className="icon tb-more" aria-label="More actions" title="Run, open, ports and more" aria-expanded={moreOpen} onClick={() => setMoreOpen((o) => !o)}><Ellipsis size={18} /></button>
                      {moreOpen && (
                        <span className="menu ws-menu" role="menu" onMouseLeave={() => setMoreOpen(false)}>
                          {runLive
                            ? <button role="menuitem" onClick={() => { setMoreOpen(false); runScript(); }}><Square size={15} /><span>Stop the run script</span><kbd>{keyHint("⌘R")}</kbd></button>
                            : <button role="menuitem" onClick={() => { setMoreOpen(false); runScript({ stay: tab === "preview" }); }}><Play size={15} /><span>Run</span><kbd>{keyHint("⌘R")}</kbd></button>}
                          {(repo?.prefs?.actions ?? []).map(([n, c]) => (
                            <button key={n} role="menuitem" onClick={() => { setMoreOpen(false); runAction(n); }} title={c}><Play size={15} /><span>{n}</span></button>
                          ))}
                          {urlShown && <button role="menuitem" onClick={() => { setMoreOpen(false); openLocal(); }}><ExternalLink size={15} /><span>Open {localUrls[ws.id].replace(/^https?:\/\//, "")}</span><kbd>{keyHint("⌘⇧B")}</kbd></button>}
                          <button role="menuitem" className="tb-ports" onClick={() => { setMoreOpen(false); setPortsOpen(true); }}><Network size={15} /><span>Ports</span></button>
                          {paired && <button role="menuitem" onClick={() => { setMoreOpen(false); setComparing(ws.pair_id); }}><Columns2 size={15} /><span>Compare both agents</span></button>}
                          {openTargets.length > 0 && <>
                            <div className="menu-sep" />
                            <div className="menu-label">Open in</div>
                            {openTargets.map((t) => (
                              <button key={t.id} role="menuitem" onClick={() => { setMoreOpen(false); api.openIn(ws.id, t.id).catch((er) => say(errText(er), true)); }}><AppWindow size={15} /><span>{t.name}</span></button>
                            ))}
                          </>}
                          <div className="menu-sep" />
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setEditingTitle(displayName(sky, ws)); }}><Pencil size={15} /><span>Rename</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); api.setPinned(ws.id, !ws.pinned).then(refresh); }}>{ws.pinned ? <PinOff size={15} /> : <Pin size={15} />}<span>{ws.pinned ? "Unpin" : "Pin to top"}</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setTab("notes"); }}><NotebookPen size={15} /><span>Open notes</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setRulesFor({ id: ws.id, pane: "files" }); }}><ScrollText size={15} /><span>Rules</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setEnvFor(ws.id); }}><VariableIcon size={15} /><span>Environment</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setCommandsFor(ws.id); }}><SquareTerminal size={15} /><span>Commands the agent ran</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); setExportFor({ id: ws.id }); }}><Share size={15} /><span>Export session…</span></button>
                          <button role="menuitem" onClick={() => { setMoreOpen(false); window.dispatchEvent(new CustomEvent("nekotrail-rename-branch", { detail: ws.id })); }}><GitBranch size={15} /><span>Rename branch</span></button>
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
                          <button role="menuitem" onClick={() => { setMoreOpen(false); archive(); }}><Archive size={15} /><span>Archive</span><kbd>{keyHint("⌘⇧A")}</kbd></button>
                        </span>
                      )}
                    </span>
                  </>
                )}
              </div>
            </header>

            {/* One row of tabs: the chats (Chat puts them here) on the left, the other views on the right. */}
            {ws.state === "active" && !isHistory(ws) && (
              <div className={`ws-tabbar${tab === "chat" ? " on-chat" : ""}`}>
                <div className="ws-chats" ref={setChatSlot} />
                <nav className="tabs view-tabs" role="tablist" aria-label="Views">
                  {TABS.filter((t) => t.id !== "chat" && (t.id !== "notes" || tab === "notes" || ws.notes.trim())).map((t) => (
                    <button key={t.id} role="tab" data-tour={t.id === "changes" ? "changes" : undefined} aria-selected={tab === t.id} className={`tab ${tab === t.id ? "active" : ""}`} onClick={() => setTab(t.id)}
                      title={`${TAB_KEYS[t.id] ? `${t.label} (${TAB_KEYS[t.id]})` : t.label}${t.id === "changes" && lines[ws.id] ? `: +${lines[ws.id][0]} -${lines[ws.id][1]}` : ""}`}>
                      {t.label}
                      {t.id === "checks" && taskDot && taskDot !== "passed" && (
                        <span className={`ws-check ck-${taskDot}`} role="img" aria-label={taskDot === "failed" ? "failing" : "running"} />
                      )}
                    </button>
                  ))}
                </nav>
              </div>
            )}

            {ws.state === "active" && ws.review_pr > 0 && (
              <ReviewBar ws={ws} request={reviewRequests.find((r) => r.repo_id === ws.repo_id && r.number === ws.review_pr)} refreshKey={wsRefreshKey} onChanges={() => setTab("changes")} />
            )}
            {ws.state === "active" && isTask && (
              <GroupPrBar
                members={members}
                repos={repos}
                prs={prs}
                title={displayName(sky, ws)}
                refreshKey={wsRefreshKey}
                onSay={say}
                onArchive={() => archive()}
                onShowRepo={(id) => { setRepoView((v) => ({ ...v, [ws.id]: id })); setTab("changes"); }}
                onSendAgent={(text) => { setTab("chat"); api.sendToWorkspace(ws.id, text).then(refresh).catch((e) => say(errText(e), true)); }}
              />
            )}

            {showMoveNotice && <MoveNotice compact count={outside.length} onMove={() => setConductorSheet("move")} onDismiss={dismissMoveNotice} />}
            {away?.id === ws.id && (
              <div className="away-banner" role="status">
                <span className="unread" /><b>While you were away:</b> {away.text}
                <span className="spacer" />
                <button className="icon" aria-label="Dismiss" onClick={() => setAway(null)}><X size={13} /></button>
              </div>
            )}

            <div className="panel">
              {ws.state === "archived" || isHistory(ws) ? (
                <div className="pane">
                  <Chat key={`ro-${ws.id}`} workspaceId={ws.id} running={runningSessions} onRunningChange={refresh} jump={jump} readOnly history={isHistory(ws) ? ws.path : undefined} />
                </div>
              ) : (
                <>
                  <div className="pane" hidden={tab !== "chat"}>
                    <Chat tabsSlot={chatSlot} workspaceId={ws.id} running={runningSessions} onRunningChange={refresh} inject={inject} append={append} jump={jump} reviewPr={ws.review_pr} taskRepos={isTask ? members.map((m) => repoNameOf(m.repo_id)) : undefined} handoff={handoffReq?.workspaceId === ws.id ? handoffReq : undefined} />
                  </div>
                  <div className="pane" hidden={tab !== "changes"}>
                    {tab === "changes" && viewWs && perRepo(
                      <Diff
                        key={viewWs.id}
                        workspaceId={viewWs.id} refreshKey={wsRefreshKey} focusFile={focusFile}
                        onSendReview={(text, send) => { setTab("chat"); setInject({ text: inRepo(viewWs, text), nonce: Date.now(), send }); }}
                        reviewPr={ws.review_pr}
                        onSubmitted={(r) => {
                          refresh();
                          const where = r.folded ? ` ${r.folded} went in the summary because their lines aren't in the PR's changes.` : "";
                          say(`Review posted to GitHub.${where}${r.archived ? " The review workspace is archived." : " The workspace has changes, so it stays open."}`, false, r.url ? { label: "Open on GitHub", run: () => openUrl(r.url!).catch(() => {}) } : undefined);
                        }}
                      />
                    )}
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
                          show={previewShow?.ws === ws.id ? previewShow : undefined}
                        />
                      </Suspense>
                    )}
                  </div>
                  <div className="pane" hidden={tab !== "checks"}>
                    {tab === "checks" && viewWs && perRepo(
                      <Suspense fallback={null}>
                        <Checks
                          key={viewWs.id}
                          workspaceId={viewWs.id}
                          repo={repos.find((r) => r.id === viewWs.repo_id)}
                          active={tab === "checks"}
                          onSendAgent={(text) => { setTab("chat"); api.sendToWorkspace(ws.id, inRepo(viewWs, text)).then(refresh).catch((e) => say(errText(e), true)); }}
                          onRepoSettings={() => setSettingsFor(viewWs.repo_id)}
                          onRepoChanged={refresh}
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
          onOpenSettings={() => { setNewFor(null); setNewSource("task"); setSettingsPane("integrations"); setSettingsOpen(true); }}
        />
      )}
      {trustAsk && <TrustSheet repo={trustAsk.repo} trust={trustAsk.trust} agent={trustAsk.agent} onAnswer={trustAsk.answer} />}
      {settingsRepo && <RepoSettings repo={settingsRepo} initialSection={settingsSection} onClose={() => { setSettingsFor(null); setSettingsSection(undefined); refresh(); }} />}
      {palette && <Palette commands={commands()} onClose={() => setPalette(false)} />}
      {searchOpen && (
        <SearchSheet
          repos={repos}
          nameOf={(g) => g.title || starName(sky, { name: g.name } as Workspace)}
          onOpen={(hit) => openHit(hit)}
          onClose={() => setSearchOpen(false)}
        />
      )}
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
      {settingsOpen && <Settings home="~/nekotrail/workspaces" onClose={() => { setSettingsOpen(false); setSettingsPane(undefined); }} update={updater.phase} onCheckUpdates={() => updater.checkNow(false)}
        workspaceId={ws && ws.state === "active" ? ws.id : undefined} repoName={repos.find((r) => r.id === ws?.repo_id)?.name} initialPane={settingsPane} />}
      <ConfirmHost />
      {tourOpen && <Tour onClose={() => setTourOpen(false)} />}
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
      {envFor && (() => {
        const w = workspaces.find((x) => x.id === envFor);
        return <EnvSheet workspaceId={envFor} name={w ? displayName(sky, w) : "this workspace"} onSay={(m) => say(m)} onEditRepo={(id) => { setEnvFor(null); setSettingsSection("files"); setSettingsFor(id); }} onClose={() => setEnvFor(null)} />;
      })()}
      {rulesFor && (() => {
        const w = workspaces.find((x) => x.id === rulesFor.id);
        return <RulesSheet workspaceId={rulesFor.id} initialPane={rulesFor.pane} name={w ? displayName(sky, w) : "this workspace"} readOnly={!w || w.state !== "active" || w.review_pr > 0 || isHistory(w)} onClose={() => setRulesFor(null)} />;
      })()}
      {exportFor && (() => {
        const w = workspaces.find((x) => x.id === exportFor.id);
        if (!w) return null;
        return <ExportSheet ws={w} repoName={repos.find((r) => r.id === w.repo_id)?.name ?? ""} star={starName(sky, w)} initialChat={exportFor.chat}
          pr={prs[w.id]?.pr ?? undefined} onSay={say} onClose={() => setExportFor(null)} />;
      })()}
      {conductorSheet && (
        <ConductorSheet mode={conductorSheet} autoStart={!!rescueParam && ["moving", "done", "partial"].includes(rescueParam)}
          onChanged={conductorChanged} onClose={(note) => { setConductorSheet(null); if (note) say(note.text, note.bad); }} />
      )}
      {terminalImport && <TerminalImport onClose={() => setTerminalImport(false)} onImported={terminalImported} />}
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
          onDone={async (r) => {
            setCloning(false);
            await refresh();
            say(`Cloned ${r.name}.`);
            // During the welcome flow, "Try it" is the next step; otherwise the new-workspace panel opens on it.
            if (!onboardStep) { setNewSource("task"); setNewFor(r.id); }
          }}
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
