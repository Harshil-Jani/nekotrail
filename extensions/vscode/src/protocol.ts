// The nekotrail socket protocol, as the app speaks it (src-tauri/src/remote.rs).
//
// One JSON request line, one JSON answer line, over a Unix socket in a folder only you can open.
// The app also checks that the process on the other end runs as you. There is no TCP port.
// This file has no `vscode` import so it can be tested on its own.

import * as net from "node:net";
import * as path from "node:path";

/** The newest protocol this extension speaks. */
export const PROTOCOL = 2;
/** Mirrors `remote::MAX_EDITOR_PROMPT`: the longest task "New task from selection" may send. */
export const MAX_EDITOR_PROMPT = 20_000;
/** Mirrors `remote::MAX_DRAFT`: the longest text for a workspace's message box. */
export const MAX_DRAFT = 100_000;
/** Answers are small; this only stops a broken peer from filling memory. */
const MAX_ANSWER = 16 * 1024 * 1024;

export type Task = { prompt: string; agent?: string | null; base?: string | null; plan?: boolean };

export type Request =
  | { cmd: "show" }
  | { cmd: "add"; path: string }
  | { cmd: "ls"; cwd: string | null; all: boolean }
  | { cmd: "open"; cwd: string | null; query: string }
  | { cmd: "status" }
  | { cmd: "hello" }
  | { cmd: "propose"; cwd: string; task: Task }
  | { cmd: "draft"; workspace: string; text: string }
  | { cmd: "changes"; workspace: string };

/** One workspace as `ls` and `status` describe it. `path` and `target_branch` came in v2. */
export type Row = {
  id: string;
  name: string;
  title: string;
  display: string;
  branch: string;
  path?: string;
  target_branch?: string;
  repo_id: string;
  repo: string | null;
  state: "idle" | "working" | "needs OK" | "done, unread" | string;
  agent: string | null;
  pr: string | null;
  pr_url: string | null;
  needs: string[];
};

export type LsAnswer = { repo: string | null; workspaces: Row[] };
export type StatusAnswer = { working: number; needs: Row[] };
export type HelloAnswer = { protocol: number; min_protocol: number; app: string };
export type ProposeAnswer = { asked: boolean };
export type DraftAnswer = { workspace: string; title: string };
export type FileChange = { path: string; status: string; additions: number; deletions: number };
export type ChangesAnswer = { workspace: string; path: string; target_branch: string; base: string; files: FileChange[] };

/** The version that added each command. A request goes out at that version, like the CLI's. */
export function since(cmd: Request["cmd"]): number {
  return cmd === "hello" || cmd === "propose" || cmd === "draft" || cmd === "changes" ? 2 : 1;
}

/** The line to write: `{"v":<since>,"cmd":...}\n`. */
export function encode(req: Request): string {
  return `${JSON.stringify({ v: since(req.cmd), ...req })}\n`;
}

/** nekotrail isn't running (no socket, or nobody listening on it). */
export class NotRunning extends Error {
  constructor() {
    super("nekotrail isn't running.");
    this.name = "NotRunning";
  }
}

/** The running app is older than this extension. */
export class Outdated extends Error {
  constructor() {
    super("This extension needs a newer nekotrail. Update nekotrail, then reopen it.");
    this.name = "Outdated";
  }
}

/** nekotrail answered, and said no. The message is written for a person. */
export class Refused extends Error {
  constructor(message: string) {
    super(message);
    this.name = "Refused";
  }
}

/** Read one answer line: `{"ok":true,"data":...}` or `{"ok":false,"error":"..."}`. */
export function parseAnswer<T = unknown>(line: string): T {
  let v: { ok?: unknown; data?: unknown; error?: unknown };
  try {
    v = JSON.parse(line.trim());
  } catch {
    throw new Refused("nekotrail sent an answer this extension can't read.");
  }
  if (v && v.ok === true) return v.data as T;
  const err = typeof v?.error === "string" ? v.error : "nekotrail couldn't do that.";
  // An app that predates a command answers like this (remote::decode).
  if (/different versions|needs protocol \d/.test(err)) throw new Outdated();
  throw new Refused(err);
}

/**
 * `<data dir>/dev.nekotrail.app/cli/nekotrail.sock`, the same folder the app uses (`dirs::data_dir`):
 * `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux.
 */
export function socketPath(platform: string, home: string, env: Record<string, string | undefined>): string {
  return path.join(dataDir(platform, home, env), "cli", "nekotrail.sock");
}

export function dataDir(platform: string, home: string, env: Record<string, string | undefined>): string {
  if (platform === "darwin") return path.join(home, "Library", "Application Support", "dev.nekotrail.app");
  const xdg = env.XDG_DATA_HOME;
  const base = xdg && path.isAbsolute(xdg) ? xdg : path.join(home, ".local", "share");
  return path.join(base, "dev.nekotrail.app");
}

export type Connect = (socket: string) => net.Socket;

/** Talks to the running app. Each request is its own short connection, as the CLI does it. */
export class Client {
  constructor(
    private socket: string,
    private connect: Connect = (p) => net.createConnection(p),
  ) {}

  /** Send one request and read its answer. `timeoutMs` covers the whole exchange. */
  request<T>(req: Request, timeoutMs = 10_000): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      let done = false;
      let buf = "";
      const conn = this.connect(this.socket);
      const finish = (err: Error | null, value?: T) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        conn.destroy();
        if (err) reject(err);
        else resolve(value as T);
      };
      const timer = setTimeout(() => finish(new Refused("nekotrail didn't answer in time.")), timeoutMs);
      conn.setEncoding("utf8");
      conn.on("connect", () => conn.write(encode(req)));
      conn.on("data", (chunk: string) => {
        buf += chunk;
        if (buf.length > MAX_ANSWER) return finish(new Refused("nekotrail's answer was too long."));
        const nl = buf.indexOf("\n");
        if (nl < 0) return;
        try {
          finish(null, parseAnswer<T>(buf.slice(0, nl)));
        } catch (e) {
          finish(e as Error);
        }
      });
      conn.on("end", () => finish(buf.trim() ? new Refused("nekotrail closed the connection mid-answer.") : new Refused("nekotrail closed the connection without answering.")));
      conn.on("error", (e: NodeJS.ErrnoException) => {
        if (e.code === "ENOENT" || e.code === "ECONNREFUSED" || e.code === "ENOTDIR") finish(new NotRunning());
        else finish(new Refused(`Couldn't reach nekotrail: ${e.message}`));
      });
    });
  }

  hello() { return this.request<HelloAnswer>({ cmd: "hello" }, 3_000); }
  show() { return this.request<object>({ cmd: "show" }); }
  add(dir: string) { return this.request<{ repo: string; added: boolean }>({ cmd: "add", path: dir }, 60_000); }
  ls(cwd: string) { return this.request<LsAnswer>({ cmd: "ls", cwd, all: false }); }
  status() { return this.request<StatusAnswer>({ cmd: "status" }, 5_000); }
  open(id: string) { return this.request<{ workspace: string }>({ cmd: "open", cwd: null, query: id }); }
  propose(cwd: string, task: Task) { return this.request<ProposeAnswer>({ cmd: "propose", cwd, task }, 30_000); }
  draft(workspace: string, text: string) { return this.request<DraftAnswer>({ cmd: "draft", workspace, text }); }
  // Comparing runs git in the app; big repos can take a moment.
  changes(workspace: string) { return this.request<ChangesAnswer>({ cmd: "changes", workspace }, 60_000); }
}
