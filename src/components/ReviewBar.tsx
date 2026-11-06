import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Eye, FilePlus2, Lock } from "lucide-react";
import { api, type ReviewPlan, type ReviewRequest, type Workspace } from "../api";

/**
 * The bar over a review workspace, in place of the PR bar: whose PR this is, that nothing is pushed
 * from here, and the way to your drafts. Pushing, merging and autopilot don't exist here.
 */
export function ReviewBar({ ws, request, refreshKey, onChanges }: {
  ws: Workspace;
  /** The PR as the poller last saw it, while it still waits for your review. */
  request?: ReviewRequest;
  refreshKey: number;
  onChanges: () => void;
}) {
  const [plan, setPlan] = useState<ReviewPlan | null>(null);
  useEffect(() => { api.reviewPlan(ws.id).then(setPlan).catch(() => setPlan(null)); }, [ws.id, refreshKey]);
  const url = plan?.url || request?.url;
  const author = plan?.author || request?.author.login;
  // The author pushed after the review started: comments still go on the commit the agent read.
  const moved = !!(request && plan && request.headRefOid && plan.head_sha && request.headRefOid !== plan.head_sha);
  return (
    <div className="prbar review-bar-top">
      <Eye size={15} className="muted" />
      <b>Reviewing #{ws.review_pr}</b>
      {author && <span className="muted">by @{author}</span>}
      <span className="chip read-only" title="Someone else's PR: the agent only reads, and nothing is pushed from this workspace."><Lock size={11} />Read only</span>
      {moved && <span className="chip" title={`Your comments go on ${plan!.head_sha.slice(0, 7)}, the commit the agent read. GitHub marks them as on an older version.`}>New commits since you started</span>}
      <span className="spacer" />
      {url && <button className="quiet" onClick={() => openUrl(url).catch(() => {})}><ExternalLink size={14} />Open on GitHub</button>}
      <button onClick={onChanges} title="Edit the drafts, then Submit review"><FilePlus2 size={14} />Drafts in Changes</button>
    </div>
  );
}
