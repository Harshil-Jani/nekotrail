// The nekotrail extension for VS Code, Cursor and other VS Code forks. It reads and asks the
// nekotrail app over its owner-only Unix socket (protocol.ts) and never starts an agent itself:
// a new task always waits for your OK in the app. No telemetry, no network.

import { execFile } from "node:child_process";
import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";
import { launch, launchers } from "./launch";
import { addedStatus, deletedStatus, draftText, rowForFile, statusBar, taskPrompt, toItems, type Item, type Selection, type ViewState } from "./model";
import { Client, dataDir, NotRunning, Outdated, socketPath, type ChangesAnswer, type Row } from "./protocol";

const BASE_SCHEME = "nekotrail-base";
const RELEASES = "https://github.com/Harshil-Jani/nekotrail/releases";

let client: Client;
let log: vscode.OutputChannel;

export function activate(ctx: vscode.ExtensionContext) {
  // $HOME first, as the app's `dirs` crate reads it.
  const home = process.env.HOME || os.homedir();
  client = new Client(socketPath(process.platform, home, process.env));
  log = vscode.window.createOutputChannel("NekoTrail");

  const view = new Workspaces();
  const tree = vscode.window.createTreeView("nekotrail.workspaces", { treeDataProvider: view, showCollapseAll: false });
  const bar = vscode.window.createStatusBarItem("nekotrail.status", vscode.StatusBarAlignment.Left, 50);
  bar.name = "NekoTrail";
  bar.command = "nekotrail.focusApp";
  bar.show();

  let app: "unknown" | "offline" | "outdated" | "ok" = "unknown";
  let helloOk = false;
  let needs = 0;
  let busy = false;

  const setState = (s: ViewState) => {
    if (view.state === s) return;
    view.state = s;
    vscode.commands.executeCommand("setContext", "nekotrail.state", s);
  };
  const paintBar = () => {
    const b = statusBar(app === "unknown" ? "ok" : app, needs);
    bar.text = b.text;
    bar.tooltip = b.tooltip;
    bar.backgroundColor = app === "ok" && needs > 0 ? new vscode.ThemeColor("statusBarItem.warningBackground") : undefined;
  };

  /** Ask nekotrail what's new. Quiet: errors only change what the view says. */
  const refresh = async () => {
    if (busy) return;
    busy = true;
    try {
      if (!helloOk) {
        await client.hello();
        helloOk = true;
      }
      needs = (await client.status()).needs.length;
      app = "ok";
      const folder = currentFolder();
      if (!folder) {
        view.set([]);
        setState("noFolder");
      } else {
        const ls = await client.ls(folder);
        // ls answers for every repo when the folder isn't one nekotrail has (repo is null).
        if (ls.repo === null) {
          view.set([]);
          setState("notAdded");
        } else {
          view.set(toItems(ls.workspaces, folder));
          setState(ls.workspaces.length ? "ready" : "empty");
        }
      }
    } catch (e) {
      helloOk = false;
      if (e instanceof NotRunning) {
        app = "offline";
        setState("offline");
      } else if (e instanceof Outdated) {
        app = "outdated";
        setState("outdated");
      } else {
        log.appendLine(`refresh: ${(e as Error).message}`);
      }
      view.set([]);
    } finally {
      busy = false;
      paintBar();
    }
  };

  /** True when nekotrail answers. Offers to start it when it doesn't. */
  const ensureRunning = async (): Promise<boolean> => {
    try {
      await client.hello();
      return true;
    } catch (e) {
      if (e instanceof Outdated) {
        vscode.window.showErrorMessage(e.message);
        return false;
      }
      if (!(e instanceof NotRunning)) {
        vscode.window.showErrorMessage((e as Error).message);
        return false;
      }
    }
    const pick = await vscode.window.showInformationMessage("NekoTrail isn't running. Start it?", "Start NekoTrail");
    return pick === "Start NekoTrail" && startApp();
  };

  /** Start the app and wait until it answers (up to 40 seconds, as the CLI waits). */
  const startApp = async (): Promise<boolean> => {
    const started = await launch(launchers(process.platform, home, dataDir(process.platform, home, process.env), process.env.PATH));
    if (!started) {
      const pick = await vscode.window.showErrorMessage(
        process.platform === "darwin"
          ? "Couldn't start NekoTrail. Is it in your Applications folder?"
          : "Couldn't find NekoTrail. Open it once, then install its terminal command (Settings, General) so the editor can start it.",
        "Get NekoTrail",
      );
      if (pick) vscode.env.openExternal(vscode.Uri.parse(RELEASES));
      return false;
    }
    const ok = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: "Starting nekotrail..." }, async () => {
      const until = Date.now() + 40_000;
      while (Date.now() < until) {
        await new Promise((r) => setTimeout(r, 500));
        try {
          await client.hello();
          return true;
        } catch (e) {
          if (e instanceof Outdated) return false;
        }
      }
      return false;
    });
    if (!ok) vscode.window.showErrorMessage("NekoTrail started but didn't answer. Open it once from your applications, then try again.");
    await refresh();
    return ok;
  };

  /** Run `f`, making sure nekotrail is up first, and say what went wrong in plain words. */
  const guarded = (f: (...args: any[]) => Promise<unknown>) => async (...args: any[]) => {
    if (!(await ensureRunning())) return;
    try {
      await f(...args);
    } catch (e) {
      if (e instanceof NotRunning) vscode.window.showErrorMessage("NekoTrail stopped answering. Start it and try again.");
      else vscode.window.showErrorMessage((e as Error).message);
    }
    refresh();
  };

  /** The workspace an item stands for, or one you pick. */
  const pickRow = async (item: Item | undefined, placeHolder: string, dir?: string): Promise<Row | undefined> => {
    if (item?.row) return item.row;
    const folder = dir ?? currentFolder();
    if (!folder) {
      vscode.window.showWarningMessage("Open a folder from a git repo first.");
      return;
    }
    const ls = await client.ls(folder);
    if (ls.repo === null) {
      vscode.window.showWarningMessage("This repo isn't in NekoTrail yet. Run \"NekoTrail: Add this repo to NekoTrail\".");
      return;
    }
    const items = toItems(ls.workspaces, folder);
    if (!items.length) {
      vscode.window.showInformationMessage(`No workspaces in ${ls.repo} yet.`);
      return;
    }
    const picked = await vscode.window.showQuickPick(
      items.map((i) => ({ label: i.label, description: i.description, item: i })),
      { placeHolder, matchOnDescription: true },
    );
    return picked?.item.row;
  };

  const openChanges = async (item?: Item) => {
    const row = await pickRow(item, "Which workspace's changes?");
    if (!row) return;
    const ch = await client.changes(row.id);
    await showChanges(ch);
  };

  const openWindow = async (item?: Item) => {
    const row = await pickRow(item, "Open which workspace in a new window?");
    if (!row) return;
    if (!row.path) throw new Outdated();
    await vscode.commands.executeCommand("vscode.openFolder", vscode.Uri.file(row.path), { forceNewWindow: true });
  };

  const newTask = async () => {
    const ed = vscode.window.activeTextEditor;
    if (!ed) return vscode.window.showWarningMessage("Open a file and select the code the task is about.");
    const sel = await selection(ed, true);
    const dir = cwdFor(ed);
    if (!dir) return vscode.window.showWarningMessage("Save the file in a git repo first, so NekoTrail knows which repo the task is for.");
    const instruction = await vscode.window.showInputBox({
      title: "NekoTrail: New task from selection",
      prompt: `The task for the agent. ${sel.code ? "The selected code" : "The file and line"} goes with it. You confirm in NekoTrail before anything starts.`,
      placeHolder: "For example: make this retry once on a network error",
      validateInput: (v) => (v.trim() ? null : "Say what the agent should do."),
      ignoreFocusOut: true,
    });
    if (!instruction) return;
    const agent = vscode.workspace.getConfiguration("nekotrail").get<string>("agent")?.trim() || null;
    const res = await client.propose(dir, { prompt: taskPrompt(instruction, sel), agent });
    vscode.window.showInformationMessage(res.asked ? "Check the task in NekoTrail and press Start agent there." : "That task is already waiting for your answer in NekoTrail.");
  };

  const send = async (item?: Item) => {
    const ed = vscode.window.activeTextEditor;
    if (!ed || ed.selections.every((s) => s.isEmpty)) return vscode.window.showWarningMessage("Select some code first.");
    const sel = await selection(ed, false);
    let row = item?.row;
    if (!row) {
      const dir = cwdFor(ed);
      // A file inside a workspace's worktree goes to that workspace; otherwise you pick.
      if (dir && ed.document.uri.scheme === "file") {
        const ls = await client.ls(dir);
        row = rowForFile(ls.workspaces, ed.document.uri.fsPath);
      }
      row ??= await pickRow(undefined, "Send the selection to which workspace's chat?", dir ?? undefined);
    }
    if (!row) return;
    const res = await client.draft(row.id, draftText(sel));
    vscode.window.showInformationMessage(`It's in the message box of ${res.title || res.workspace}. Press Send in NekoTrail.`);
  };

  const click = (item: Item) => (vscode.workspace.getConfiguration("nekotrail").get("click") === "window" ? openWindow(item) : openChanges(item));

  ctx.subscriptions.push(
    log,
    tree,
    bar,
    vscode.workspace.registerTextDocumentContentProvider(BASE_SCHEME, new BaseFiles()),
    vscode.commands.registerCommand("nekotrail.refresh", refresh),
    vscode.commands.registerCommand("nekotrail.start", async () => {
      try {
        await client.hello();
        await client.show();
      } catch (e) {
        if (e instanceof NotRunning) await startApp();
        else vscode.window.showErrorMessage((e as Error).message);
      }
    }),
    vscode.commands.registerCommand("nekotrail.focusApp", guarded(() => client.show())),
    vscode.commands.registerCommand("nekotrail.addRepo", guarded(async () => {
      const folder = currentFolder();
      if (!folder) return vscode.window.showWarningMessage("Open a folder from a git repo first.");
      const r = await client.add(folder);
      vscode.window.showInformationMessage(r.added ? `Added ${r.repo} to NekoTrail.` : `${r.repo} is already in NekoTrail.`);
    })),
    vscode.commands.registerCommand("nekotrail.click", guarded(click)),
    vscode.commands.registerCommand("nekotrail.openChanges", guarded(openChanges)),
    vscode.commands.registerCommand("nekotrail.openWindow", guarded(openWindow)),
    vscode.commands.registerCommand("nekotrail.openInApp", guarded(async (item?: Item) => {
      const row = await pickRow(item, "Show which workspace in NekoTrail?");
      if (row) await client.open(row.id);
    })),
    vscode.commands.registerCommand("nekotrail.openPr", async (item?: Item) => {
      const url = item?.row.pr_url;
      if (url && /^https:\/\//.test(url)) vscode.env.openExternal(vscode.Uri.parse(url));
    }),
    vscode.commands.registerCommand("nekotrail.newTaskFromSelection", guarded(newTask)),
    vscode.commands.registerCommand("nekotrail.sendSelection", guarded(() => send())),
    vscode.commands.registerCommand("nekotrail.sendSelectionTo", guarded((item: Item) => send(item))),
    vscode.window.onDidChangeActiveTextEditor(() => refresh()),
    vscode.workspace.onDidChangeWorkspaceFolders(() => refresh()),
    vscode.window.onDidChangeWindowState((s) => s.focused && refresh()),
    tree.onDidChangeVisibility((e) => e.visible && refresh()),
  );

  // Poll while the window is open; each ask is one short local connection.
  let timer: NodeJS.Timeout | undefined;
  const schedule = () => {
    if (timer) clearInterval(timer);
    const secs = Math.max(2, vscode.workspace.getConfiguration("nekotrail").get<number>("refreshSeconds") ?? 5);
    timer = setInterval(() => vscode.window.state.focused && refresh(), secs * 1000);
  };
  schedule();
  ctx.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((e) => e.affectsConfiguration("nekotrail.refreshSeconds") && schedule()),
    { dispose: () => timer && clearInterval(timer) },
  );
  paintBar();
  refresh();
}

export function deactivate() {}

// ---------- the view ----------

class Workspaces implements vscode.TreeDataProvider<Item> {
  state: ViewState | undefined;
  private items: Item[] = [];
  private key = "";
  private changed = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.changed.event;

  /** Replace the rows. Repaints only when something shown changed, so the view doesn't flicker. */
  set(items: Item[]) {
    const key = JSON.stringify(items.map((i) => [i.id, i.label, i.description, i.icon, i.contextValue]));
    this.items = items;
    if (key !== this.key) {
      this.key = key;
      this.changed.fire();
    }
  }

  getChildren(): Item[] {
    return this.items;
  }

  getTreeItem(i: Item): vscode.TreeItem {
    const t = new vscode.TreeItem(i.label, vscode.TreeItemCollapsibleState.None);
    t.id = i.id;
    t.description = i.description;
    t.tooltip = i.tooltip;
    t.iconPath = new vscode.ThemeIcon(i.icon, i.color ? new vscode.ThemeColor(i.color) : undefined);
    t.contextValue = i.contextValue;
    t.command = { command: "nekotrail.click", title: "Open", arguments: [i] };
    t.accessibilityInformation = { label: `${i.label}, ${i.description}` };
    return t;
  }
}

// ---------- helpers ----------

/** The folder the view is about: the active file's workspace folder, else the first one. */
function currentFolder(): string | null {
  const doc = vscode.window.activeTextEditor?.document;
  const f = doc && doc.uri.scheme === "file" ? vscode.workspace.getWorkspaceFolder(doc.uri) : undefined;
  const folder = f ?? vscode.workspace.workspaceFolders?.find((w) => w.uri.scheme === "file");
  return folder ? folder.uri.fsPath : null;
}

/** Where to ask from for this editor: the file's folder, so the right repo is found in a multi-root window. */
function cwdFor(ed: vscode.TextEditor): string | null {
  return ed.document.uri.scheme === "file" ? path.dirname(ed.document.uri.fsPath) : currentFolder();
}

/** git, with fsmonitor off so an untrusted repo's config can't run a program through it. */
function git(cwd: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile("git", ["-c", "core.fsmonitor=false", ...args], { cwd, maxBuffer: 64 * 1024 * 1024 }, (err, out) => (err ? reject(err) : resolve(out)));
  });
}

/** The selection with its path from the repo root (what the agent's worktree sees) and its lines. */
async function selection(ed: vscode.TextEditor, allowEmpty: boolean): Promise<Selection> {
  const s = ed.selection;
  const doc = ed.document;
  let rel = vscode.workspace.asRelativePath(doc.uri, false);
  if (doc.uri.scheme === "file") {
    const top = await git(path.dirname(doc.uri.fsPath), ["rev-parse", "--show-toplevel"]).then((o) => o.trim()).catch(() => "");
    if (top) rel = path.relative(top, doc.uri.fsPath).split(path.sep).join("/");
  }
  const startLine = s.start.line + 1;
  // A selection ending at the start of a line doesn't include that line.
  const endLine = s.end.line > s.start.line && s.end.character === 0 ? s.end.line : s.end.line + 1;
  const code = s.isEmpty && allowEmpty ? "" : doc.getText(s);
  return { relPath: rel, startLine, endLine, languageId: doc.languageId, code };
}

/** Old files for the diff: `git show <base>:<path>` in the worktree, or nothing. */
class BaseFiles implements vscode.TextDocumentContentProvider {
  async provideTextDocumentContent(uri: vscode.Uri): Promise<string> {
    const q = JSON.parse(uri.query || "{}") as { cwd?: string; rev?: string; rel?: string };
    // The base is a commit id from nekotrail; anything else shows as empty.
    if (!q.cwd || !q.rel || !q.rev || !/^[0-9a-f]{7,64}$/.test(q.rev)) return "";
    return git(q.cwd, ["show", `${q.rev}:${q.rel}`]).catch(() => "");
  }
}

function baseUri(abs: string, cwd: string, rev: string, rel: string): vscode.Uri {
  return vscode.Uri.file(abs).with({ scheme: BASE_SCHEME, query: JSON.stringify({ cwd, rev, rel }) });
}

/** The workspace's changes in the editor's multi-file diff (one file at a time on older editors). */
async function showChanges(ch: ChangesAnswer) {
  if (!ch.files.length) {
    vscode.window.showInformationMessage(`${ch.workspace} has no changes against ${ch.target_branch} yet.`);
    return;
  }
  const title = `${ch.workspace} vs ${ch.target_branch}`;
  const rows = ch.files.map((f) => {
    const abs = path.join(ch.path, f.path);
    const left = baseUri(abs, ch.path, addedStatus(f.status) ? "" : ch.base, f.path);
    const right = deletedStatus(f.status) ? baseUri(abs, ch.path, "", f.path) : vscode.Uri.file(abs);
    return { f, abs, left, right };
  });
  const all = await vscode.commands.getCommands(true);
  if (all.includes("vscode.changes")) {
    await vscode.commands.executeCommand("vscode.changes", title, rows.map((r) => [vscode.Uri.file(r.abs), r.left, r.right]));
    return;
  }
  const pick = await vscode.window.showQuickPick(
    rows.map((r) => ({ label: r.f.path, description: `${r.f.status} +${r.f.additions} -${r.f.deletions}`, r })),
    { placeHolder: title },
  );
  if (pick) await vscode.commands.executeCommand("vscode.diff", pick.r.left, pick.r.right, `${pick.r.f.path} (${title})`);
}
