/**
 * First run: the three-step welcome (agents, your code, try it), the coach-mark tour, and the
 * local-only timer from first launch to the first agent answer. Pure logic, so it can be tested;
 * the components keep their state in localStorage through `KV`.
 */
import type { Agent } from "./api";

/** localStorage, or a Map in tests. */
export type KV = { getItem(k: string): string | null; setItem(k: string, v: string): void };

export const KEYS = {
  /** "agents" | "code" | "try" while the welcome flow runs, "done" after. */
  onboarding: "nekotrail.onboarding",
  /** "seen" once the tour has been shown (finished, skipped or closed). */
  tour: "nekotrail.tour",
  /** FirstRun as JSON. */
  firstRun: "nekotrail.firstRun",
} as const;

export type Step = "agents" | "code" | "try";
export const STEPS: { id: Step; label: string }[] = [
  { id: "agents", label: "Agents" },
  { id: "code", label: "Your code" },
  { id: "try", label: "Try it" },
];

/** Where the welcome flow opens: the saved step, or null once it is done. "Try it" needs a repo. */
export function startStep(saved: string | null, repoCount: number): Step | null {
  if (saved === "done") return null;
  if (saved === "try") return repoCount > 0 ? "try" : "code";
  if (saved === "code") return "code";
  return "agents";
}

/** A repo came in (added, cloned or imported): "Your code" moves on to "Try it". */
export function stepAfterRepos(step: Step | null, before: number, after: number): Step | null {
  return step === "code" && after > before ? "try" : step;
}

/** The setup check, enough to say if the first step can be left: git and one working agent. */
export type DoctorLike = { git: { ok: boolean } } & Partial<Record<Agent, { ok: boolean }>>;
export function agentsReady(d: DoctorLike | null, agents: readonly Agent[]): Agent[] {
  if (!d || !d.git.ok) return [];
  return agents.filter((a) => d[a]?.ok);
}

/** "Where's your code?": the primary action follows what is on this Mac. */
export type CodeAction = "conductor" | "terminal" | "local" | "clone";
export type Found = {
  /** Repos plus workspaces in a Conductor install that aren't here yet; 0 without Conductor. */
  conductor: number;
  /** Terminal chats (Claude Code, Codex, Gemini) not imported yet. */
  terminal: number;
};
export function codeActions(found: Found): { primary: CodeAction; rest: CodeAction[] } {
  const primary: CodeAction = found.conductor > 0 ? "conductor" : found.terminal > 0 ? "terminal" : "local";
  const all: CodeAction[] = ["conductor", "terminal", "local", "clone"];
  const rest = all.filter((a) => a !== primary && (a === "local" || a === "clone" || (a === "conductor" ? found.conductor > 0 : found.terminal > 0)));
  return { primary, rest };
}

/** Terminal chats not in nekotrail yet, from a terminal scan. */
export function newTerminalChats(scan: { repos: { folders: { sessions: { imported: boolean }[] }[] }[] } | null): number {
  if (!scan) return 0;
  let n = 0;
  for (const r of scan.repos) for (const f of r.folders) for (const s of f.sessions) if (!s.imported) n++;
  return n;
}

/** What the backend reads from a repo for the first task: README start, package.json, top-level names. */
export type RepoBrief = { readme: string | null; package_json: string | null; files: string[] };

type Pkg = { name?: string; description?: string; scripts?: Record<string, string>; dependencies?: Record<string, string>; devDependencies?: Record<string, string> };
function parsePkg(text: string | null): Pkg | null {
  if (!text) return null;
  try {
    const p = JSON.parse(text);
    return p && typeof p === "object" ? (p as Pkg) : null;
  } catch {
    return null;
  }
}

/** The package manager command a JS repo uses, from its lockfile. */
function runner(files: string[]): string {
  if (files.includes("bun.lock") || files.includes("bun.lockb")) return "bun run";
  if (files.includes("pnpm-lock.yaml")) return "pnpm";
  if (files.includes("yarn.lock")) return "yarn";
  return "npm run";
}

/** How the repo runs, in a few words the agent should explain. */
function howItRuns(b: RepoBrief, pkg: Pkg | null): string {
  const f = b.files;
  const script = pkg?.scripts && (["dev", "start", "serve"].find((s) => pkg.scripts![s]));
  if (script) return `what \`${runner(f)} ${script}\` does`;
  if (f.includes("Cargo.toml")) return "how to build and test it with cargo";
  if (f.includes("go.mod")) return "how to run and test it with go";
  if (f.includes("pyproject.toml") || f.includes("requirements.txt")) return "how to set it up and run it with Python";
  if (f.includes("Gemfile")) return "how to set it up and run it with Ruby";
  if (f.includes("Makefile")) return "what the Makefile targets do";
  return "how to run it locally";
}

/** Has a test command or a test folder we can name. */
function hasTests(b: RepoBrief, pkg: Pkg | null): boolean {
  if (pkg?.scripts?.test && !/no test specified/.test(pkg.scripts.test)) return true;
  return b.files.some((n) => /^(tests?|__tests__|spec|e2e)\/$/.test(n)) || b.files.includes("Cargo.toml") || b.files.includes("go.mod") || b.files.includes("pytest.ini");
}

/** The README's own name for the project (its first heading), if it has one. */
export function readmeTitle(readme: string | null): string | null {
  if (!readme) return null;
  const m = /^#\s+(.+)$/m.exec(readme) ?? /^(.+)\n=+\s*$/m.exec(readme);
  const t = m?.[1].replace(/<[^>]+>|!\[[^\]]*\]\([^)]*\)|\[([^\]]*)\]\([^)]*\)/g, "$1").replace(/[*_`]/g, "").trim();
  return t && t.length <= 60 ? t : null;
}

/**
 * Suggested first tasks for a repo, best first. Every one only reads: the flow runs it in Plan mode.
 * The first one is always "explain the layout", tailored to what the repo has.
 */
export type Idea = { label: string; text: string };
export function suggestTasks(repoName: string, b: RepoBrief | null): Idea[] {
  const brief: RepoBrief = b ?? { readme: null, package_json: null, files: [] };
  const pkg = parsePkg(brief.package_json);
  const name = readmeTitle(brief.readme) ?? pkg?.name ?? repoName;
  const skip = /^(node_modules|dist|build|target|out|vendor|coverage|public)\/$/;
  const dirs = brief.files.filter((n) => n.endsWith("/") && !skip.test(n)).slice(0, 4).map((n) => n.slice(0, -1));
  const folders = dirs.length ? `the main folders (${dirs.join(", ")})` : "the main folders";
  return [
    { label: "Explain the layout", text: `Explain how ${name} is laid out and where to start: ${folders}, ${howItRuns(brief, pkg)}, and one small first change worth making.` },
    hasTests(brief, pkg)
      ? { label: "Find the gaps in tests", text: "Find how this repo is tested, how to run the tests, and which important parts have no tests yet." }
      : { label: "Plan a first test", text: "I can't see any tests here. Suggest where a first test would help most and how to set it up." },
    brief.readme
      ? { label: "Check the README", text: "Read the README and check it against the code. List anything out of date or missing." }
      : { label: "Outline a README", text: "Outline a short README for this repo: what it does, how to run it, and where things live." },
  ];
}

/** The first-run timer, kept on this Mac only and shown nowhere but Copy diagnostics. Times are ms since the epoch. */
export type FirstRun = {
  /** Installed before this was measured, or the user already had repos: nothing is timed. */
  skipped?: boolean;
  launch?: number;
  agents?: number;
  repo?: number;
  sent?: number;
  answer?: number;
};
export type Mark = "agents" | "repo" | "sent" | "answer";

export function readFirstRun(kv: KV): FirstRun | null {
  try {
    const v = JSON.parse(kv.getItem(KEYS.firstRun) ?? "null");
    return v && typeof v === "object" ? (v as FirstRun) : null;
  } catch {
    return null;
  }
}

/**
 * The first look at the database after launch. A brand-new install starts the clock. Someone who
 * already has repos or workspaces (an update, or local storage was cleared) skips the welcome flow
 * and the tour, and nothing is timed. Runs once: after it, the saved value wins.
 */
export function firstLook(kv: KV, counts: { repos: number; workspaces: number }, now: number): FirstRun {
  const had = readFirstRun(kv);
  if (had) return had;
  const existing = counts.repos > 0 || counts.workspaces > 0;
  const fr: FirstRun = existing ? { skipped: true } : { launch: now };
  kv.setItem(KEYS.firstRun, JSON.stringify(fr));
  if (existing) {
    if (!kv.getItem(KEYS.onboarding)) kv.setItem(KEYS.onboarding, "done");
    if (!kv.getItem(KEYS.tour)) kv.setItem(KEYS.tour, "seen");
  }
  return fr;
}

/** Record a step once. Only a timed first run records anything, and the answer counts only once. */
export function mark(fr: FirstRun | null, what: Mark, now: number): FirstRun | null {
  if (!fr || fr.skipped || fr.launch === undefined || fr[what] !== undefined || fr.answer !== undefined) return null;
  return { ...fr, [what]: now };
}
export function markFirstRun(kv: KV, what: Mark, now = Date.now()): void {
  const next = mark(readFirstRun(kv), what, now);
  if (next) kv.setItem(KEYS.firstRun, JSON.stringify(next));
}

const clock = (ms: number) => {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
};
/** One line for Copy diagnostics. Durations only: no paths, names or prompts. */
export function describeFirstRun(fr: FirstRun | null): string {
  if (!fr || fr.skipped || fr.launch === undefined) return "first run: not timed (set up before this version)";
  const at = (t?: number) => (t === undefined ? null : clock(t - fr.launch!));
  const steps = [["agents", at(fr.agents)], ["repo", at(fr.repo)], ["task sent", at(fr.sent)]].filter(([, v]) => v).map(([k, v]) => `${k} ${v}`);
  const tail = steps.length ? ` (${steps.join(", ")})` : "";
  return fr.answer !== undefined ? `first run: ${clock(fr.answer - fr.launch)} to the first agent answer${tail}` : `first run: no agent answer yet${tail}`;
}

/** A turn that ended in an answer, not in a setup problem. */
export function isAnswer(p: { running: boolean; ok?: boolean; needs?: string | null; more_queued?: boolean }): boolean {
  if (p.running || p.more_queued || p.ok === false) return false;
  return !["install", "login", "limit", "error"].includes(p.needs ?? "");
}

/** The tour: six stops on the main surfaces. `target` is a data-tour value; none means centred. */
export type TourStop = { target: string | null; title: string; body: string };
export const TOUR: TourStop[] = [
  { target: "sidebar", title: "Your workspaces", body: "Every task gets its own workspace: a branch and a folder of its own, so agents never step on each other. A dot means there's news." },
  { target: "chat", title: "Talk to the agent", body: "Describe a change, a bug or a question. Pick the agent, model and mode here. Plan mode reads and suggests; it changes nothing." },
  { target: "changes", title: "Review in Changes", body: "Everything the agent edits shows up here as a diff against the target branch. Comment on a line to send it back to the agent." },
  { target: "prbar", title: "Ship it as a PR", body: "When the work looks right, create the PR from here. CI checks and reviews show up in the same bar." },
  { target: null, title: "⌘K does everything", body: "Press ⌘K to find any workspace or action. ⌘N starts a new workspace, ⌘1 to ⌘5 switch tabs." },
  { target: "new", title: "Run several at once", body: "Start another workspace while this one works: ⌘N, or the + next to a repo's name. Each agent gets its own copy of the repo, and you review them one at a time." },
];

/** The tour shows by itself once: after the first workspace opens, when nothing else is on top. */
export function shouldAutoTour(kv: KV, ctx: { workspaceOpen: boolean; busy: boolean }): boolean {
  return kv.getItem(KEYS.tour) !== "seen" && ctx.workspaceOpen && !ctx.busy;
}
/** Marked as soon as it shows, so a crash or a quit mid-tour never brings it back. */
export function markTourSeen(kv: KV): void {
  kv.setItem(KEYS.tour, "seen");
}
