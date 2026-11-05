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
};

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
};

export type Agent = "claude" | "codex";
export type PermissionMode = "plan" | "default" | "acceptEdits" | "bypassPermissions";

export type Session = {
  id: string;
  workspace_id: string;
  agent: Agent;
  agent_session_id: string | null;
  title: string;
  model: string | null;
  permission_mode: PermissionMode;
};

export type ChatEvent =
  | { kind: "user"; text: string }
  | { kind: "text"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; id: string; name: string; input: string }
  | { kind: "tool_result"; id: string; output: string; is_error: boolean }
  | { kind: "done"; is_error: boolean; cost_usd?: number; duration_ms?: number; stopped?: boolean }
  | { kind: "error"; text: string };

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
    invoke<{ repos: Repo[]; workspaces: Workspace[]; running: string[]; running_workspaces: string[]; lit: string[]; shining: string[] }>("snapshot", { includeArchived }),
  addRepo: (path: string) => invoke<Repo>("add_repo", { path }),
  saveRepoScripts: (id: string, s: Scripts) => invoke<void>("save_repo_scripts", { id, ...s }),
  repoBranches: (id: string) => invoke<string[]>("repo_branches", { id }),
  workspaceScripts: (id: string) => invoke<Scripts>("workspace_scripts", { id }),
  createWorkspace: (repoId: string, base?: string, name?: string) =>
    invoke<Workspace>("create_workspace", { repoId, base, name }),
  archiveWorkspace: (id: string, force = false) => invoke<void>("archive_workspace", { id, force }),
  restoreWorkspace: (id: string) => invoke<void>("restore_workspace", { id }),
  setPinned: (id: string, pinned: boolean) => invoke<void>("set_pinned", { id, pinned }),
  setUnread: (id: string, unread: boolean) => invoke<void>("set_unread", { id, unread }),
  setNotes: (id: string, notes: string) => invoke<void>("set_notes", { id, notes }),
  setTargetBranch: (id: string, branch: string) => invoke<void>("set_target_branch", { id, branch }),
  changes: (id: string) => invoke<FileChange[]>("changes", { id }),
  fileDiff: (id: string, file: string) => invoke<string>("file_diff", { id, file }),
  openIn: (id: string, app: "finder" | "vscode" | "cursor" | "terminal") => invoke<void>("open_in", { id, app }),
  prStatus: (id: string) => invoke<Pr>("pr_status", { id }),
  createPr: (id: string, title: string, body: string, draft: boolean) =>
    invoke<string>("create_pr", { id, title, body, draft }),
  sessions: (workspaceId: string) => invoke<Session[]>("sessions", { workspaceId }),
  newSession: (workspaceId: string, agent: Agent) => invoke<Session>("new_session", { workspaceId, agent }),
  updateSession: (id: string, title: string, model: string | null, permissionMode: PermissionMode) =>
    invoke<void>("update_session", { id, title, model, permissionMode }),
  sessionEvents: (id: string) => invoke<ChatEvent[]>("session_events", { id }),
  queued: (id: string) => invoke<string[]>("queued", { id }),
  clearQueue: (id: string) => invoke<void>("clear_queue", { id }),
  send: (id: string, text: string) => invoke<void>("send", { id, text }),
  stop: (id: string) => invoke<void>("stop", { id }),
  ptyOpen: (id: string, cwd: string, cols: number, rows: number) => invoke<void>("pty_open", { id, cwd, cols, rows }),
  ptyWrite: (id: string, data: string) => invoke<void>("pty_write", { id, data }),
  ptyResize: (id: string, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  ptyClose: (id: string) => invoke<void>("pty_close", { id }),
  conductorAvailable: () => invoke<boolean>("conductor_available"),
  importConductor: () =>
    invoke<{ repos: number; workspaces: number; sessions: number; skipped: number }>("import_conductor"),
};

export const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
