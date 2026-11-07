// End-to-end user journeys against the real app. Run through scripts/e2e.sh, which builds it.
//
//   bun tests/e2e/run.ts <path to the debug nekotrail binary>
//
// The app runs with no window (NEKOTRAIL_E2E=1), in a throwaway home folder, with fake `claude`,
// `codex` and `gh` first on PATH (tests/e2e/bin). The journeys go through the app's `nekotrail`
// socket, whose test door calls the same commands the window calls (src-tauri/src/e2e.rs), and
// check what was saved and which events the window would have seen.
//
// Every wait has a deadline, and the whole run is killed after RUN_LIMIT_MS, so a hang in the
// app fails the run with the step it hung in instead of waiting forever.

import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { join, resolve } from "node:path";

const RUN_LIMIT_MS = 240_000;
const STEP_MS = 15_000;

const bin = resolve(process.argv[2] ?? "");
if (!process.argv[2] || !existsSync(bin)) {
  console.error("usage: bun tests/e2e/run.ts <path to the debug nekotrail binary>");
  process.exit(2);
}

// /tmp, not $TMPDIR: the socket path must stay under the 104-byte limit for Unix sockets.
const root = mkdtempSync("/tmp/nte2e-");
const home = join(root, "home");
const repo = join(root, "repo");
const agentLog = join(root, "agents.log");
const appLog = join(root, "app.log");
const fakes = resolve(import.meta.dir, "bin");
const sock = join(home, "Library", "Application Support", "dev.nekotrail.app", "cli", "nekotrail.sock");

let app: ChildProcess | null = null;
let step = "starting";
let failed = false;
const started = Date.now();

function cleanup() {
  if (app && app.exitCode === null) app.kill("SIGKILL");
  // Fake agents left running ([hang], [linger]) by a failed run.
  spawnSync("pkill", ["-f", `${fakes}/`]);
  if (!process.env.E2E_KEEP) rmSync(root, { recursive: true, force: true });
}

function fail(msg: string): never {
  failed = true;
  console.error(`\nFAIL in "${step}": ${msg}`);
  const tail = existsSync(appLog) ? readFileSync(appLog, "utf8").split("\n").slice(-30).join("\n") : "";
  if (tail.trim()) console.error(`--- last lines the app printed ---\n${tail}`);
  if (existsSync(agentLog)) console.error(`--- agent turns ---\n${readFileSync(agentLog, "utf8")}`);
  cleanup();
  process.exit(1);
}

setTimeout(() => fail(`the whole run took over ${RUN_LIMIT_MS / 1000} s`), RUN_LIMIT_MS).unref();

function git(dir: string, ...args: string[]) {
  const r = spawnSync("git", args, { cwd: dir, encoding: "utf8", env: gitEnv() });
  if (r.status !== 0) fail(`git ${args.join(" ")}: ${r.stderr}`);
}

function gitEnv() {
  return {
    ...process.env, HOME: home, GIT_CONFIG_NOSYSTEM: "1",
    GIT_AUTHOR_NAME: "E2E", GIT_AUTHOR_EMAIL: "e2e@example.com", GIT_COMMITTER_NAME: "E2E", GIT_COMMITTER_EMAIL: "e2e@example.com",
  };
}

/** One request on the socket, one answer back. Rejects after `ms`. */
function call(op: string, args: Record<string, unknown> = {}, ms = STEP_MS): Promise<any> {
  return new Promise((ok, no) => {
    const c = createConnection(sock);
    let buf = "";
    const timer = setTimeout(() => { c.destroy(); no(new Error(`no answer to "${op}" within ${ms / 1000} s (the app is stuck)`)); }, ms);
    c.on("connect", () => c.write(JSON.stringify({ e2e: op, ...args }) + "\n"));
    c.on("data", (d) => (buf += d));
    c.on("error", (e) => { clearTimeout(timer); no(e); });
    c.on("close", () => {
      clearTimeout(timer);
      try {
        const a = JSON.parse(buf);
        a.ok ? ok(a.data) : no(new Error(`"${op}" failed: ${a.error}`));
      } catch {
        no(new Error(`"${op}": bad answer ${JSON.stringify(buf)}`));
      }
    });
  });
}

async function $(op: string, args: Record<string, unknown> = {}, ms = STEP_MS): Promise<any> {
  try {
    return await call(op, args, ms);
  } catch (e) {
    fail((e as Error).message);
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Poll `check` until it returns a truthy value, or fail with `what` after `ms`. */
async function until<T>(what: string, check: () => Promise<T | undefined | false>, ms = STEP_MS): Promise<T> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const v = await check();
    if (v) return v;
    await sleep(100);
  }
  fail(`timed out after ${ms / 1000} s waiting for: ${what}`);
}

const running = async (sid: string) => ((await $("snapshot")).running as string[]).includes(sid);
const idle = (sid: string, ms = STEP_MS) => until(`the chat's turn to end (spinner off)`, async () => !(await running(sid)), ms);
const events = (sid: string) => $("events", { session_id: sid }) as Promise<any[]>;
const userTexts = async (sid: string) => (await events(sid)).filter((e) => e.kind === "user").map((e) => e.text as string);
const seenSince = (n: number) => $("seen", { since: n }) as Promise<any[]>;
const agentTurns = () => (existsSync(agentLog) ? readFileSync(agentLog, "utf8").trim().split("\n").filter(Boolean) : []);
const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch { return false; } };

async function journey(name: string, body: () => Promise<void>) {
  step = name;
  const t = Date.now();
  await body();
  console.log(`  ok  ${name} (${((Date.now() - t) / 1000).toFixed(1)} s)`);
}

// ---------- setup ----------

mkdirSync(home, { recursive: true });
mkdirSync(repo, { recursive: true });
git(repo, "init", "-q", "-b", "main");
writeFileSync(join(repo, "README.md"), "# demo\n");
git(repo, "add", "-A");
git(repo, "commit", "-qm", "first");

const path = `${fakes}:/usr/bin:/bin:/usr/sbin:/sbin`;
app = spawn(bin, [], {
  env: {
    ...gitEnv(), HOME: home, ZDOTDIR: home, SHELL: "/bin/zsh", PATH: path, NEKOTRAIL_E2E: "1", NEKOTRAIL_E2E_PATH: path,
    FAKE_AGENT_LOG: agentLog, RUST_BACKTRACE: "1",
  },
  stdio: ["ignore", "pipe", "pipe"],
});
const out: string[] = [];
const keep = (d: Buffer) => { out.push(d.toString()); writeFileSync(appLog, out.join("")); };
app.stdout!.on("data", keep);
app.stderr!.on("data", keep);
app.on("exit", (code, sig) => { if (!failed && step !== "done") fail(`the app exited (code ${code}, signal ${sig})`); });

console.log(`e2e: ${bin}\n      data in ${root}`);
step = "app starts and answers on its socket";
await until("the app's socket", async () => existsSync(sock) && (await call("ping", {}, 2000).catch(() => null)), 30_000);
const pong = await $("ping");
if (!String(pong.home).startsWith(home)) fail(`the app uses the real home folder (${pong.home}), not the test one`);

// ---------- journeys ----------

let ws = "", sid = "";

await journey("create a workspace and a Claude chat", async () => {
  const r = await $("add_repo", { path: repo });
  const w = await $("create_workspace", { repo_id: r.id });
  ws = w.id;
  if (!existsSync(w.path)) fail(`the workspace folder ${w.path} wasn't made`);
  sid = (await $("new_session", { workspace_id: ws, agent: "claude" })).id;
});

await journey("a greeting gets an answer, the turn ends, the spinner stops", async () => {
  const n = (await seenSince(0)).length;
  await $("send", { session_id: sid, text: "Hey there" });
  await idle(sid);
  const ev = await events(sid);
  if (!ev.some((e) => e.kind === "user" && e.text === "Hey there")) fail("the message isn't in the chat");
  if (!ev.some((e) => e.kind === "text" && e.text === "ok")) fail("the answer isn't in the chat");
  if (!ev.some((e) => e.kind === "done")) fail("no Done line");
  const seen = await seenSince(n);
  const off = seen.find((e) => e.event === "running" && e.payload.session_id === sid && e.payload.running === false);
  if (!off) fail(`the window never got running:false (it got: ${[...new Set(seen.map((e) => e.event))].join(", ")})`);
  const w = await $("workspace", { id: ws });
  if (w.title !== "") fail(`a greeting named the workspace "${w.title}"`);
});

await journey("the terminal starts, shows a prompt and runs what you type", async () => {
  const id = `${ws}:term`;
  const w = await $("workspace", { id: ws });
  const n = (await seenSince(0)).length;
  await $("pty_open", { id, cwd: w.path });
  const output = async () => (await seenSince(n)).filter((e) => e.event === "pty" && e.payload.id === id).map((e) => e.payload.data).join("");
  await until("the shell's first output (its prompt)", async () => (await output()).trim().length > 0);
  await $("pty_write", { id, data: "echo e2e-$((6*7))\r" });
  await until("the typed command's output (e2e-42)", async () => (await output()).includes("e2e-42"));
});

await journey("a message sent while the agent works is queued, then runs next, in order", async () => {
  writeFileSync(agentLog, "");
  await $("send", { session_id: sid, text: "[slow] add a health check" });
  await until("the turn to start", () => running(sid));
  await $("send", { session_id: sid, text: "then add a test for it" });
  const q = await $("queued", { session_id: sid });
  if (JSON.stringify(q) !== JSON.stringify(["then add a test for it"])) fail(`queued is ${JSON.stringify(q)}, not the second message`);
  await until("both turns to finish", async () => agentTurns().filter((l) => l.startsWith("end")).length >= 2 && !(await running(sid)), 25_000);
  const order = agentTurns().map((l) => l.split(" ")[0] + ":" + (l.includes("[slow]") ? "1" : "2"));
  if (order.join(",") !== "start:1,end:1,start:2,end:2") fail(`turns ran as ${order.join(",")}`);
  const texts = await userTexts(sid);
  if (texts.slice(-2).join("|") !== "[slow] add a health check|then add a test for it") fail(`chat shows ${JSON.stringify(texts)}`);
  if ((await $("queued", { session_id: sid })).length !== 0) fail("the queue isn't empty after both ran");
  const w = await $("workspace", { id: ws });
  if (!/health check/i.test(w.title)) fail(`the workspace title is "${w.title}", not from the first real task`);
});

await journey("a message sent after the turn ended runs at once", async () => {
  await $("send", { session_id: sid, text: "and update the readme" });
  await idle(sid);
  if ((await userTexts(sid)).at(-1) !== "and update the readme") fail("the third message isn't in the chat");
});

await journey("an agent that stays alive after answering still ends the turn", async () => {
  writeFileSync(agentLog, "");
  const t = Date.now();
  await $("send", { session_id: sid, text: "[linger] answer and stay" });
  await idle(sid, 15_000);
  const pid = Number(agentTurns()[0]?.split(" ")[1]);
  if (Date.now() - t > 12_000) fail("the turn took too long to end");
  await until("the lingering agent to be stopped", async () => !alive(pid), 5000);
});

await journey("Stop ends a turn that never answers", async () => {
  writeFileSync(agentLog, "");
  await $("send", { session_id: sid, text: "[hang] think forever" });
  await until("the turn to start", () => running(sid));
  await until("the agent to start", async () => agentTurns().length > 0);
  await $("stop", { session_id: sid });
  await idle(sid, 15_000);
  const last = (await events(sid)).at(-1);
  if (!(last?.kind === "done" && last.stopped)) fail(`the chat ends with ${JSON.stringify(last)}, not a stop`);
});

await journey("the terminal answers while a turn runs", async () => {
  const id = `${ws}:term`;
  await $("send", { session_id: sid, text: "[slow] refactor the parser" });
  await until("the turn to start", () => running(sid));
  const n = (await seenSince(0)).length;
  await $("pty_write", { id, data: "echo during-$((2+3))\r" });
  await until("terminal output while the agent works", async () =>
    (await seenSince(n)).some((e) => e.event === "pty" && e.payload.id === id && String(e.payload.data).includes("during-5")));
  if (!(await running(sid))) fail("the turn had already ended; this didn't test anything");
  await idle(sid);
});

await journey("a Codex chat runs a turn to the end", async () => {
  const s = (await $("new_session", { workspace_id: ws, agent: "codex" })).id;
  await $("send", { session_id: s, text: "list the files" });
  await idle(s);
  if (!(await events(s)).some((e) => e.kind === "text" && e.text === "ok")) fail("no answer in the Codex chat");
});

await journey("the Queue screen loads", async () => {
  const v = await $("queue_view");
  if (!Array.isArray(v.tasks) || typeof v.max_total !== "number") fail(`odd queue data: ${JSON.stringify(v).slice(0, 200)}`);
});

await journey("archive the workspace", async () => {
  await $("archive", { id: ws, force: true });
  const snap = await $("snapshot");
  if ((snap.workspaces as any[]).some((w) => w.id === ws)) fail("the workspace is still listed");
});

step = "done";
const log = out.join("");
if (/panicked at/.test(log)) fail("the app panicked (see above)");
console.log(`e2e: all journeys passed in ${((Date.now() - started) / 1000).toFixed(1)} s`);
cleanup();
process.exit(0);
