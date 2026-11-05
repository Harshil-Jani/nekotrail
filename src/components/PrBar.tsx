import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleCheck, CircleDashed, CircleX, GitBranch, GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, MessageSquare, Sparkles } from "lucide-react";
import { api, avatar, errText, type Pr, type Workspace } from "../api";
import { Mascot } from "./Mascot";
import { Avatar } from "./Avatar";

type Checks = { cls: "good" | "bad" | "wait"; text: string; names: string[] };

function checkSummary(pr: NonNullable<Pr>): Checks | null {
  const checks = pr.statusCheckRollup ?? [];
  if (checks.length === 0) return null;
  const result = (c: (typeof checks)[number]) => (c.conclusion || c.state || c.status || "").toUpperCase();
  const failed = checks.filter((c) => ["FAILURE", "ERROR", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE"].includes(result(c)));
  const pending = checks.filter((c) => ["", "PENDING", "QUEUED", "IN_PROGRESS", "EXPECTED", "WAITING"].includes(result(c)));
  const passed = checks.length - failed.length - pending.length;
  if (failed.length) return { cls: "bad", text: `${failed.length} failing`, names: failed.map((c) => c.name || c.context || "check") };
  if (pending.length) return { cls: "wait", text: `${pending.length} running`, names: [] };
  return { cls: "good", text: `${passed} passed`, names: [] };
}

function PrState({ pr }: { pr: NonNullable<Pr> }) {
  if (pr.state === "MERGED") return <span className="pill merged"><GitMerge size={14} />Merged</span>;
  if (pr.state === "CLOSED") return <span className="pill bad"><GitPullRequestClosed size={14} />Closed</span>;
  if (pr.isDraft) return <span className="pill"><GitPullRequestDraft size={14} />Draft</span>;
  return <span className="pill good"><GitPullRequest size={14} />Open</span>;
}

export function PrBar({
  ws,
  name,
  refreshKey,
  onAskAgent,
  onArchive,
}: {
  ws: Workspace;
  name: string;
  refreshKey: number;
  onAskAgent: (prompt: string) => void;
  onArchive: () => void;
}) {
  const [pr, setPr] = useState<Pr | undefined>(undefined);
  const [form, setForm] = useState<{ title: string; body: string; draft: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () =>
    api.prStatus(ws.id).then((p) => { setPr(p); setError(null); }).catch((e) => { setPr(null); setError(errText(e)); });

  useEffect(() => {
    setPr(undefined);
    setError(null);
    setForm(null);
    load();
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  }, [ws.id, refreshKey]);

  const create = async () => {
    if (!form) return;
    setBusy(true);
    setError(null);
    try {
      await api.createPr(ws.id, form.title, form.body, form.draft);
      setForm(null);
      await load();
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  const checks = pr ? checkSummary(pr) : null;
  const CheckIcon = checks?.cls === "good" ? CircleCheck : checks?.cls === "bad" ? CircleX : CircleDashed;
  const reviewers = pr
    ? [
        ...pr.latestReviews.map((r) => ({ login: r.author.login, state: r.state })),
        ...pr.reviewRequests.filter((r) => r.login).map((r) => ({ login: r.login!, state: "PENDING" })),
      ]
    : [];

  return (
    <div className="prbar">
      <span className="branch" title={`${ws.branch} into ${ws.target_branch}`}>
        <GitBranch size={14} />{ws.branch} → {ws.target_branch}
      </span>
      <span className="spacer" />
      {pr === undefined && <span className="muted">Looking for a PR…</span>}
      {pr && (
        <>
          <Avatar src={avatar(pr.author.login)} size={24} label={pr.author.login} title={`Opened by @${pr.author.login}`} />
          <PrState pr={pr} />
          <button className="link num" onClick={() => openUrl(pr.url)} title={pr.title}>#{pr.number}</button>
          <span className="num"><span className="add">+{pr.additions}</span> <span className="del">-{pr.deletions}</span></span>
          {pr.comments.length > 0 && <span className="check num"><MessageSquare size={14} />{pr.comments.length}</span>}
          {reviewers.length > 0 && (
            <span className="reviewers" aria-label="Reviewers">
              {reviewers.map((r) => (
                <span key={r.login} className={`rv rv-${r.state}`} title={`@${r.login}: ${r.state.toLowerCase().replace("_", " ")}`}>
                  <Avatar src={avatar(r.login)} size={24} label={r.login} />
                  <i />
                </span>
              ))}
            </span>
          )}
          {checks && <span className={`pill ${checks.cls}`}><CheckIcon size={14} />CI {checks.text}</span>}
          {pr.state === "OPEN" && checks?.cls === "bad" && <Mascot size={28} mood="worried" title="CI is failing" />}
          {pr.state === "OPEN" && checks?.cls === "bad" && (
            <button onClick={() => onAskAgent(`CI is failing on PR #${pr.number}: ${checks.names.join(", ")}. Run \`gh pr checks ${pr.number}\`, read the failing logs, and fix them.`)}>
              <Sparkles size={14} />Fix CI
            </button>
          )}
          {pr.state === "OPEN" && pr.mergeable === "CONFLICTING" && (
            <button onClick={() => onAskAgent(`PR #${pr.number} has merge conflicts with ${ws.target_branch}. Fetch origin, merge ${ws.target_branch} into this branch, resolve the conflicts, and commit.`)}>
              <Sparkles size={14} />Resolve conflicts
            </button>
          )}
        </>
      )}
      {pr?.state === "MERGED" && (
        <div className="merged-banner" role="status">
          <Mascot size={36} mood="happy" title="Merged" />
          <span><b>Merged.</b> {name}'s work is in {ws.target_branch}. Archive it to tidy up; the branch stays.</span>
          <span className="spacer" />
          <button className="primary" onClick={onArchive}>Archive {name}</button>
        </div>
      )}
      {pr === null && !form && (
        <>
          <button className="quiet" onClick={() => onAskAgent("Commit all the work in this workspace with a clear message. Then reply with a PR title on the first line and a short PR description below it.")}>
            <Sparkles size={14} />Commit with agent
          </button>
          <button className="primary" onClick={() => setForm({ title: "", body: "", draft: false })}>
            <GitPullRequest size={16} />Create PR
          </button>
        </>
      )}
      {form && (
        <div className="pr-form">
          <input placeholder="Title" aria-label="PR title" value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} autoFocus />
          <textarea placeholder="What changed and why" aria-label="PR description" rows={4} value={form.body} onChange={(e) => setForm({ ...form, body: e.target.value })} />
          <div className="row">
            <label className="check"><input type="checkbox" checked={form.draft} onChange={(e) => setForm({ ...form, draft: e.target.checked })} /> Open as draft</label>
            <span className="spacer" />
            <button className="quiet" onClick={() => setForm(null)}>Cancel</button>
            <button className="primary" disabled={busy || !form.title.trim()} onClick={create}>
              {busy ? <span className="spinner" /> : <GitPullRequest size={16} />}
              {busy ? "Pushing…" : "Push and open PR"}
            </button>
          </div>
        </div>
      )}
      {error && <span className="pill bad" title={error}><CircleX size={14} />GitHub: {error.split("\n")[0].slice(0, 60)}</span>}
    </div>
  );
}
