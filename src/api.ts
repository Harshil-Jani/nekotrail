import { invoke } from "@tauri-apps/api/core";

export type Repo = {
  id: string;
  name: string;
  root_path: string;
  default_branch: string;
  remote_url: string | null;
  setup_script: string | null;
  run_script: string | null;
  archive_script: string | null;
  prefs: RepoPrefs;
};

export type RepoPrefs = {
  general_prompt: string;
  review_prompt: string;
  pr_prompt: string;
  copy_files: string[];
  env: [string, string][];
  merge_method: string;
  actions: [string, string][];
};

export type Doctor = {
  git: { ok: boolean; version: string };
  gh: { installed: boolean; ok: boolean; login: string | null };
  claude: { ok: boolean; version: string };
  codex: { ok: boolean; version: string };
};
export type ImportReport = { repos: number; workspaces: number; sessions: number; skipped: number; chats_with_history: number };

export type CatalogModel = { id: string; label: string; efforts: string[]; default_effort?: string | null; new?: boolean };
export type AgentCatalog = { label: string; effort_label: string; effort_names: Record<string, string>; default_model: string; models: CatalogModel[] };
export type ModelCatalog = Record<Agent, AgentCatalog>;

export type AppSettings = {
  caffeinate: "on" | "off"; auto_branch: "on" | "off"; home: string;
  claude_model: string; claude_effort: string; claude_mode: string;
  codex_model: string; codex_effort: string; codex_mode: string;
};

export type WsStatus = "" | "backlog" | "in_progress" | "in_review" | "done";
export type Effort = string;

export type Workspace = {
  id: string;
  repo_id: string;
  name: string;
  branch: string;
  path: string;
  target_branch: string;
  state: "active" | "archived";
  pinned: boolean;
  unread: boolean;
  notes: string;
  imported_from: string | null;
  created_at: string;
  /** A name the user gave it; the star name is shown when empty. */
  title: string;
  /** Set by hand; empty means derived from the PR. */
  status: WsStatus;
  /** Shared by the workspaces of one bake-off (same task, different agents). */
  pair_id: string;
  /** Sidebar section, or "" for none. */
  section_id: string;
};

export type Section = { id: string; name: string; sort: number };

export type Agent = "claude" | "codex";
export type PermissionMode = "plan" | "default" | "ask" | "acceptEdits" | "bypassPermissions";

export type Session = {
  id: string;
  workspace_id: string;
  agent: Agent;
  agent_session_id: string | null;
  title: string;
  model: string | null;
  permission_mode: PermissionMode;
  effort: Effort | null;
};

export type ChatEvent =
  | { kind: "user"; text: string; checkpoint?: string | null }
  | { kind: "note"; text: string }
  | { kind: "approval"; request_id: string; tool: string; input: string; description?: string; can_always: boolean }
  | { kind: "approval_answer"; request_id: string; allow: boolean; always: boolean }
  | { kind: "text"; text: string; fix?: "install" | "login" | "limit" | null; agent?: Agent }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; id: string; name: string; input: string }
  | { kind: "tool_result"; id: string; output: string; is_error: boolean }
  | { kind: "done"; is_error: boolean; cost_usd?: number; duration_ms?: number; stopped?: boolean; context_tokens?: number; context_window?: number }
  | { kind: "error"; text: string; fix?: "install" | "login" | "limit" | null; agent?: Agent };

export type FileChange = { path: string; status: string; additions: number; deletions: number };
export type Scripts = { setup: string | null; run: string | null; archive: string | null };

export type Pr = {
  number: number;
  url: string;
  state: "OPEN" | "CLOSED" | "MERGED";
  title: string;
  isDraft: boolean;
  reviewDecision: string;
  mergeable: string;
  statusCheckRollup: { name?: string; context?: string; conclusion?: string; state?: string; status?: string }[];
  author: { login: string };
  latestReviews: { author: { login: string }; state: string }[];
  reviewRequests: { login?: string; name?: string }[];
  additions: number;
  deletions: number;
  comments: unknown[];
} | null;

export type GithubUser = { login: string; name: string | null; avatar_url: string; html_url: string; public_repos: number; followers: number } | null;
export type RepoGithub = { nameWithOwner: string; description: string; stargazerCount: number; url: string; owner_avatar: string; isPrivate: boolean } | null;
export type Star = { slug: string; name: string; constellation: string; x: number; y: number };
export type Constellation = { slug: string; name: string; story: string; lines: [string, string][] };
export type Sky = { stars: Star[]; constellations: Constellation[] };

/** GitHub avatar for any login, at the size we draw it (2x for retina). */
export const avatar = (login: string, px = 24) => `https://github.com/${login}.png?size=${px * 2}`;

/** What the sidebar and header call a workspace: the user's title, else its star, else its name. */
export const displayName = (sky: Sky, w: Workspace) => w.title || starFor(sky, w.name)?.name || w.name;

/** Status set by hand, else derived: merged PR = done, open PR = in review, else in progress. */
export const statusOf = (w: Workspace, prState?: string | null): Exclude<WsStatus, ""> =>
  w.status || (prState === "MERGED" ? "done" : prState === "OPEN" ? "in_review" : "in_progress");

export const STATUS_LABEL: Record<Exclude<WsStatus, "">, string> = {
  backlog: "Backlog",
  in_progress: "In progress",
  in_review: "In review",
  done: "Done",
};

/** The star behind a workspace name like `mochi` or `mochi-2`; imported workspaces have none. */
export const starFor = (sky: Sky, name: string) => {
  const base = name.replace(/-\d+$/, "");
  return sky.stars.find((s) => s.slug === base) ?? null;
};
export const constellationOf = (sky: Sky, star: Star) => sky.constellations.find((c) => c.slug === star.constellation)!;

export const api = {
  githubProfile: () => invoke<GithubUser>("github_profile"),
  repoGithub: (id: string) => invoke<RepoGithub>("repo_github", { id }),
  sky: () => invoke<Sky>("sky"),
  snapshot: (includeArchived = false) =>
    invoke<{ repos: Repo[]; workspaces: Workspace[]; running: string[]; running_workspaces: string[]; lit: string[]; shining: string[]; costs: Record<string, number> }>("snapshot", { includeArchived }),
  addRepo: (path: string) => invoke<Repo>("add_repo", { path }),
  saveRepoScripts: (id: string, s: Scripts) => invoke<void>("save_repo_scripts", { id, ...s }),
  repoBranches: (id: string) => invoke<string[]>("repo_branches", { id }),
  workspaceScripts: (id: string) => invoke<Scripts>("workspace_scripts", { id }),
  createWorkspace: (repoId: string, base?: string, name?: string, from?: string) =>
    invoke<Workspace>("create_workspace", { repoId, base, name, from }),
  repoPrs: (id: string) =>
    invoke<{ number: number; title: string; headRefName: string; author: { login: string }; isDraft: boolean; updatedAt: string }[]>("repo_prs", { id }),
  setWorkspaceTitle: (id: string, title: string) => invoke<void>("set_workspace_title", { id, title }),
  setWorkspaceStatus: (id: string, status: WsStatus) => invoke<void>("set_workspace_status", { id, status }),
  renameBranch: (id: string, branch: string) => invoke<void>("rename_branch", { id, branch }),
  setRepoPrefs: (id: string, prefs: RepoPrefs) => invoke<void>("set_repo_prefs", { id, prefs }),
  setSessionEffort: (id: string, effort: Effort | null) => invoke<void>("set_session_effort", { id, effort }),
  startChat: (id: string, agent: Agent, mode: PermissionMode, title: string, text: string) => invoke<string>("start_chat", { id, agent, mode, title, text }),
  setPair: (ids: string[]) => invoke<string>("set_pair", { ids }),
  workspaceStats: (id: string) => invoke<{ agent: Agent | ""; turns: number; errors: number; duration_ms: number; cost_usd: number }>("workspace_stats", { id }),
  usageSummary: () => invoke<{ today: number; week: number; by_agent: Record<string, number> }>("usage_summary"),
  resourceUsage: () => invoke<{ total_mb: number; top: { name: string; mb: number }[] }>("resource_usage"),
  workspaceStorage: () => invoke<{ workspace_id: string; total_kb: number; cleanable_kb: number; dirs: { path: string; kb: number }[] }[]>("workspace_storage"),
  cleanWorkspace: (id: string) => invoke<number>("clean_workspace", { id }),
  freeDiskMb: () => invoke<number>("free_disk_mb"),
  answerApproval: (requestId: string, allow: boolean, always = false) => invoke<void>("answer_approval", { requestId, allow, always }),
  agentScorecard: () => invoke<{ agent: string; turns: number; errors: number; duration_ms: number; merged: number; cost_usd: number }[]>("agent_scorecard"),
  setAutopilot: (id: string, on: boolean) => invoke<void>("set_autopilot", { id, on }),
  autopilotStatus: () => invoke<Record<string, number>>("autopilot_status"),
  overlaps: (id: string) => invoke<{ workspace_id: string; files: string[] }[]>("overlaps", { id }),
  importedOutside: () => invoke<string[]>("imported_outside"),
  relocateImported: () => invoke<[number, string[]]>("relocate_imported"),
  sections: () => invoke<Section[]>("sections"),
  createSection: (name: string) => invoke<Section>("create_section", { name }),
  renameSection: (id: string, name: string) => invoke<void>("rename_section", { id, name }),
  deleteSection: (id: string) => invoke<void>("delete_section", { id }),
  setSection: (id: string, section: string) => invoke<void>("set_section", { id, section }),
  modelCatalog: () => invoke<ModelCatalog>("model_catalog"),
  mcpList: (id: string, agent: Agent) => invoke<string>("mcp_list", { id, agent }),
  slashCommands: (id: string) => invoke<[string, string][]>("slash_commands", { id }),
  addDir: (id: string, dir: string) => invoke<string[]>("add_dir", { id, dir }),
  forkSession: (id: string) => invoke<Session>("fork_session", { id }),
  removeQueued: (id: string, index: number) => invoke<string[]>("remove_queued", { id, index }),
  hideSession: (id: string) => invoke<void>("hide_session", { id }),
  appSettings: () => invoke<AppSettings>("app_settings"),
  setAppSetting: (key: Exclude<keyof AppSettings, "home">, value: string) => invoke<void>("set_app_setting", { key, value }),
  commitPush: (id: string, message = "") => invoke<string>("commit_push", { id, message }),
  pullTarget: (id: string) => invoke<"up-to-date" | "merged" | "conflicts">("pull_target", { id }),
  mergePr: (id: string, auto = false) => invoke<void>("merge_pr", { id, auto }),
  failingLogs: (id: string) => invoke<string>("failing_logs", { id }),
  sendToWorkspace: (id: string, text: string) => invoke<string>("send_to_workspace", { id, text }),
  archiveWorkspace: (id: string, force = false) => invoke<void>("archive_workspace", { id, force }),
  restoreWorkspace: (id: string) => invoke<void>("restore_workspace", { id }),
  setPinned: (id: string, pinned: boolean) => invoke<void>("set_pinned", { id, pinned }),
  setUnread: (id: string, unread: boolean) => invoke<void>("set_unread", { id, unread }),
  setNotes: (id: string, notes: string) => invoke<void>("set_notes", { id, notes }),
  setTargetBranch: (id: string, branch: string) => invoke<void>("set_target_branch", { id, branch }),
  changes: (id: string, uncommitted = false) => invoke<FileChange[]>("changes", { id, uncommitted }),
  fileDiff: (id: string, file: string, uncommitted = false) => invoke<string>("file_diff", { id, file, uncommitted }),
  readFile: (id: string, path: string) => invoke<string>("read_file", { id, path }),
  writeFile: (id: string, path: string, content: string) => invoke<void>("write_file", { id, path, content }),
  prReviewComments: (id: string) => invoke<import("./components/Diff").GhComment[]>("pr_review_comments", { id }),
  openIn: (id: string, app: string) => invoke<void>("open_in", { id, app }),
  openInTargets: () => invoke<{ id: string; name: string }[]>("open_in_targets"),
  lineCounts: () => invoke<Record<string, [number, number]>>("line_counts"),
  prStates: () => invoke<Record<string, "OPEN" | "MERGED" | "CLOSED">>("pr_states"),
  prStatus: (id: string) => invoke<Pr>("pr_status", { id }),
  createPr: (id: string, title: string, body: string, draft: boolean) =>
    invoke<string>("create_pr", { id, title, body, draft }),
  sessions: (workspaceId: string) => invoke<Session[]>("sessions", { workspaceId }),
  newSession: (workspaceId: string, agent: Agent) => invoke<Session>("new_session", { workspaceId, agent }),
  updateSession: (id: string, title: string, model: string | null, permissionMode: PermissionMode) =>
    invoke<void>("update_session", { id, title, model, permissionMode }),
  sessionEventsTail: (id: string, limit: number) => invoke<{ events: ChatEvent[]; total: number }>("session_events_tail", { id, limit }),
  sessionEvents: (id: string) => invoke<ChatEvent[]>("session_events", { id }),
  queued: (id: string) => invoke<string[]>("queued", { id }),
  clearQueue: (id: string) => invoke<void>("clear_queue", { id }),
  send: (id: string, text: string) => invoke<void>("send", { id, text }),
  stop: (id: string) => invoke<void>("stop", { id }),
  restoreCheckpoint: (id: string, checkpoint: string) => invoke<void>("restore_checkpoint", { id, checkpoint }),
  workspaceFiles: (id: string) => invoke<string[]>("workspace_files", { id }),
  saveAttachment: async (id: string, file: File) =>
    invoke<string>("save_attachment", { id, name: file.name || "pasted.png", bytes: Array.from(new Uint8Array(await file.arrayBuffer())) }),
  /** Preview tab: is a local dev server up, and will it show inside a frame? Local addresses only. */
  previewProbe: (url: string) => invoke<{ up: boolean; status: number | null; blocked: string | null }>("preview_probe", { url }),
  /** PNG of the whole app web view right now, at screen resolution (Annotate crops it). */
  previewSnapshot: () => invoke<ArrayBuffer>("preview_snapshot"),
  /** Start a shell, or reattach to a running one; returns what it printed so far. */
  ptyOpen: (id: string, cwd: string, cols: number, rows: number) => invoke<string>("pty_open", { id, cwd, cols, rows }),
  ptyRun: (id: string, cwd: string, cmd: string, cols = 120, rows = 30) => invoke<void>("pty_run", { id, cwd, cmd, cols, rows }),
  ptyAttach: (id: string) => invoke<string | null>("pty_attach", { id }),
  ptyWrite: (id: string, data: string) => invoke<void>("pty_write", { id, data }),
  ptyResize: (id: string, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  ptyClose: (id: string) => invoke<void>("pty_close", { id }),
  doctor: () => invoke<Doctor>("doctor"),
  githubRepos: () => invoke<{ nameWithOwner: string; description: string; pushedAt: string; isPrivate: boolean }[]>("github_repos"),
  cloneRepo: (spec: string) => invoke<Repo>("clone_repo", { spec }),
  importPreview: () => invoke<ImportReport>("import_preview"),
  repoIssues: (id: string) => invoke<{ number: number; title: string; body: string; url: string; labels: { name: string }[] }[]>("repo_issues", { id }),
  conductorAvailable: () => invoke<boolean>("conductor_available"),
  importConductor: () => invoke<ImportReport>("import_conductor"),
};

export const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
