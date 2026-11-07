// Activation against a small stand-in for the `vscode` module: every command the manifest
// contributes is registered, and with no app running the view and status bar say so.
// This is not a real editor; the README lists what only a real editor run covers.

import { expect, mock, test } from "bun:test";
import * as fs from "node:fs";
import * as path from "node:path";

const registered = new Set<string>();
const contexts: Record<string, unknown> = {};
const bar: { text?: string; tooltip?: string; command?: string } = {};
const disposable = { dispose() {} };
const event = () => disposable;

mock.module("vscode", () => ({
  StatusBarAlignment: { Left: 1 },
  TreeItemCollapsibleState: { None: 0 },
  ProgressLocation: { Notification: 15 },
  ThemeIcon: class { constructor(public id: string) {} },
  ThemeColor: class { constructor(public id: string) {} },
  TreeItem: class { constructor(public label: string) {} },
  EventEmitter: class { event = event; fire() {} },
  Uri: { file: (p: string) => ({ fsPath: p, scheme: "file" }), parse: (s: string) => s },
  window: {
    state: { focused: true },
    activeTextEditor: undefined,
    createOutputChannel: () => ({ appendLine() {}, dispose() {} }),
    createTreeView: () => ({ onDidChangeVisibility: event, dispose() {} }),
    createStatusBarItem: () => Object.assign(bar, { show() {}, dispose() {} }),
    onDidChangeActiveTextEditor: event,
    onDidChangeWindowState: event,
  },
  workspace: {
    workspaceFolders: undefined,
    getConfiguration: () => ({ get: () => undefined }),
    registerTextDocumentContentProvider: () => disposable,
    onDidChangeWorkspaceFolders: event,
    onDidChangeConfiguration: event,
  },
  commands: {
    registerCommand: (id: string) => { registered.add(id); return disposable; },
    executeCommand: async (cmd: string, key: string, value: unknown) => { if (cmd === "setContext") contexts[key] = value; },
  },
  env: { openExternal() {} },
}));

test("registers every contributed command and reads as offline with no app", async () => {
  // A home with no nekotrail socket, so this never talks to a nekotrail running on this machine.
  // Short on purpose: a Unix socket path can't be much over 100 bytes.
  const home = fs.mkdtempSync("/tmp/nth-");
  const was = process.env.HOME;
  process.env.HOME = home;
  process.env.XDG_DATA_HOME = path.join(home, "data");
  try {
    const ext = await import("../src/extension");
    const subs: { dispose(): void }[] = [];
    ext.activate({ subscriptions: subs } as any);
    const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
    for (const c of manifest.contributes.commands) expect(registered.has(c.command)).toBe(true);
    expect(registered.has("nekotrail.click")).toBe(true);
    // The first refresh finds no socket.
    for (let i = 0; i < 50 && contexts["nekotrail.state"] !== "offline"; i++) await new Promise((r) => setTimeout(r, 10));
    expect(contexts["nekotrail.state"]).toBe("offline");
    expect(bar.text).toBe("$(circle-slash) NekoTrail");
    expect(bar.command).toBe("nekotrail.focusApp");
    for (const s of subs) s.dispose();
  } finally {
    process.env.HOME = was;
    delete process.env.XDG_DATA_HOME;
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test("every welcome state the view can be in has text in the manifest", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
  const whens = manifest.contributes.viewsWelcome.map((w: { when: string }) => w.when);
  for (const s of ["offline", "outdated", "noFolder", "notAdded", "empty"]) expect(whens).toContain(`nekotrail.state == ${s}`);
});
