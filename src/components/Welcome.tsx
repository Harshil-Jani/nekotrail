import { useEffect, useMemo, useState } from "react";
import { Check, ChevronRight, CircleAlert, Copy, Download, FolderInput, FolderOpen, GitFork, Lock, Plus, RotateCw, Search, SquareTerminal, X } from "lucide-react";
import { api, errText, type Agent, type Doctor, type GithubUser, type ImportReport, type Repo } from "../api";
import { IS_MAC, keyHint } from "../platform";
import { AGENTS, AGENT_INFO } from "../agents";
import { agentsReady } from "../onboarding";
import { Mascot, type Mood } from "./Mascot";

/** A command the user can copy, for anything that has to be installed or signed in. */
function Cmd({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <span className="cmd">
      <code>{text}</code>
      <button className="icon" aria-label={`Copy ${text}`} title="Copy command" onClick={() => { navigator.clipboard.writeText(text); setCopied(true); setTimeout(() => setCopied(false), 1500); }}>
        {copied ? <Check size={13} /> : <Copy size={13} />}
      </button>
    </span>
  );
}

function CheckRow({ ok, warn, title, detail, fix }: { ok: boolean; warn?: boolean; title: string; detail?: string; fix?: React.ReactNode }) {
  return (
    <li className={`check-row ${ok ? "ok" : warn ? "warn" : "bad"}`}>
      {ok ? <Check size={15} /> : <CircleAlert size={15} />}
      <div>
        <b>{title}</b>{detail && <span className="muted"> · {detail}</span>}
        {!ok && fix && <div className="check-fix">{fix}</div>}
      </div>
    </li>
  );
}

/** The setup checklist: what nekotrail needs on this Mac, and the command for anything missing. */
export function SetupCheck({ compact = false, heading, foldMissing = false, onStatus }: {
  compact?: boolean;
  /** With an agent working, the ones you could add all fold away (the first-run flow keeps it short). */
  foldMissing?: boolean;
  /** Replaces "Finish setup" / "All set" (the first-run flow says "Agents on this Mac"). */
  heading?: string;
  /** `ready`: git and these agents work, enough to start. */
  onStatus?: (s: { allOk: boolean; ghOk: boolean; ready: Agent[] }) => void;
}) {
  const [d, setD] = useState<Doctor | null>(null);
  const [busy, setBusy] = useState(false);
  const run = () => {
    setBusy(true);
    api.doctor()
      .then((x) => { setD(x); onStatus?.({ allOk: x.git.ok && x.gh.ok && AGENTS.some((a) => x[a]?.ok), ghOk: x.gh.ok, ready: agentsReady(x, AGENTS) }); })
      .catch(() => {})
      .finally(() => setBusy(false));
  };
  useEffect(run, []);
  if (!d) return <p className="muted"><span className="spinner" /> Checking your setup…</p>;
  const agentOk = AGENTS.some((a) => d[a]?.ok);
  // The first other agent that works, for "optional, Codex is set up".
  const otherOk = (a: Agent) => AGENTS.find((b) => b !== a && d[b]?.ok);
  const allOk = d.git.ok && d.gh.ok && agentOk;
  if (allOk && compact) return null;
  // The first four agents and every installed one get a row; the rest fold away.
  // Installed ones lead, so what you have reads before what you could add.
  const front = [...AGENTS.filter((a) => d[a]?.ok), ...AGENTS.filter((a, i) => i < 4 && !d[a]?.ok && !(foldMissing && agentOk))];
  const rest = AGENTS.filter((a) => !front.includes(a));
  const row = (a: Agent) => {
    const other = otherOk(a);
    return (
      <CheckRow
        key={a}
        ok={!!d[a]?.ok}
        warn={!!other}
        title={AGENT_INFO[a].full}
        detail={d[a]?.ok ? d[a].version : other ? `optional, ${AGENT_INFO[other].full} is set up` : "not found"}
        fix={<><Cmd text={AGENT_INFO[a].install} /><span className="muted">Then run <code>{AGENT_INFO[a].setup}</code> once to sign in.</span></>}
      />
    );
  };
  return (
    <section className="setup-check">
      <div className="row">
        <h2>{heading ?? (allOk ? "All set" : "Finish setup (about 2 minutes)")}</h2>
        <span className="spacer" />
        <button className="quiet" onClick={run} disabled={busy}>{busy ? <span className="spinner" /> : <RotateCw size={13} />}Check again</button>
      </div>
      <ul>
        <CheckRow ok={d.git.ok} title="git" detail={d.git.version.replace("git version ", "")} fix={<Cmd text={IS_MAC ? "xcode-select --install" : "sudo apt install git"} />} />
        <CheckRow
          ok={d.gh.ok}
          title="GitHub CLI, signed in"
          detail={d.gh.login ? `@${d.gh.login}` : d.gh.installed ? "installed, not signed in" : "not installed"}
          fix={<>{!d.gh.installed && <Cmd text={IS_MAC ? "brew install gh" : "sudo apt install gh"} />}<Cmd text="gh auth login" /><span className="muted">For PRs, CI status and avatars.</span></>}
        />
        {front.map(row)}
      </ul>
      {rest.length > 0 && (
        <details className="setup-more">
          <summary><ChevronRight size={15} aria-hidden /><span className="muted">{rest.length} more agents you can add: {rest.map((a) => AGENT_INFO[a].name).join(", ")}</span></summary>
          <ul>{rest.map(row)}</ul>
        </details>
      )}
      {!agentOk && <p className="muted">You need at least one agent, such as Claude Code, Codex, Gemini CLI or OpenCode. nekotrail uses your own login and subscription.</p>}
    </section>
  );
}

/** Clone one of your GitHub repos (or any owner/name) into ~/nekotrail/repos. */
export function CloneSheet({ onDone, onClose }: { onDone: (r: Repo) => void; onClose: () => void }) {
  const [list, setList] = useState<Awaited<ReturnType<typeof api.githubRepos>> | null>(null);
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.githubRepos().then(setList).catch((e) => { setList([]); setError(errText(e)); }); }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const shown = useMemo(() => (list ?? []).filter((r) => r.nameWithOwner.toLowerCase().includes(q.trim().toLowerCase())).slice(0, 50), [list, q]);
  const clone = async (spec: string) => {
    setBusy(spec);
    setError(null);
    try { onDone(await api.cloneRepo(spec)); } catch (e) { setError(errText(e)); } finally { setBusy(null); }
  };
  const typed = /^[\w.-]+\/[\w.-]+$/.test(q.trim()) || /^(https?:|git@)/.test(q.trim());
  return (
    <div className="palette-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="palette clone" role="dialog" aria-modal="true" aria-label="Clone from GitHub">
        <div className="palette-input">
          <GitFork size={16} />
          <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search your repos, or type owner/name"
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); const t = typed ? q.trim() : shown[0]?.nameWithOwner; if (t) clone(t); } }} />
          <button className="icon" aria-label="Close" onClick={onClose}><X size={15} /></button>
        </div>
        <div className="palette-list">
          {error && <p className="banner error">{error}</p>}
          {list === null && <p className="muted palette-empty"><span className="spinner" /> Loading your repos…</p>}
          {typed && !shown.some((r) => r.nameWithOwner === q.trim()) && (
            <button onClick={() => clone(q.trim())} disabled={!!busy}>
              <span className="palette-icon"><Download size={15} /></span><span className="palette-label">Clone {q.trim()}</span>
            </button>
          )}
          {shown.map((r) => (
            <button key={r.nameWithOwner} onClick={() => clone(r.nameWithOwner)} disabled={!!busy}>
              <span className="palette-icon">{busy === r.nameWithOwner ? <span className="spinner" /> : r.isPrivate ? <Lock size={14} /> : <Search size={14} />}</span>
              <span className="palette-label">{r.nameWithOwner}<small className="muted"> {r.description}</small></span>
              <span className="palette-hint">{busy === r.nameWithOwner ? "Cloning…" : ""}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

/** What you see with no workspace open: setup, ways to start, and the Conductor import. */
export function Welcome({
  me, repos, mood, purring, onPet, onAddRepo, onClone, onNew, canImport, onImport, onImported, onImportTerminal,
}: {
  me: GithubUser; repos: Repo[]; mood: Mood; purring: boolean; onPet: () => void;
  onAddRepo: () => void; onClone: () => void; onNew: () => void;
  canImport: boolean; onImport: () => Promise<void>;
  onImported?: () => void;
  /** Opens "Import terminal sessions" (Claude Code, Codex and Gemini chats run in a terminal). */
  onImportTerminal?: () => void;
}) {
  const [preview, setPreview] = useState<ImportReport | null>(null);
  const [importing, setImporting] = useState(false);
  const [outside, setOutside] = useState<string[]>([]);
  const [moving, setMoving] = useState(false);
  const [moveResult, setMoveResult] = useState<string | null>(null);
  useEffect(() => { api.importedOutside().then(setOutside).catch(() => {}); }, [repos.length]);
  useEffect(() => { if (canImport) api.importPreview().then(setPreview).catch(() => {}); }, [canImport, repos.length]);
  const fresh = repos.length === 0;
  // Setup state from the checklist: it goes first while anything is red, and the greeting waits for GitHub.
  const [setup, setSetup] = useState<{ allOk: boolean; ghOk: boolean } | null>(null);
  const setupRed = !setup || !setup.allOk;
  // The import card shows only for Conductor work not brought over yet, and stays away once dismissed
  // until there is something new.
  const importKey = preview ? `${preview.repos}:${preview.workspaces}:${preview.sessions}` : "";
  const [dismissed, setDismissed] = useState(() => localStorage.getItem("nekotrail.importDismissed") ?? "");
  const hasImport = !!preview && preview.workspaces + preview.repos > 0 && dismissed !== importKey;
  const dismissImport = () => { localStorage.setItem("nekotrail.importDismissed", importKey); setDismissed(importKey); };
  const setupCard = <SetupCheck compact={!fresh} onStatus={setSetup} />;
  return (
    <div className="welcome">
      <Mascot size={88} mood={mood === "working" ? "working" : "happy"} follow purring={purring} onClick={onPet} title="Pet the cat" />
      <h1>{setup?.ghOk && me?.name ? `Hi ${me.name.split(" ")[0]}.` : "Welcome to nekotrail."}</h1>
      <p className="lede">Run many coding agents at once. Each one gets its own branch and folder, and each one ends as a PR.</p>
      {setupRed && setupCard}

      {outside.length > 0 && (
        <div className="import-card hero">
          <h2>Move your imported workspaces into nekotrail</h2>
          <p>
            {outside.length} imported {outside.length === 1 ? "workspace still lives" : "workspaces still live"} in Conductor's folder. If Conductor cleans up or is uninstalled, that folder can be deleted with your uncommitted work.
            Moving them takes a few seconds; branches, chats and uncommitted changes come along.
          </p>
          <button disabled={moving} onClick={async () => {
            setMoving(true);
            try {
              const [n, problems] = await api.relocateImported();
              setMoveResult(problems.length ? `Moved ${n}. Not moved: ${problems.join(" ")}` : `Moved ${n} ${n === 1 ? "workspace" : "workspaces"} into ~/nekotrail/workspaces.`);
              setOutside(await api.importedOutside());
              onImported?.();
            } catch (e) { setMoveResult(String(e)); } finally { setMoving(false); }
          }}>
            {moving ? <span className="spinner" /> : <FolderInput size={15} />}Move {outside.length} {outside.length === 1 ? "workspace" : "workspaces"}
          </button>
        </div>
      )}
      {moveResult && <p className="muted">{moveResult}</p>}
      {hasImport && (
        <div className="import-card hero">
          <div className="row import-head">
            <h2>{fresh ? "Pick up where you left off in Conductor" : "New work in Conductor"}</h2>
            <span className="spacer" />
            <button className="icon" aria-label="Dismiss" title="Hide until there is new work in Conductor" onClick={dismissImport}><X size={14} /></button>
          </div>
          <p>
            Bring over {preview.repos > 0 ? `${preview.repos} ${preview.repos === 1 ? "repo" : "repos"}, ` : ""}
            {preview.workspaces} {preview.workspaces === 1 ? "workspace" : "workspaces"} and {preview.sessions} {preview.sessions === 1 ? "chat" : "chats"} with their history.
            {preview.skipped > 0 ? ` ${preview.skipped} already here or gone from disk.` : ""} nekotrail only reads Conductor's data. Nothing is moved or deleted, and chats continue where they left off.
          </p>
          <button disabled={importing} onClick={async () => { setImporting(true); await onImport(); setImporting(false); setPreview(null); }}>
            {importing ? <span className="spinner" /> : <Download size={15} />}Import from Conductor
          </button>
        </div>
      )}

      {!setupRed && setupCard}

      <div className="start-actions">
        {!fresh && <button className="primary" onClick={onNew}><Plus size={15} />Create workspace <kbd>{keyHint("⌘N")}</kbd></button>}
        <button className={fresh && setup?.allOk ? "primary" : ""} onClick={onAddRepo}><FolderOpen size={15} />Add local repo</button>
        <button onClick={onClone}><GitFork size={15} />Clone repo from GitHub</button>
        {onImportTerminal && <button onClick={onImportTerminal} title="Bring in the Claude Code, Codex and Gemini chats you ran in a terminal. Read-only."><SquareTerminal size={15} />Import terminal sessions</button>}
      </div>
      {!fresh && <p className="muted">{keyHint("⌘K finds any workspace or action. ⌘⌥L jumps to the next one with news.")}</p>}
    </div>
  );
}
