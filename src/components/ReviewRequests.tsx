import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, Eye, GitPullRequest, GitPullRequestDraft } from "lucide-react";
import type { Agent, Repo, ReviewRequest, Workspace } from "../api";
import { AGENT_INFO, splitAgents } from "../agents";
import { useInstalledAgents } from "../useInstalledAgents";
import { requestMeta, reviewWorkspaceFor } from "../review";

/**
 * "Review requests" at the top of the sidebar: open PRs where someone asked for your review, from
 * the PR poller. Click one to start a read-only review with your usual agent; the arrow picks
 * another. A PR you are already reviewing opens its workspace.
 */
export function ReviewRequests({ requests, workspaces, repos, busy, onReview, onOpen }: {
  requests: ReviewRequest[];
  workspaces: Workspace[];
  repos: Repo[];
  /** "repo:number" of a review being set up. */
  busy: string | null;
  onReview: (r: ReviewRequest, agent?: Agent) => void;
  onOpen: (workspaceId: string) => void;
}) {
  const [folded, setFolded] = useState(() => localStorage.getItem("nekotrail.reviewsFolded") === "1");
  const [menu, setMenu] = useState<string | null>(null);
  const installed = useInstalledAgents();
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => { if (!(e.target as HTMLElement).closest(".review-with-menu, .review-with")) setMenu(null); };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);
  if (!requests.length) return null;
  const toggle = () => setFolded((f) => { localStorage.setItem("nekotrail.reviewsFolded", f ? "0" : "1"); return !f; });
  // Installed agents first, then the rest under "Not installed", so the list answers "which can I use?".
  const all = splitAgents(installed);
  const every = [...all.shown, ...all.installed, ...all.missing];
  const has = new Set(installed ?? []);
  const split = { installed: every.filter((a) => has.has(a)), missing: every.filter((a) => !has.has(a)) };
  const repoName = (id: string) => repos.find((r) => r.id === id)?.name;
  return (
    <section className="repo review-requests" aria-label="Review requests">
      <div className="repo-head section-head">
        <button className="fold" tabIndex={-1} onClick={toggle} aria-expanded={!folded} aria-label={folded ? "Expand review requests" : "Collapse review requests"}>
          {folded ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
        </button>
        <Eye size={12} className="muted" />
        <span className="repo-name" onClick={toggle}>Review requests</span>
        <span className="repo-meta num">{requests.length}</span>
      </div>
      {!folded && requests.map((r) => {
        const key = `${r.repo_id}:${r.number}`;
        const open = reviewWorkspaceFor(r, workspaces);
        const meta = requestMeta(r, repos.length > 1 ? repoName(r.repo_id) : undefined);
        const item = (a: Agent) => (
          <button key={a} role="menuitem" onClick={() => { setMenu(null); onReview(r, a); }}><span className="menu-icon-space" aria-hidden />{AGENT_INFO[a].name}</button>
        );
        return (
          <div key={key} className="review-req">
            <button
              role="option"
              aria-selected={false}
              tabIndex={-1}
              className="ws two"
              disabled={busy === key}
              title={`${r.title}\n${meta}${open ? "\nYou're reviewing this. Click to open it." : "\nClick to review it with an agent, read-only. Nothing is posted until you submit."}`}
              onClick={() => (open ? onOpen(open.id) : onReview(r))}
            >
              <span className="ws-mark">
                {busy === key ? <span className="spinner" aria-label="Starting the review" />
                  : r.isDraft ? <GitPullRequestDraft size={15} className="pr-draft" aria-label="Draft PR" />
                  : <GitPullRequest size={15} className="pr-open" aria-label="Open PR" />}
              </span>
              <span className="ws-text">
                <span className="ws-name">{r.title}</span>
                <span className="ws-sub-line"><span className="ws-star">{open ? `Reviewing · ${meta}` : meta}</span></span>
              </span>
            </button>
            {!open && (
              <button className="icon review-with" tabIndex={-1} aria-label={`Review #${r.number} with another agent`} title="Review with another agent" aria-expanded={menu === key}
                onClick={() => setMenu((m) => (m === key ? null : key))}>
                <ChevronDown size={13} />
              </button>
            )}
            {menu === key && (
              <span className="menu review-with-menu" role="menu">
                <div className="menu-label">Review with</div>
                {[...split.installed, ...(installed === null ? split.missing : [])].map(item)}
                {installed !== null && split.missing.length > 0 && <div className="menu-label">Not installed</div>}
                {installed !== null && split.missing.map(item)}
              </span>
            )}
          </div>
        );
      })}
    </section>
  );
}
