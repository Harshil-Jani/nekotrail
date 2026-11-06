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
  /** Paths agents should leave alone, gitignore-style (`.env*`, `migrations/**`). */
  protected: string[];
};

export type Doctor = {
  git: { ok: boolean; version: string };
  gh: { installed: boolean; ok: boolean; login: string | null };
} & Record<Agent, { ok: boolean; version: string }>;
export type ImportReport = { repos: number; workspaces: number; sessions: number; skipped: number; chats_with_history: number };

export type CatalogModel = { id: string; label: string; efforts: string[]; default_effort?: string | null; new?: boolean; released?: string };
export type AgentCatalog = { label: string; effort_label: string; effort_names: Record<string, string>; default_model: string; models: CatalogModel[] };
export type ModelCatalog = Record<Agent, AgentCatalog>;

export type AppSettings = {
  caffeinate: "on" | "off"; auto_branch: "on" | "off"; home: string;
} & Record<`${Agent}_${"model" | "effort" | "mode"}`, string>;

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
  /** What the task is called: from the first prompt, then the PR, or typed by hand. The star name is shown when empty. */
  title: string;
  /** Where `title` came from; "" while there is none. A PR title replaces "auto", nothing replaces "user". */
  title_source: "" | "auto" | "pr" | "user";
  /** Set by hand; empty means derived from the PR. */
  status: WsStatus;
  /** Shared by the workspaces of one bake-off (same task, different agents). */
  pair_id: string;
  /** Sidebar section, or "" for none. */
  section_id: string;
  /** Stacked PRs: the workspace whose branch this one started from and targets, or "". */
  parent_id: string;
};

/** The workspace a stacked one sits on, and that workspace's PR (null before it has one). */
export type StackParent = {
  id: string; branch: string; target: string; state: "active" | "archived";
  pr: { number: number; url: string; state: "OPEN" | "MERGED" | "CLOSED"; title: string; isDraft: boolean; baseRefName: string } | null;
} | null;

export type Section = { id: string; name: string; sort: number };

/** Every agent CLI nekotrail drives (src/agents.ts lists them in order). */
export type Agent = McpAgent | "cursor" | "copilot" | "goose" | "qwen" | "kimi" | "auggie" | "grok" | "kiro" | "droid" | "vibe" | "cline" | "kilo";
/** The agents MCP server and skill sharing knows (src-tauri/src/mcpshare.rs). */
export type McpAgent = "claude" | "codex" | "gemini" | "opencode";
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
  /** Files were rewound. `undo` is the checkpoint saved just before, so the rewind can be undone. */
  | { kind: "rewind"; text: string; undo: string; files: number }
  | { kind: "approval"; request_id: string; tool: string; input: string; description?: string; can_always: boolean }
  | { kind: "approval_answer"; request_id: string; allow: boolean; always: boolean }
  | { kind: "text"; text: string; fix?: "install" | "login" | "limit" | null; agent?: Agent }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; id: string; name: string; input: string }
  | { kind: "tool_result"; id: string; output: string; is_error: boolean }
  | { kind: "done"; is_error: boolean; cost_usd?: number; duration_ms?: number; stopped?: boolean; context_tokens?: number; context_window?: number }
  | { kind: "error"; text: string; fix?: "install" | "login" | "limit" | null; agent?: Agent }
  /** The agent tried to edit a protected file and nekotrail refused (OpenCode and other ACP agents). */
  | { kind: "blocked"; text: string; files: string[] }
  /** After a turn: protected files it changed. `checkpoint` and `ignored` are what Put it back restores from. */
  | { kind: "guard"; files: GuardHit[]; checkpoint: string | null; ignored: string | null; level: GuardLevel }
  /** Put it back ran: these files are as they were before the turn. */
  | { kind: "guard_restored"; files: string[]; checkpoint: string };

/** One protected file a turn changed. */
export type GuardHit = { path: string; change: "added" | "changed" | "deleted" };
/** "blocked": the agent is stopped before it writes a protected file. "after": checked after each turn only. */
export type GuardLevel = "blocked" | "asks" | "after";

export type FileChange = { path: string; status: string; additions: number; deletions: number };
/** Workspaces in one repo changing the same files; `order` is the suggested merge order. */
export type Collision = { repo_id: string; order: string[]; pairs: { a: string; b: string; files: string[] }[] };
export type Scripts = { setup: string | null; run: string | null; archive: string | null };
/** Commands from the repo's own nekotrail.json that the user has not approved yet. */
export type ScriptTrust = Scripts & { fingerprint: string };
export type WorkspaceScripts = Scripts & { trust?: ScriptTrust | null };

/** Who listens on a port (from lsof, your own processes only). */
export type PortHolder = {
  pid: number;
  command: string;
  /** The workspace that started it, if any. */
  workspace_id: string | null;
  /** "tree": its terminal, script or agent started it. "folder": it runs in its folder, parent gone. */
  how: "tree" | "folder" | null;
  /** In this workspace's process tree, so "Free this port" may stop it. */
  can_free: boolean;
};
export type PortRow = {
  port: number;
  /** Variables holding this port: NEKOTRAIL_PORT, PORT, a name from nekotrail.json. */
  vars: string[];
  /** Its name from nekotrail.json ("web"). */
  name: string | null;
  /** Outside the workspace's block: something it started ignored PORT. */
  outside: boolean;
  holder: PortHolder | null;
};
export type PortProblem = { port: number | null; text: string; fix: string };
export type WsPorts = { id: string; base: number; main: number; rows: PortRow[]; problems: PortProblem[] };

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
  /** Conversation comments on the PR (not review comments on lines). */
  commentCount: number;
  baseRefName?: string;
  headRefOid?: string;
} | null;

/** What the PR poller knows about one workspace (the `pr` event payload). `pr` is missing until the
 * first look; `problem` is set when GitHub couldn't be asked. */
export type PrView = {
  workspace_id: string;
  pr?: Pr;
  problem?: { code: "no_gh" | "not_logged_in" | "not_github" | "rate_limited" | "error"; message: string };
};

/** What the PR bar shows for a poller view. `pr`: undefined while unknown ("Looking for a PR"), null
 * for no PR. A problem waiting can't fix (no gh, not logged in, no GitHub remote) settles it to null;
 * a rate limit keeps the last answer and says nothing. `error` is only for what you can act on. */
export function prFromView(view?: PrView): { pr: Pr | undefined; error: string | null } {
  const problem = view?.problem;
  const pr = view?.pr !== undefined ? view.pr : problem && problem.code !== "rate_limited" ? null : undefined;
  const actionable = problem && (problem.code === "no_gh" || problem.code === "not_logged_in" || (problem.code === "error" && view?.pr === undefined));
  return { pr, error: actionable ? problem.message : null };
}

/** PR state for the sidebar, board and radar: OPEN, DRAFT, MERGED or CLOSED. */
export const prStateOf = (pr?: Pr) => (pr ? (pr.isDraft && pr.state === "OPEN" ? "DRAFT" : pr.state) : undefined);

export type GithubUser = { login: string; name: string | null; avatar_url: string; html_url: string; public_repos: number; followers: number } | null;
export type RepoGithub = { nameWithOwner: string; description: string; stargazerCount: number; url: string; owner_avatar: string; isPrivate: boolean } | null;
export type Star = { slug: string; name: string; constellation: string; x: number; y: number };
export type Constellation = { slug: string; name: string; story: string; lines: [string, string][] };
export type Sky = { stars: Star[]; constellations: Constellation[] };

/** GitHub avatar for any login, at the size we draw it (2x for retina). */
export const avatar = (login: string, px = 24) => `https://github.com/${login}.png?size=${px * 2}`;

/** A workspace's star name ("Mochi", "Mochi 2" for a second one); imported workspaces keep their own name. */
export const starName = (sky: Sky, w: Workspace) => {
  const star = starFor(sky, w.name);
  return star ? `${star.name}${w.name.match(/-(\d+)$/)?.[0].replace("-", " ") ?? ""}` : w.name;
};

/** What the sidebar and header call a workspace: its task title, else its star. */
export const displayName = (sky: Sky, w: Workspace) => w.title || starName(sky, w);

/** The secondary line under a titled workspace: its star. Empty when the star is already the title. */
export const subName = (sky: Sky, w: Workspace) => (w.title ? starName(sky, w) : "");

/** The name inside a sentence. A task title is quoted so "Fix login is done" reads as a name: “Fix login” is done. */
export const nameInText = (sky: Sky, w: Workspace) => (w.title ? `“${w.title}”` : starName(sky, w));

/** Status set by hand, else derived: merged PR = done, open PR = in review, else in progress. */
export const statusOf = (w: Workspace, prState?: string | null): Exclude<WsStatus, ""> =>
  w.status || (prState === "MERGED" || prState === "CLOSED" ? "done" : prState === "OPEN" ? "in_review" : "in_progress");

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

/** Things a repo's own agent config (.claude/settings.json, .mcp.json, .codex/; .gemini/ and .env for Gemini; opencode.json and .opencode/ for OpenCode; each other ACP agent's own files) would run, not trusted yet. */
export type AgentConfigItem = { agent: Agent; file: string; what: string; command: string };
export type AgentConfigTrust = { repo: string; fingerprint: string; items: AgentConfigItem[] };

/** MCP servers and skills across Claude, Codex, Gemini and OpenCode (src-tauri/src/mcpshare.rs). */
export type McpHas = { agent: McpAgent; scope: "user" | "local" | "project"; file: string };
export type McpSecret = { id: string; kind: "env" | "header" | "arg" | "url"; key: string; can_ref: boolean };
export type McpRow = {
  name: string; transport: "stdio" | "http" | "sse" | "ws" | null; summary: string; env: string[];
  has: McpHas[]; from: McpHas; secrets: McpSecret[]; targets: { agent: McpAgent; ok: boolean; why: string | null }[]; problem: string | null;
};
export type SkillShare = { target: "agents" | "claude"; label: string; link: string; to: string; ok: boolean; why: string | null };
export type SkillRow = { name: string; description: string; has: McpAgent[]; share: SkillShare[] };
export type McpOverview = { servers: McpRow[]; skills: SkillRow[]; files: Record<McpAgent, string>; opencode_skill_dirs: string[]; problems: string[] };
/** How each secret comes across: as a variable, or a copy of the value. */
export type SecretChoices = Record<string, "ref" | "copy">;
export type McpPreview = { file: string; creates: boolean; backup: string | null; snippet: string; notes: string[]; blockers: string[]; asks: string[]; secrets: McpSecret[]; token: string };

/** Runs before every message is sent; App uses it to ask about a repo's agent config first. */
let beforeTurn: (id: string) => Promise<void> = async () => {};
export const setBeforeTurn = (f: (id: string) => Promise<void>) => { beforeTurn = f; };

export const api = {
  githubProfile: () => invoke<GithubUser>("github_profile"),
  repoGithub: (id: string) => invoke<RepoGithub>("repo_github", { id }),
  sky: () => invoke<Sky>("sky"),
  snapshot: (includeArchived = false) =>
    invoke<{ repos: Repo[]; workspaces: Workspace[]; running: string[]; running_workspaces: string[]; lit: string[]; shining: string[]; costs: Record<string, number> }>("snapshot", { includeArchived }),
  addRepo: (path: string) => invoke<Repo>("add_repo", { path }),
  saveRepoScripts: (id: string, s: Scripts) => invoke<void>("save_repo_scripts", { id, ...s }),
  repoBranches: (id: string) => invoke<string[]>("repo_branches", { id }),
  workspaceScripts: (id: string) => invoke<WorkspaceScripts>("workspace_scripts", { id }),
  trustScripts: (id: string, fingerprint: string) => invoke<void>("trust_scripts", { id, fingerprint }),
  /** `id` is a workspace or a chat. Null when there is nothing to trust. */
  agentConfigTrust: (id: string) => invoke<AgentConfigTrust | null>("agent_config_trust", { id }),
  trustAgentConfig: (id: string, fingerprint: string) => invoke<void>("trust_agent_config", { id, fingerprint }),
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
  stackParent: (id: string) => invoke<StackParent>("stack_parent", { id }),
  /** The parent PR merged: rebase onto where it merged, push with lease, `gh pr edit --base`. */
  restackWorkspace: (id: string) =>
    invoke<{ result: "rebased" | "up-to-date" | "conflicts"; base: string; fork?: string; pushed?: boolean; pr?: number | null }>("restack_workspace", { id }),
  /** From the file lists line counts already gathered: no git of its own. */
  collisions: (prs: Record<string, string>) => invoke<Collision[]>("collisions", { prs }),
  importedOutside: () => invoke<string[]>("imported_outside"),
  relocateImported: () => invoke<[number, string[]]>("relocate_imported"),
  sections: () => invoke<Section[]>("sections"),
  createSection: (name: string) => invoke<Section>("create_section", { name }),
  renameSection: (id: string, name: string) => invoke<void>("rename_section", { id, name }),
  deleteSection: (id: string) => invoke<void>("delete_section", { id }),
  setSection: (id: string, section: string) => invoke<void>("set_section", { id, section }),
  modelCatalog: () => invoke<ModelCatalog>("model_catalog"),
  mcpList: (id: string, agent: Agent) => invoke<string>("mcp_list", { id, agent }),
  mcpShareOverview: (id: string) => invoke<McpOverview>("mcp_share_overview", { id }),
  mcpSharePreview: (id: string, name: string, target: McpAgent, choices: SecretChoices) =>
    invoke<McpPreview>("mcp_share_preview", { id, name, target, choices }),
  mcpShareApply: (id: string, name: string, target: McpAgent, choices: SecretChoices, token: string) =>
    invoke<{ file: string; backup: string | null }>("mcp_share_apply", { id, name, target, choices, token }),
  skillShare: (name: string, target: "agents" | "claude") => invoke<string>("skill_share", { name, target }),
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
  sendToWorkspace: async (id: string, text: string) => { await beforeTurn(id); return invoke<string>("send_to_workspace", { id, text }); },
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
  /** Discard one file's uncommitted changes; its current copy goes to the Trash. */
  discardFile: (id: string, path: string) => invoke<"trashed" | "restored">("discard_file", { id, path }),
  prReviewComments: (id: string) => invoke<import("./components/Diff").GhComment[]>("pr_review_comments", { id }),
  /** Draft review comments on the diff, per workspace. */
  reviewComments: (id: string) => invoke<import("./components/Diff").ReviewComment[]>("review_comments", { id }),
  addReviewComments: (id: string, comments: import("./components/Diff").ReviewComment[]) => invoke<void>("add_review_comments", { id, comments }),
  removeReviewComments: (id: string, ids: string[]) => invoke<void>("remove_review_comments", { id, ids }),
  openIn: (id: string, app: string) => invoke<void>("open_in", { id, app }),
  openInTargets: () => invoke<{ id: string; name: string }[]>("open_in_targets"),
  lineCounts: () => invoke<Record<string, [number, number]>>("line_counts"),
  /** Every workspace's PR as the background poller last saw it. Never calls GitHub; updates come as `pr` events. */
  prCache: () => invoke<Record<string, PrView>>("pr_cache"),
  /** Ask the poller to look at this workspace's repo again soon (`force` right after a change you made). */
  prRefresh: (id?: string, force = false) => invoke<void>("pr_refresh", { id, force }),
  /** The window was hidden or came back: polling pauses while hidden. */
  prVisible: (visible: boolean) => invoke<void>("pr_visible", { visible }),
  createPr: (id: string, title: string, body: string, draft: boolean) =>
    invoke<string>("create_pr", { id, title, body, draft }),
  sessions: (workspaceId: string) => invoke<Session[]>("sessions", { workspaceId }),
  newSession: (workspaceId: string, agent: Agent) => invoke<Session>("new_session", { workspaceId, agent }),
  updateSession: (id: string, title: string, model: string | null, permissionMode: PermissionMode) =>
    invoke<void>("update_session", { id, title, model, permissionMode }),
  sessionEventsTail: (id: string, limit: number) => invoke<{ events: ChatEvent[]; total: number }>("session_events_tail", { id, limit }),
  sessionEvents: (id: string) => invoke<ChatEvent[]>("session_events", { id }),
  /** ⌘⇧F: search every chat, archived workspaces included. `indexing` is set while older messages are still being indexed. */
  searchChats: (query: string, filter: SearchFilter) => invoke<{ groups: SearchGroup[]; indexing: SearchProgress | null }>("search_chats", { query, filter }),
  /** Where a search hit lives: its chat and how many events come before it. */
  searchOpen: (messageId: number) => invoke<SearchPlace>("search_open", { messageId }),
  queued: (id: string) => invoke<string[]>("queued", { id }),
  clearQueue: (id: string) => invoke<void>("clear_queue", { id }),
  send: async (id: string, text: string) => { await beforeTurn(id); return invoke<void>("send", { id, text }); },
  stop: (id: string) => invoke<void>("stop", { id }),
  restoreCheckpoint: (id: string, checkpoint: string) => invoke<void>("restore_checkpoint", { id, checkpoint }),
  /** What rewinding chat `id` to `checkpoint` would change. Errors if an agent is working or it is gone. */
  checkpointPreview: (id: string, checkpoint: string) => invoke<{ files: number; commits: number }>("checkpoint_preview", { id, checkpoint }),
  /** Which of these checkpoints can still be rewound to, for a workspace. */
  liveCheckpoints: (id: string, checkpoints: string[]) => invoke<string[]>("live_checkpoints", { id, checkpoints }),
  /** Protected paths for a repo: the app's list and what its nekotrail.json adds. */
  repoGuard: (id: string) => invoke<{ app: string[]; file: string[] }>("repo_guard", { id }),
  /** Put back the protected files a turn changed (`id` is the chat). Returns the paths put back. */
  putBackProtected: (id: string, checkpoint: string, ignored: string | null, paths: string[]) =>
    invoke<string[]>("put_back_protected", { id, checkpoint, ignored, paths }),
  workspaceFiles: (id: string) => invoke<string[]>("workspace_files", { id }),
  saveAttachment: async (id: string, file: File) =>
    invoke<string>("save_attachment", { id, name: file.name || "pasted.png", bytes: Array.from(new Uint8Array(await file.arrayBuffer())) }),
  /** Preview tab: is a local dev server up, and will it show inside a frame? Local addresses only. */
  previewProbe: (url: string) => invoke<{ up: boolean; status: number | null; blocked: string | null }>("preview_probe", { url }),
  /** The workspace's port block and the port its Preview opens. Null when it holds none (archived). */
  workspacePorts: (id: string) => invoke<{ base: number; main: number } | null>("workspace_ports", { id }),
  /** Every active workspace's ports and who listens on them, or just one workspace's. */
  portsStatus: (id?: string) => invoke<WsPorts[]>("ports_status", { id: id ?? null }),
  /** Stop what listens on `port`, only if this workspace started it. Resolves to how many processes stopped. */
  freePort: (id: string, port: number) => invoke<number>("free_port", { id, port }),
  /** PNG of the whole app web view right now, at screen resolution (Annotate crops it). */
  previewSnapshot: () => invoke<ArrayBuffer>("preview_snapshot"),
  /** Start a shell, or reattach to a running one; returns what it printed so far. */
  ptyOpen: (id: string, cwd: string, cols: number, rows: number) => invoke<string>("pty_open", { id, cwd, cols, rows }),
  /** Run the workspace's setup or run script; `id` is `<workspace id>:<script>`. The backend looks the command up. */
  ptyRun: (id: string, script: "setup" | "run", cols = 120, rows = 30) => invoke<void>("pty_run", { id, script, cols, rows }),
  ptyAttach: (id: string) => invoke<string | null>("pty_attach", { id }),
  ptyWrite: (id: string, data: string) => invoke<void>("pty_write", { id, data }),
  ptyResize: (id: string, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  ptyClose: (id: string) => invoke<void>("pty_close", { id }),
  doctor: () => invoke<Doctor>("doctor"),
  /** Agents whose CLI is on PATH (nothing is run): pickers list these first. */
  installedAgents: () => invoke<Agent[]>("installed_agents"),
  githubRepos: () => invoke<{ nameWithOwner: string; description: string; pushedAt: string; isPrivate: boolean }[]>("github_repos"),
  cloneRepo: (spec: string) => invoke<Repo>("clone_repo", { spec }),
  importPreview: () => invoke<ImportReport>("import_preview"),
  repoIssues: (id: string) => invoke<{ number: number; title: string; body: string; url: string; labels: { name: string }[] }[]>("repo_issues", { id }),
  conductorAvailable: () => invoke<boolean>("conductor_available"),
  importConductor: () => invoke<ImportReport>("import_conductor"),
};

export type SearchKind = "user" | "agent" | "command" | "file";
export type SearchFilter = { repo?: string | null; agent?: string | null; workspace?: string | null; since?: string | null; kind?: "user" | "command" | null; limit?: number };
export type SearchHit = {
  message_id: number; session_id: string; kind: SearchKind;
  /** Text around the match; matches are wrapped in \u0002 and \u0003. */
  snippet: string; created_at: string; agent: Agent; chat: string;
};
export type SearchGroup = {
  workspace_id: string; repo_id: string; repo: string; name: string; title: string; branch: string;
  state: "active" | "archived"; hits: SearchHit[];
  /** Hits in this workspace beyond the ones listed. */
  more: number;
};
export type SearchProgress = { done: number; total: number; finished: boolean };
export type SearchPlace = { session: Session; workspace_id: string; index: number; total: number };
/** Open a chat at one message: Chat loads enough history to show it, scrolls there and flashes it. */
export type JumpTo = { sessionId: string; session: Session; index: number; total: number; nonce: number };

export const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
