import { useEffect, useState, type FocusEvent } from "react";
import { ArrowLeft, Check, Download, FolderOpen, GitFork, Play, SquareTerminal } from "lucide-react";
import { api, errText, type Agent, type ImportReport, type Repo } from "../api";
import { AGENTS, AGENT_INFO } from "../agents";
import { IS_MAC, keyHint, mod } from "../platform";
import { codeActions, newTerminalChats, STEPS, suggestTasks, type CodeAction, type Found, type RepoBrief, type Step } from "../onboarding";
import { Mascot, type Mood } from "./Mascot";
import { SetupCheck } from "./Welcome";

/**
 * Focus the step's primary button on load, so Return goes on, without the keyboard focus ring:
 * the ring would read as "already pressed". It shows again once you Tab away and back.
 */
const quietFocus = {
  ref: (el: HTMLButtonElement | null) => {
    if (!el || el.dataset.focused) return;
    el.dataset.focused = "1";
    el.dataset.quietFocus = "1";
    el.focus({ preventScroll: true });
  },
  onBlur: (e: FocusEvent<HTMLButtonElement>) => { delete e.currentTarget.dataset.quietFocus; },
};

/** Step 1 of 3 and where you are, as a list a screen reader can read. */
function Progress({ step }: { step: Step }) {
  const at = STEPS.findIndex((s) => s.id === step);
  return (
    <ol className="fr-steps" aria-label={`Step ${at + 1} of ${STEPS.length}`}>
      {STEPS.map((s, i) => (
        <li key={s.id} className={i < at ? "done" : i === at ? "on" : ""} aria-current={i === at ? "step" : undefined}>
          <span className="fr-dot num" aria-hidden>{i < at ? <Check size={11} strokeWidth={3} /> : i + 1}</span>{s.label}
        </li>
      ))}
    </ol>
  );
}

export type StartRequest = { repoId: string; agent: Agent; prompt: string };

/**
 * The first run, one screen at a time: the agents on this Mac, where your code is, then a first
 * task in Plan mode so nothing in the repo changes. Shown until the first task starts or you skip it.
 */
export function FirstRun({
  step, onStep, repos, canImport, mood, purring, onPet, onAddRepo, onClone, onImportConductor, onImportTerminal, onStart,
}: {
  step: Step;
  onStep: (s: Step | "done") => void;
  repos: Repo[];
  canImport: boolean;
  mood: Mood; purring: boolean; onPet: () => void;
  onAddRepo: () => void;
  onClone: () => void;
  onImportConductor: () => Promise<void>;
  onImportTerminal: () => void;
  onStart: (r: StartRequest) => Promise<void>;
}) {
  // Working agents from step 1; a relaunch straight into "Try it" asks which CLIs are on PATH instead.
  const [ready, setReady] = useState<Agent[] | null>(null);
  return (
    <div className="welcome first-run">
      <Progress step={step} />
      {step === "agents" && <AgentsStep mood={mood} purring={purring} onPet={onPet} ready={ready} onReady={setReady} onNext={() => onStep("code")} onSkip={() => onStep("done")} />}
      {step === "code" && (
        <CodeStep repos={repos} canImport={canImport} onAddRepo={onAddRepo} onClone={onClone} onImportConductor={onImportConductor} onImportTerminal={onImportTerminal}
          onBack={() => onStep("agents")} onNext={() => onStep("try")} />
      )}
      {step === "try" && <TryStep repos={repos} ready={ready} onBack={() => onStep("code")} onSkip={() => onStep("done")} onStart={onStart} />}
    </div>
  );
}

function AgentsStep({ mood, purring, onPet, ready, onReady, onNext, onSkip }: {
  mood: Mood; purring: boolean; onPet: () => void; ready: Agent[] | null; onReady: (a: Agent[]) => void; onNext: () => void; onSkip: () => void;
}) {
  const [ghOk, setGhOk] = useState(true);
  const can = !!ready?.length;
  return (
    <>
      <Mascot size={64} mood={mood === "working" ? "working" : "happy"} follow purring={purring} onClick={onPet} title="Pet the cat" />
      <h1>Welcome to nekotrail.</h1>
      <p className="lede">Let's get you a first answer from an agent, in about 3 minutes. First, the agents on {IS_MAC ? "this Mac" : "this computer"}. nekotrail uses your own logins and subscriptions.</p>
      <SetupCheck heading={IS_MAC ? "Agents on this Mac" : "Agents on this computer"} foldMissing onStatus={(s) => { onReady(s.ready); setGhOk(s.ghOk); }} />
      {can && !ghOk && <p className="muted fr-note">GitHub sign-in is only needed for pull requests. You can do it later.</p>}
      <div className="fr-actions">
        <button className="quiet" onClick={onSkip}>Skip setup</button>
        <span className="spacer" />
        <button className="primary" {...quietFocus} disabled={!can} onClick={onNext}>Continue</button>
      </div>
    </>
  );
}

const ACTION: Record<CodeAction, { label: string; icon: React.ReactNode }> = {
  conductor: { label: "Import from Conductor…", icon: <Download size={15} /> },
  terminal: { label: "Import terminal chats", icon: <SquareTerminal size={15} /> },
  local: { label: "Open a local repo", icon: <FolderOpen size={15} /> },
  clone: { label: "Clone from GitHub", icon: <GitFork size={15} /> },
};

function CodeStep({ repos, canImport, onAddRepo, onClone, onImportConductor, onImportTerminal, onBack, onNext }: {
  repos: Repo[]; canImport: boolean;
  onAddRepo: () => void; onClone: () => void; onImportConductor: () => Promise<void>; onImportTerminal: () => void;
  onBack: () => void; onNext: () => void;
}) {
  const [conductor, setConductor] = useState<ImportReport | null>(null);
  const [terminal, setTerminal] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (canImport) api.importPreview().then(setConductor).catch(() => setConductor(null));
    api.terminalScan().then((s) => setTerminal(newTerminalChats(s))).catch(() => setTerminal(0));
  }, [canImport]);
  // Local repo and Clone are always there; the lookups only decide which one leads.
  const looking = terminal === null || (canImport && conductor === null);
  const found: Found = { conductor: conductor ? conductor.repos + conductor.workspaces : 0, terminal: terminal ?? 0 };
  const { primary, rest } = codeActions(found);
  const run = async (a: CodeAction) => {
    if (a === "local") onAddRepo();
    else if (a === "clone") onClone();
    else if (a === "terminal") onImportTerminal();
    else { setBusy(true); try { await onImportConductor(); } finally { setBusy(false); } }
  };
  const hero = primary === "conductor" && conductor ? {
    title: "Pick up where you left off in Conductor",
    body: `Found ${conductor.repos} ${conductor.repos === 1 ? "repo" : "repos"}, ${conductor.workspaces} ${conductor.workspaces === 1 ? "workspace" : "workspaces"} and ${conductor.sessions} ${conductor.sessions === 1 ? "chat" : "chats"}. nekotrail brings them over and moves their folders out of Conductor's into its own, since Conductor says it will delete all user data when its app shuts down in early 2027. You see exactly what moves first, and chats continue where they left off.`,
  } : primary === "terminal" ? {
    title: "Bring in your terminal chats",
    body: `Found ${found.terminal} ${found.terminal === 1 ? "chat" : "chats"} you ran with Claude Code, Codex or Gemini in a terminal. You pick which ones, and their repos come along. Read-only.`,
  } : {
    title: "Open a repo you work on",
    body: "Pick a folder with a git repo in it, or clone one from GitHub. Your main checkout never changes: every task works in its own copy.",
  };
  return (
    <>
      <h1>Where's your code?</h1>
      <p className="lede">Pick one repo to start with. You can add more any time.</p>
      {looking ? (
        <p className="muted fr-looking"><span className="spinner" /> Looking for Conductor and terminal chats on {IS_MAC ? "this Mac" : "this computer"}…</p>
      ) : (
        <div className="import-card hero fr-hero">
          <h2>{hero.title}</h2>
          <p>{hero.body}</p>
          <button className="primary" {...quietFocus} disabled={busy} onClick={() => run(primary)}>
            {busy ? <span className="spinner" /> : ACTION[primary].icon}{ACTION[primary].label}
          </button>
        </div>
      )}
      <div className="start-actions" aria-label="Other ways to add code">
        {(looking ? (["local", "clone"] as CodeAction[]) : rest).map((a) => (
          <button key={a} disabled={busy} onClick={() => run(a)}>{ACTION[a].icon}{ACTION[a].label}</button>
        ))}
      </div>
      <div className="fr-actions">
        <button className="quiet" onClick={onBack}><ArrowLeft size={14} />Back</button>
        <span className="spacer" />
        {repos.length > 0 && <button onClick={onNext}>Continue with {repos.length === 1 ? repos[0].name : `${repos.length} repos`}</button>}
      </div>
    </>
  );
}

function TryStep({ repos, ready, onBack, onSkip, onStart }: {
  repos: Repo[]; ready: Agent[] | null; onBack: () => void; onSkip: () => void; onStart: (r: StartRequest) => Promise<void>;
}) {
  const [repoId, setRepoId] = useState(repos[0]?.id ?? "");
  const repo = repos.find((r) => r.id === repoId) ?? repos[0];
  const [agents, setAgents] = useState<Agent[]>(ready ?? []);
  useEffect(() => {
    if (ready?.length) return setAgents(ready);
    api.installedAgents().then((a) => setAgents(AGENTS.filter((x) => a.includes(x)))).catch(() => setAgents(["claude"]));
  }, [ready]);
  const [agent, setAgent] = useState<Agent | "">("");
  const pick = agent || agents[0] || "claude";
  const [brief, setBrief] = useState<RepoBrief | null | undefined>(undefined);
  useEffect(() => {
    if (!repo) return;
    setBrief(undefined);
    api.repoBrief(repo.id).then(setBrief).catch(() => setBrief(null));
  }, [repo?.id]);
  const ideas = brief === undefined || !repo ? [] : suggestTasks(repo.name, brief);
  const [text, setText] = useState("");
  // The text follows the suggestion until you type in it.
  const [typed, setTyped] = useState(false);
  useEffect(() => { if (!typed && ideas[0]) setText(ideas[0].text); }, [ideas[0]?.text, typed]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const start = async () => {
    if (!repo || !text.trim() || busy) return;
    setBusy(true);
    setError(null);
    try { await onStart({ repoId: repo.id, agent: pick, prompt: text.trim() }); } catch (e) { setError(errText(e)); setBusy(false); }
  };
  if (!repo) return null;
  return (
    <>
      <h1>Try it</h1>
      <p className="lede">Ask {AGENT_INFO[pick].name} about {repo.name}. It runs in Plan mode: it reads your code and suggests, and changes nothing.</p>
      <div className="fr-try">
        <div className="fr-fields">
          {repos.length > 1 && (
            <label>Repo
              <select value={repo.id} onChange={(e) => { setRepoId(e.target.value); setTyped(false); }}>
                {repos.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
              </select>
            </label>
          )}
          {agents.length > 1 && (
            <label>Agent
              <select value={pick} onChange={(e) => setAgent(e.target.value as Agent)}>
                {agents.map((a) => <option key={a} value={a}>{AGENT_INFO[a].full}</option>)}
              </select>
            </label>
          )}
          <span className="chip fr-mode" title="Plan mode only reads. Switch modes later from the chat.">Plan mode, read only</span>
        </div>
        <textarea
          className="fr-prompt"
          aria-label="Your first task"
          rows={4}
          value={brief === undefined ? "" : text}
          placeholder={brief === undefined ? "Reading the repo for a good first task…" : "Ask anything about this repo"}
          onChange={(e) => { setText(e.target.value); setTyped(true); }}
          onKeyDown={(e) => { if (e.key === "Enter" && mod(e)) { e.preventDefault(); start(); } }}
        />
        {ideas.length > 1 && (
          <div className="fr-ideas" role="group" aria-label="Other ideas">
            <span className="muted">Or try:</span>
            {ideas.map((t) => (
              <button key={t.label} className={`fr-idea${text === t.text ? " on" : ""}`} aria-pressed={text === t.text} title={t.text} onClick={() => { setText(t.text); setTyped(true); }}>
                {t.label}
              </button>
            ))}
          </div>
        )}
        {error && <p className="banner error" role="alert">{error}</p>}
        <p className="muted fr-note">The agent works in a new workspace of its own. While it works, a short tour shows you around.</p>
      </div>
      <div className="fr-actions">
        <button className="quiet" onClick={onBack}><ArrowLeft size={14} />Back</button>
        <button className="quiet" onClick={onSkip}>Skip, I'll look around</button>
        <span className="spacer" />
        <span className="muted fr-hint">{keyHint("⌘↵")}</span>
        <button className="primary" disabled={busy || !text.trim() || brief === undefined} onClick={start}>
          {busy ? <span className="spinner" /> : <Play size={14} />}Start in Plan mode
        </button>
      </div>
    </>
  );
}
