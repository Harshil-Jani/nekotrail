import { useEffect, useMemo, useState } from "react";
import { Check, CircleAlert, Copy, Download, FolderOpen, GitFork, Lock, Plus, RotateCw, Search, X } from "lucide-react";
import { api, errText, type Doctor, type GithubUser, type ImportReport, type Repo } from "../api";
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
export function SetupCheck({ compact = false }: { compact?: boolean }) {
  const [d, setD] = useState<Doctor | null>(null);
  const [busy, setBusy] = useState(false);
  const run = () => { setBusy(true); api.doctor().then(setD).catch(() => {}).finally(() => setBusy(false)); };
  useEffect(run, []);
  if (!d) return <p className="muted"><span className="spinner" /> Checking your setup…</p>;
  const agentOk = d.claude.ok || d.codex.ok;
  const allOk = d.git.ok && d.gh.ok && agentOk;
  if (allOk && compact) return null;
  return (
    <section className="setup-check">
      <div className="row">
        <h2>{allOk ? "All set" : "Finish setup (about 2 minutes)"}</h2>
        <span className="spacer" />
        <button className="quiet" onClick={run} disabled={busy}>{busy ? <span className="spinner" /> : <RotateCw size={13} />}Check again</button>
      </div>
      <ul>
        <CheckRow ok={d.git.ok} title="git" detail={d.git.version.replace("git version ", "")} fix={<Cmd text="xcode-select --install" />} />
        <CheckRow
          ok={d.gh.ok}
          title="GitHub CLI, signed in"
          detail={d.gh.login ? `@${d.gh.login}` : d.gh.installed ? "installed, not signed in" : "not installed"}
          fix={<>{!d.gh.installed && <Cmd text="brew install gh" />}<Cmd text="gh auth login" /><span className="muted">For PRs, CI status and avatars.</span></>}
        />
        <CheckRow
          ok={d.claude.ok}
          warn={d.codex.ok}
          title="Claude Code"
          detail={d.claude.ok ? d.claude.version : d.codex.ok ? "optional, Codex is set up" : "not found"}
          fix={<><Cmd text="npm install -g @anthropic-ai/claude-code" /><span className="muted">Then run <code>claude</code> once to sign in.</span></>}
        />
        <CheckRow
          ok={d.codex.ok}
          warn={d.claude.ok}
          title="Codex"
          detail={d.codex.ok ? d.codex.version : d.claude.ok ? "optional, Claude Code is set up" : "not found"}
          fix={<><Cmd text="npm install -g @openai/codex" /><span className="muted">Then run <code>codex</code> once to sign in.</span></>}
        />
      </ul>
      {!agentOk && <p className="muted">You need at least one of Claude Code or Codex. nekotrail uses your own login and subscription.</p>}
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
  me, repos, mood, purring, onPet, onAddRepo, onClone, onNew, canImport, onImport,
}: {
  me: GithubUser; repos: Repo[]; mood: Mood; purring: boolean; onPet: () => void;
  onAddRepo: () => void; onClone: () => void; onNew: () => void;
  canImport: boolean; onImport: () => Promise<void>;
}) {
  const [preview, setPreview] = useState<ImportReport | null>(null);
  const [importing, setImporting] = useState(false);
  useEffect(() => { if (canImport) api.importPreview().then(setPreview).catch(() => {}); }, [canImport, repos.length]);
  const fresh = repos.length === 0;
  const hasImport = !!preview && preview.workspaces + preview.repos > 0;
  return (
    <div className="welcome">
      <Mascot size={88} mood={mood === "working" ? "working" : "happy"} follow purring={purring} onClick={onPet} title="Pet the cat" />
      <h1>{me?.name ? `Hi ${me.name.split(" ")[0]}.` : "Hi there."}</h1>
      <p className="lede">Run many coding agents at once. Each one gets its own branch and folder, and each one ends as a PR.</p>

      {hasImport && (
        <div className="import-card hero">
          <h2>{fresh ? "Pick up where you left off in Conductor" : "New work in Conductor"}</h2>
          <p>
            Bring over {preview.repos > 0 ? `${preview.repos} ${preview.repos === 1 ? "repo" : "repos"}, ` : ""}
            {preview.workspaces} {preview.workspaces === 1 ? "workspace" : "workspaces"} and {preview.sessions} {preview.sessions === 1 ? "chat" : "chats"} with their history.
            {preview.skipped > 0 ? ` ${preview.skipped} already here or gone from disk.` : ""} nekotrail only reads Conductor's data. Nothing is moved or deleted, and chats continue where they left off.
          </p>
          <button className="accent" disabled={importing} onClick={async () => { setImporting(true); await onImport(); setImporting(false); setPreview(null); }}>
            {importing ? <span className="spinner" /> : <Download size={15} />}Import from Conductor
          </button>
        </div>
      )}

      <SetupCheck compact={!fresh} />

      <div className="start-actions">
        {!fresh && <button className="primary" onClick={onNew}><Plus size={15} />Create workspace <kbd>⌘N</kbd></button>}
        <button className={fresh ? "primary" : ""} onClick={onAddRepo}><FolderOpen size={15} />Add local repo</button>
        <button onClick={onClone}><GitFork size={15} />Clone repo from GitHub</button>
      </div>
      {!fresh && <p className="muted">⌘K finds any workspace or action. ⌘⌥L jumps to the next one with news.</p>}
    </div>
  );
}
