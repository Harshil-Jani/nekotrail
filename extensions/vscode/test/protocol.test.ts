import { afterEach, describe, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as net from "node:net";
import * as os from "node:os";
import * as path from "node:path";
import { Client, encode, MAX_DRAFT, MAX_EDITOR_PROMPT, NotRunning, Outdated, parseAnswer, Refused, since, socketPath } from "../src/protocol";
import { launchers } from "../src/launch";

describe("requests", () => {
  test("go out as one line at the version that added the command, like the CLI", () => {
    expect(encode({ cmd: "status" })).toBe('{"v":1,"cmd":"status"}\n');
    expect(encode({ cmd: "ls", cwd: "/r", all: false })).toBe('{"v":1,"cmd":"ls","cwd":"/r","all":false}\n');
    expect(JSON.parse(encode({ cmd: "propose", cwd: "/r", task: { prompt: "fix it" } }))).toEqual({ v: 2, cmd: "propose", cwd: "/r", task: { prompt: "fix it" } });
    for (const cmd of ["hello", "propose", "draft", "changes"] as const) expect(since(cmd)).toBe(2);
    for (const cmd of ["show", "add", "ls", "open", "status"] as const) expect(since(cmd)).toBe(1);
  });

  test("a prompt with newlines and quotes stays on one line", () => {
    const line = encode({ cmd: "draft", workspace: "mochi", text: 'a\n"b"\n```' });
    expect(line.indexOf("\n")).toBe(line.length - 1);
    expect(JSON.parse(line).text).toBe('a\n"b"\n```');
  });

  test("limits match the app's (remote.rs)", () => {
    const rs = fs.readFileSync(path.join(__dirname, "../../../src-tauri/src/remote.rs"), "utf8");
    expect(rs).toContain(`pub const MAX_EDITOR_PROMPT: usize = ${MAX_EDITOR_PROMPT.toLocaleString("en").replace(/,/g, "_")};`);
    expect(rs).toContain(`pub const MAX_DRAFT: usize = ${MAX_DRAFT.toLocaleString("en").replace(/,/g, "_")};`);
    expect(rs).toContain("pub const PROTOCOL: u32 = 2;");
  });
});

describe("answers", () => {
  test("ok gives the data, an error is refused with the app's words", () => {
    expect(parseAnswer('{"ok":true,"data":{"a":1}}')).toEqual({ a: 1 });
    expect(() => parseAnswer('{"ok":false,"error":"No active workspace matches \\"x\\"."}')).toThrow(Refused);
    expect(() => parseAnswer("garbage")).toThrow("can't read");
    expect(() => parseAnswer('{"ok":false}')).toThrow("couldn't do that");
  });

  test("an older app's version answer reads as Outdated", () => {
    expect(() => parseAnswer('{"ok":false,"error":"This nekotrail command and the running app are different versions. Quit and reopen nekotrail, then try again."}')).toThrow(Outdated);
    expect(() => parseAnswer('{"ok":false,"error":"nekotrail didn\'t understand that request: \\"hello\\" needs protocol 2 or newer."}')).toThrow(Outdated);
  });
});

describe("where the socket is", () => {
  test("the app's data folder on each system", () => {
    expect(socketPath("darwin", "/Users/me", {})).toBe("/Users/me/Library/Application Support/dev.nekotrail.app/cli/nekotrail.sock");
    expect(socketPath("linux", "/home/me", {})).toBe("/home/me/.local/share/dev.nekotrail.app/cli/nekotrail.sock");
    expect(socketPath("linux", "/home/me", { XDG_DATA_HOME: "/data" })).toBe("/data/dev.nekotrail.app/cli/nekotrail.sock");
    // A relative XDG_DATA_HOME is ignored, as the spec says (and as the app's `dirs` crate does).
    expect(socketPath("linux", "/home/me", { XDG_DATA_HOME: "rel" })).toBe("/home/me/.local/share/dev.nekotrail.app/cli/nekotrail.sock");
  });
});

describe("starting the app", () => {
  test("macOS opens it by bundle id; Linux runs the terminal command it can find", () => {
    expect(launchers("darwin", "/Users/me", "/x", "/usr/bin")).toEqual([{ cmd: "open", args: ["-b", "dev.nekotrail.app"] }]);
    const have = new Set(["/home/me/.local/bin/nekotrail", "/d/bin/nekotrail"]);
    expect(launchers("linux", "/home/me", "/d", "/usr/bin:/home/me/.local/bin:relative", (p) => have.has(p))).toEqual([
      { cmd: "/home/me/.local/bin/nekotrail", args: [] },
      { cmd: "/d/bin/nekotrail", args: [] },
    ]);
    expect(launchers("linux", "/home/me", "/d", undefined, () => false)).toEqual([]);
  });
});

describe("the client over a real Unix socket", () => {
  let dir = "";
  let server: net.Server | null = null;
  afterEach(() => {
    server?.close();
    server = null;
    if (dir) fs.rmSync(dir, { recursive: true, force: true });
  });

  /** A stand-in app: records each request line and answers with `reply(request)`. */
  const serve = async (reply: (req: any) => string | null) => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "nt-"));
    const sock = path.join(dir, "s.sock");
    const seen: any[] = [];
    server = net.createServer((c) => {
      let buf = "";
      c.setEncoding("utf8");
      c.on("data", (d: string) => {
        buf += d;
        const nl = buf.indexOf("\n");
        if (nl < 0) return;
        const req = JSON.parse(buf.slice(0, nl));
        seen.push(req);
        const out = reply(req);
        if (out === null) return; // never answer
        c.end(out);
      });
    });
    await new Promise<void>((r) => server!.listen(sock, r));
    return { sock, seen };
  };

  test("sends one versioned line and reads one answer", async () => {
    const { sock, seen } = await serve((req) => JSON.stringify({ ok: true, data: req.cmd === "ls" ? { repo: "app", workspaces: [] } : { protocol: 2, min_protocol: 1, app: "0.18.0" } }) + "\n");
    const c = new Client(sock);
    expect(await c.hello()).toEqual({ protocol: 2, min_protocol: 1, app: "0.18.0" });
    expect(await c.ls("/r")).toEqual({ repo: "app", workspaces: [] });
    expect(seen).toEqual([{ v: 2, cmd: "hello" }, { v: 1, cmd: "ls", cwd: "/r", all: false }]);
  });

  test("an answer split across packets is put back together", async () => {
    const { sock } = await serve(() => null);
    server!.removeAllListeners("connection");
    server!.on("connection", (c) => {
      c.once("data", () => {
        c.write('{"ok":true,');
        setTimeout(() => c.end('"data":{"asked":true}}\n'), 20);
      });
    });
    expect(await new Client(sock).propose("/r", { prompt: "x" })).toEqual({ asked: true });
  });

  test("the app's refusal comes back as Refused with its words", async () => {
    const { sock } = await serve(() => '{"ok":false,"error":"There\'s nothing to send. Select some code first."}\n');
    await expect(new Client(sock).draft("mochi", " ")).rejects.toThrow("nothing to send");
  });

  test("an older app reads as Outdated", async () => {
    const { sock } = await serve(() => '{"ok":false,"error":"This nekotrail command and the running app are different versions. Quit and reopen nekotrail, then try again."}\n');
    await expect(new Client(sock).hello()).rejects.toBeInstanceOf(Outdated);
  });

  test("no socket means not running", async () => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "nt-"));
    await expect(new Client(path.join(dir, "missing.sock")).status()).rejects.toBeInstanceOf(NotRunning);
  });

  test("an app that never answers times out", async () => {
    const { sock } = await serve(() => null);
    await expect(new Client(sock).request({ cmd: "status" }, 150)).rejects.toThrow("didn't answer in time");
  });

  test("an app that hangs up without answering is an error, not a hang", async () => {
    const { sock } = await serve(() => "");
    await expect(new Client(sock).status()).rejects.toThrow("without answering");
  });
});
