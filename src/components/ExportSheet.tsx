import { useEffect, useMemo, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { homeDir } from "@tauri-apps/api/path";
import { Check, Copy, FileCode2, FileText, GitPullRequest, Share, ShieldCheck, X } from "lucide-react";
import { api, errText, type Pr, type Session, type Workspace } from "../api";
import { agentName } from "../agents";
import { getSettings } from "../settings";
import { hint } from "../redact";
import { buildReport, exportFileName, findSecrets, pickHunkFiles, prSummary, redactReport, toHtml, toMarkdown, type ReportOptions, type ReportSource } from "../sessionReport";
import { Markdown } from "./Markdown";

type Loaded = Omit<ReportSource, "chats" | "exportedAt"> & { chats: (ReportSource["chats"][number] & { id: string })[] };

/**
 * "Export session": a report of what the agent did in a workspace or one chat, previewed with what
 * will be hidden. Copy or save it as Markdown, save a single HTML file, or put a short summary in the
 * PR description draft. Everything is built here, on this machine; nothing is uploaded.
 */
export function ExportSheet({ ws, repoName, star, initialChat, pr, onClose, onSay }: {
  ws: Workspace;
  repoName: string;
  star: string;
  /** Opened from a chat's menu: start with only that chat. */
  initialChat?: string;
  pr: Pr | undefined;
  onClose: () => void;
  onSay: (text: string, error?: boolean) => void;
}) {
  const [data, setData] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scope, setScope] = useState<string>(initialChat ?? "all");
  const [opts, setOpts] = useState<Omit<ReportOptions, "footer">>({ toolOutputs: false, fileContents: true });
  const [keep, setKeep] = useState<Set<string>>(new Set());
  const [view, setView] = useState<"preview" | "markdown">("preview");
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [footer] = useState(() => getSettings().exportFooter);

  useEffect(() => {
    let live = true;
    (async () => {
      const [sessions, changes, checks, home] = await Promise.all([
        api.sessions(ws.id),
        api.changes(ws.id).catch(() => []),
        api.checksList(ws.id).then((c) => c.results).catch(() => []),
        homeDir().catch(() => null),
      ]);
      const chats = await Promise.all(sessions.map(async (s: Session) => ({ id: s.id, title: s.title, agent: s.agent, model: s.model, events: await api.sessionEvents(s.id) })));
      const diffs: Record<string, string> = {};
      await Promise.all(pickHunkFiles(changes).map(async (p) => { diffs[p] = await api.fileDiff(ws.id, p).catch(() => ""); }));
      if (!live) return;
      setData({
        title: ws.title, repo: repoName, branch: ws.branch, target: ws.target_branch, star, chats, changes, diffs, home,
        checks: checks.map((c) => ({ name: c.name, command: c.command, status: c.status, passed: c.passed, failed: c.failed, regressed: c.regressed })),
        pr: pr ? { number: pr.number, url: pr.url, state: pr.state, title: pr.title, isDraft: pr.isDraft } : null,
      });
    })().catch((e) => live && setError(errText(e)));
    return () => { live = false; };
  }, [ws.id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  useEffect(() => { if (!copied) return; const t = setTimeout(() => setCopied(false), 1500); return () => clearTimeout(t); }, [copied]);

  const chatsWithEvents = data?.chats.filter((c) => c.events.length) ?? [];
  const built = useMemo(() => {
    if (!data) return null;
    const chats = scope === "all" ? data.chats : data.chats.filter((c) => c.id === scope);
    const report = buildReport({ ...data, chats, exportedAt: new Date().toISOString() }, { ...opts, footer });
    const findings = findSecrets(report);
    const clean = redactReport(report, findings, keep);
    return { report: clean, findings, md: toMarkdown(clean) };
  }, [data, scope, opts, keep, footer]);
  const hidden = built ? built.findings.filter((f) => !keep.has(f.value)).length : 0;

  const saveAs = async (kind: "md" | "html") => {
    if (!built) return;
    setBusy(kind);
    try {
      const path = await saveDialog({
        defaultPath: exportFileName(built.report, kind),
        filters: kind === "md" ? [{ name: "Markdown", extensions: ["md"] }] : [{ name: "Web page", extensions: ["html"] }],
      });
      if (!path) return;
      // The HTML renderer is only loaded when someone saves one.
      const text = kind === "md" ? built.md : toHtml(built.report, (await import("../reportHtml")).mdToHtml);
      await api.exportSave(path, text);
      onSay(`Saved ${path.replace(/^.*\//, "")}.`);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(null);
    }
  };

  const toPr = async () => {
    if (!built) return;
    const body = prSummary(built.report);
    if (pr && pr.state !== "CLOSED") {
      // An open PR's description lives on GitHub: nekotrail never edits it for you.
      await navigator.clipboard.writeText(body).catch(() => {});
      onSay(`PR #${pr.number} is already open, so the summary is copied. Paste it into its description on GitHub.`);
      onClose();
      return;
    }
    window.dispatchEvent(new CustomEvent("nekotrail-pr-draft", { detail: { workspaceId: ws.id, body } }));
    onSay("The summary is in the PR description. Check it, then press Push and create PR.");
    onClose();
  };
  const prOff = ws.review_pr > 0 ? "This is a review of someone else's PR." : ws.imported_from === "terminal-history" ? "History has no PR." : null;

  const toggle = (k: keyof typeof opts, label: string, note: string) => (
    <label className="export-toggle">
      <input type="checkbox" className="switch" checked={opts[k]} onChange={(e) => setOpts({ ...opts, [k]: e.target.checked })} />
      <span><b>{label}</b><small className="muted">{note}</small></span>
    </label>
  );

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal export" role="dialog" aria-modal="true" aria-labelledby="export-title" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          <Share size={16} />
          <h2 id="export-title">Export session</h2>
          <span className="muted export-where">{ws.title || star}</span>
          <span className="spacer" />
          <button className="icon" aria-label="Close" onClick={onClose}><X size={15} /></button>
        </div>
        {error && <div className="mcp-note bad" role="alert">{error}</div>}
        {!data && !error && <p className="muted export-loading"><span className="spinner" /> Reading the chats and changes…</p>}
        {data && built && (
          <div className="export-body">
            <aside className="export-side" aria-label="What goes in">
              {chatsWithEvents.length > 1 && (
                <label className="export-scope">
                  <span className="mcp-h">Chats</span>
                  <select value={scope} onChange={(e) => setScope(e.target.value)} aria-label="Chats in the export">
                    <option value="all">All {chatsWithEvents.length} chats</option>
                    {chatsWithEvents.map((c) => <option key={c.id} value={c.id}>{c.title} ({agentName(c.agent)})</option>)}
                  </select>
                </label>
              )}
              <h3 className="mcp-h">Include</h3>
              <div className="export-toggles">
                {toggle("toolOutputs", "Tool outputs", "What commands printed. Off keeps one line per command.")}
                {toggle("fileContents", "File contents", "Diff hunks, and files the agent read when outputs are on.")}
              </div>
              <h3 className="mcp-h">Hidden <span className="export-count">{hidden}</span></h3>
              {built.findings.length === 0 ? (
                <p className="muted export-small"><ShieldCheck size={13} className="ok" /> Nothing here looks like a token, key, password, .env value or email.</p>
              ) : (
                <ul className="export-secrets" aria-label="Things that look like secrets">
                  {built.findings.map((f) => {
                    const shown = keep.has(f.value);
                    return (
                      <li key={f.value}>
                        <span className="export-secret-text">
                          <span><b>{f.kind}</b> <small className="muted">· {f.count === 1 ? "1 place" : `${f.count} places`}</small></span>
                          <code className="muted">{hint(f.value)}</code>
                        </span>
                        <label className="export-show">
                          <input type="checkbox" checked={shown} onChange={() => { const n = new Set(keep); if (shown) n.delete(f.value); else n.add(f.value); setKeep(n); }} />
                          Show
                        </label>
                      </li>
                    );
                  })}
                </ul>
              )}
              <p className="muted export-small">The scan knows common formats, not everything: read the preview before you share. Paths in your home folder show as ~.</p>
            </aside>
            <section className="export-preview" aria-label="Preview">
              <div className="seg" role="tablist" aria-label="Preview as">
                <button role="tab" aria-selected={view === "preview"} className={view === "preview" ? "on" : ""} onClick={() => setView("preview")}>Preview</button>
                <button role="tab" aria-selected={view === "markdown"} className={view === "markdown" ? "on" : ""} onClick={() => setView("markdown")}>Markdown</button>
              </div>
              <div className="export-doc" tabIndex={0} aria-label={view === "preview" ? "Report preview" : "Markdown source"}>
                {view === "preview" ? <div className="msg assistant export-md"><Markdown text={built.md} /></div> : <pre>{built.md}</pre>}
              </div>
            </section>
          </div>
        )}
        <div className="row export-foot">
          <button disabled={!built || !!prOff} onClick={toPr} title={prOff ?? (pr && pr.state !== "CLOSED" ? `PR #${pr.number} is open: the summary is copied for you to paste` : "A short summary for the PR you're about to open. Nothing is posted.")}>
            <GitPullRequest size={14} />Add to PR description
          </button>
          <span className="spacer" />
          <button disabled={!built} onClick={() => { navigator.clipboard.writeText(built!.md).then(() => setCopied(true)).catch((e) => setError(errText(e))); }}>
            {copied ? <Check size={14} /> : <Copy size={14} />}{copied ? "Copied" : "Copy Markdown"}
          </button>
          <button disabled={!built || !!busy} onClick={() => saveAs("md")}>{busy === "md" ? <span className="spinner" /> : <FileText size={14} />}Save Markdown…</button>
          <button className="primary" disabled={!built || !!busy} onClick={() => saveAs("html")}>{busy === "html" ? <span className="spinner" /> : <FileCode2 size={14} />}Save HTML…</button>
        </div>
      </div>
    </div>
  );
}
