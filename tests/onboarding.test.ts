// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import {
  agentsReady, codeActions, describeFirstRun, firstLook, isAnswer, KEYS, mark, markFirstRun, markTourSeen, newTerminalChats,
  readFirstRun, readmeTitle, shouldAutoTour, startStep, stepAfterRepos, suggestTasks, TOUR, type KV,
} from "../src/onboarding";

const kv = (init: Record<string, string> = {}): KV & { m: Map<string, string> } => {
  const m = new Map(Object.entries(init));
  return { m, getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};

describe("the welcome flow's steps", () => {
  test("a new install starts on Agents; a saved step comes back; done stays done", () => {
    expect(startStep(null, 0)).toBe("agents");
    expect(startStep("code", 0)).toBe("code");
    expect(startStep("try", 2)).toBe("try");
    expect(startStep("done", 0)).toBeNull();
    expect(startStep("done", 3)).toBeNull();
  });
  test("Try it needs a repo: with none it falls back to Your code", () => {
    expect(startStep("try", 0)).toBe("code");
  });
  test("a repo coming in moves Your code on to Try it, and nothing else", () => {
    expect(stepAfterRepos("code", 0, 1)).toBe("try");
    expect(stepAfterRepos("code", 1, 1)).toBe("code");
    expect(stepAfterRepos("code", 2, 1)).toBe("code"); // a repo removed
    expect(stepAfterRepos("agents", 0, 1)).toBe("agents");
    expect(stepAfterRepos(null, 0, 1)).toBeNull();
  });
  test("Agents can be left with git and one working agent", () => {
    const all = ["claude", "codex", "gemini"] as const;
    expect(agentsReady(null, all)).toEqual([]);
    expect(agentsReady({ git: { ok: true }, claude: { ok: false }, codex: { ok: true } }, all)).toEqual(["codex"]);
    expect(agentsReady({ git: { ok: false }, claude: { ok: true } }, all)).toEqual([]);
    expect(agentsReady({ git: { ok: true } }, all)).toEqual([]);
  });
});

describe("Where's your code? leads with what is on this Mac", () => {
  test("Conductor first, then terminal chats, else a local repo", () => {
    expect(codeActions({ conductor: 5, terminal: 9 })).toEqual({ primary: "conductor", rest: ["terminal", "local", "clone"] });
    expect(codeActions({ conductor: 0, terminal: 9 })).toEqual({ primary: "terminal", rest: ["local", "clone"] });
    expect(codeActions({ conductor: 0, terminal: 0 })).toEqual({ primary: "local", rest: ["clone"] });
  });
  test("only chats not imported yet count", () => {
    const s = (imported: boolean) => ({ imported });
    expect(newTerminalChats(null)).toBe(0);
    expect(newTerminalChats({ repos: [{ folders: [{ sessions: [s(false), s(true)] }, { sessions: [s(false)] }] }, { folders: [] }] })).toBe(2);
    expect(newTerminalChats({ repos: [{ folders: [{ sessions: [s(true)] }] }] })).toBe(0);
  });
});

describe("the suggested first task", () => {
  test("names the project, its folders and how it runs, from the README and package.json", () => {
    const [layout, tests, readme] = suggestTasks("pawprint-web", {
      readme: "# Pawprint\n\nA help desk.",
      package_json: JSON.stringify({ name: "pawprint-web", scripts: { dev: "vite", test: "vitest" } }),
      files: ["bun.lock", "node_modules/", "package.json", "README.md", "src/", "tests/"],
    });
    expect(layout.text).toBe("Explain how Pawprint is laid out and where to start: the main folders (src, tests), what `bun run dev` does, and one small first change worth making.");
    expect(tests.label).toBe("Find the gaps in tests");
    expect(readme.label).toBe("Check the README");
  });
  test("other stacks and no README", () => {
    const rust = suggestTasks("cli", { readme: null, package_json: null, files: ["Cargo.toml", "src/"] });
    expect(rust[0].text).toContain("Explain how cli is laid out");
    expect(rust[0].text).toContain("how to build and test it with cargo");
    expect(rust[2].label).toBe("Outline a README");
    const bare = suggestTasks("notes", null);
    expect(bare[0].text).toBe("Explain how notes is laid out and where to start: the main folders, how to run it locally, and one small first change worth making.");
    expect(bare[1].label).toBe("Plan a first test");
  });
  test("npm's placeholder test script doesn't count as tests; a broken package.json is ignored", () => {
    const t = suggestTasks("x", { readme: null, package_json: '{"scripts":{"test":"echo \\"Error: no test specified\\" && exit 1"}}', files: [] });
    expect(t[1].label).toBe("Plan a first test");
    expect(suggestTasks("x", { readme: null, package_json: "{nope", files: ["pnpm-lock.yaml"] })[0].text).toContain("how to run it locally");
  });
  test("every suggestion only reads, so Plan mode fits", () => {
    for (const i of suggestTasks("x", null)) expect(i.text).not.toMatch(/\b(fix|implement|refactor|delete|rename)\b/i);
  });
  test("the README title: first heading, links and badges stripped, long ones skipped", () => {
    expect(readmeTitle("[![ci](x.svg)](y)\n# [Pawprint](https://x) *app*\n")).toBe("Pawprint app");
    expect(readmeTitle("Pawprint\n========\n")).toBe("Pawprint");
    expect(readmeTitle(`# ${"a".repeat(80)}`)).toBeNull();
    expect(readmeTitle(null)).toBeNull();
  });
});

describe("the first-run clock", () => {
  test("a new install starts it; someone with repos skips the flow, the tour and the clock", () => {
    const fresh = kv();
    expect(firstLook(fresh, { repos: 0, workspaces: 0 }, 1000)).toEqual({ launch: 1000 });
    expect(fresh.m.get(KEYS.onboarding)).toBeUndefined();
    const old = kv();
    expect(firstLook(old, { repos: 2, workspaces: 5 }, 1000)).toEqual({ skipped: true });
    expect(old.m.get(KEYS.onboarding)).toBe("done");
    expect(old.m.get(KEYS.tour)).toBe("seen");
  });
  test("it runs once: later launches keep the saved value, even with repos by then", () => {
    const s = kv();
    firstLook(s, { repos: 0, workspaces: 0 }, 1000);
    expect(firstLook(s, { repos: 3, workspaces: 1 }, 9000)).toEqual({ launch: 1000 });
    expect(s.m.get(KEYS.tour)).toBeUndefined();
  });
  test("each step is recorded once, and nothing after the first answer", () => {
    let fr = mark({ launch: 0 }, "repo", 40_000)!;
    expect(fr).toEqual({ launch: 0, repo: 40_000 });
    expect(mark(fr, "repo", 50_000)).toBeNull();
    fr = mark(mark(fr, "sent", 70_000)!, "answer", 130_000)!;
    expect(fr.answer).toBe(130_000);
    expect(mark(fr, "agents", 140_000)).toBeNull();
    expect(mark({ skipped: true }, "answer", 1)).toBeNull();
    expect(mark(null, "answer", 1)).toBeNull();
  });
  test("markFirstRun reads and writes the store", () => {
    const s = kv();
    markFirstRun(s, "answer", 5); // no clock yet: nothing
    expect(readFirstRun(s)).toBeNull();
    firstLook(s, { repos: 0, workspaces: 0 }, 0);
    markFirstRun(s, "answer", 95_000);
    expect(readFirstRun(s)).toEqual({ launch: 0, answer: 95_000 });
    expect(readFirstRun(kv({ [KEYS.firstRun]: "{broken" }))).toBeNull();
  });
  test("diagnostics: durations only", () => {
    expect(describeFirstRun({ launch: 0, agents: 20_000, repo: 52_000, sent: 90_400, answer: 161_000 })).toBe("first run: 2m 41s to the first agent answer (agents 0m 20s, repo 0m 52s, task sent 1m 30s)");
    expect(describeFirstRun({ launch: 0, repo: 52_000 })).toBe("first run: no agent answer yet (repo 0m 52s)");
    expect(describeFirstRun({ launch: 0 })).toBe("first run: no agent answer yet");
    expect(describeFirstRun({ skipped: true })).toBe("first run: not timed (set up before this version)");
    expect(describeFirstRun(null)).toBe("first run: not timed (set up before this version)");
  });
  test("an answer is a finished turn, not a setup problem or a queued follow-up", () => {
    expect(isAnswer({ running: false, ok: true })).toBe(true);
    expect(isAnswer({ running: false, needs: "plan" })).toBe(true);
    expect(isAnswer({ running: true })).toBe(false);
    expect(isAnswer({ running: false, ok: false })).toBe(false);
    expect(isAnswer({ running: false, more_queued: true })).toBe(false);
    for (const n of ["install", "login", "limit", "error"]) expect(isAnswer({ running: false, needs: n })).toBe(false);
  });
});

describe("the tour", () => {
  test("six stops on the main surfaces", () => {
    expect(TOUR.map((t) => t.target)).toEqual(["sidebar", "chat", "changes", "prbar", null, "new"]);
    for (const t of TOUR) expect(`${t.title} ${t.body}`).not.toContain(String.fromCharCode(0x2014));
  });
  test("shows by itself once a workspace is open and nothing is on top", () => {
    expect(shouldAutoTour(kv(), { workspaceOpen: true, busy: false })).toBe(true);
    expect(shouldAutoTour(kv(), { workspaceOpen: false, busy: false })).toBe(false);
    expect(shouldAutoTour(kv(), { workspaceOpen: true, busy: true })).toBe(false);
  });
  test("never twice: seen is set when it shows, and existing users count as seen", () => {
    const s = kv();
    markTourSeen(s);
    expect(shouldAutoTour(s, { workspaceOpen: true, busy: false })).toBe(false);
    const old = kv();
    firstLook(old, { repos: 1, workspaces: 0 }, 0);
    expect(shouldAutoTour(old, { workspaceOpen: true, busy: false })).toBe(false);
  });
});
