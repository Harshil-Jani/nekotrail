import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowDown, ArrowUp, ChevronDown, CircleCheck, CircleDashed, CircleX, GitBranch, GitMerge, GitPullRequest, Upload } from "lucide-react";
import { api, errText, prFromView, type MergeStep, type Pr, type PrView, type Repo, type Workspace } from "../api";
import { Mascot } from "./Mascot";
import { blocker, mergeAllState, move, shortRef } from "../multiRepo";

/** Saved per task, so the next Merge all starts in the order you picked. */
const orderKey = (group: string) => `nekotrail.mergeOrder.${group}`;
function loadOrder(group: string, ids: string[]): string[] {
  let saved: string[] = [];
  try { saved = JSON.parse(localStorage.getItem(orderKey(group)) || "[]"); } catch { /* none yet */ }
  return [...saved.filter((id) => ids.includes(id)), ...ids.filter((id) => !saved.includes(id))];
}

/** One repo's PR as a pill: state by colour, the number, and why it isn't green in the title. */
function pillOf(pr: Pr | undefined): { cls: string; Icon: typeof CircleCheck; why: string | null } {
  const why = blocker(pr);
  if (!pr) return { cls: "", Icon: GitPullRequest, why };
  if (why === "merged") return { cls: "merged", Icon: GitMerge, why };
  if (!why) return { cls: "good", Icon: CircleCheck, why };
  if (/failing|conflicts|closed|changes requested/.test(why)) return { cls: "bad", Icon: CircleX, why };
  return { cls: "wait", Icon: CircleDashed, why };
}

/**
 * The PR bar of a multi-repo task: every repo's PR in one line, Create PRs (one per repo, each
 * body linking the others) and Merge all (in an order you pick, only when every PR is green,
 * stopping at the first failure). Backend: `group_prs` and `merge_all` (src-tauri/src/multirepo.rs).
 */
export function GroupPrBar({ members, repos, prs, title, refreshKey, onSay, onArchive, onShowRepo, onSendAgent }: {
  /** The task's workspaces, the main one first. */
  members: Workspace[];
  repos: Repo[];
  prs: Record<string, PrView>;
  /** The task's title, for the PR title. */
  title: string;
  refreshKey: number;
  onSay: (text: string, error?: boolean) => void;
  onArchive: () => void;
  /** Open one repo's Changes. */
  onShowRepo: (workspaceId: string) => void;
  onSendAgent: (text: string) => void;
}) {
  const main = members[0];
  const group = main.group_id;
  const nameOf = (w: Workspace) => repos.find((r) => r.id === w.repo_id)?.name ?? "repo";
  const rows = members.map((w) => ({ w, name: nameOf(w), pr: prFromView(prs[w.id]).pr ?? undefined }));
  const [form, setForm] = useState<{ title: string; body: string; draft: boolean } | null>(null);
  const [merging, setMerging] = useState(false);
  const [order, setOrder] = useState<string[]>(() => loadOrder(group, members.map((m) => m.id)));
  const [busy, setBusy] = useState<string | null>(null);
  const [menu, setMenu] = useState(false);
  const [steps, setSteps] = useState<MergeStep[] | null>(null);
  useEffect(() => { setOrder(loadOrder(group, members.map((m) => m.id))); setForm(null); setMerging(false); setSteps(null); }, [group, members.length]);
  useEffect(() => { members.forEach((m) => api.prRefresh(m.id).catch(() => {})); }, [group, refreshKey]);

  const missing = rows.filter((r) => !r.pr || r.pr.state === "CLOSED");
  const merged = rows.filter((r) => r.pr?.state === "MERGED").length;
  const allMerged = merged === rows.length;
  const ordered = order.map((id) => rows.find((r) => r.w.id === id)!).filter(Boolean);
  const state = mergeAllState(ordered);

  const run = async (label: string, fn: () => Promise<void>) => {
    setBusy(label);
    try { await fn(); } catch (e) { onSay(errText(e), true); } finally {
      setBusy(null);
      members.forEach((m) => api.prRefresh(m.id, true).catch(() => {}));
    }
  };
  const create = () => form && run("pr", async () => {
    const made = await api.groupPrs(main.id, form.title.trim(), form.body, form.draft);
    setForm(null);
    onSay(`Opened ${made.map((p) => `${p.repo}#${p.number}`).join(", ")}. Each description links the others.`);
  });
  const mergeAll = () => run("merge", async () => {
    localStorage.setItem(orderKey(group), JSON.stringify(order));
    const done = await api.mergeAll(main.id, order);
    setSteps(done);
    const failed = done.find((s) => s.outcome === "failed");
    if (failed) onSay(`Merge all stopped at ${failed.name}: ${failed.detail.split("\n")[0]}`, true);
    else { setMerging(false); onSay(`Merged all ${done.length} PRs.`); }
  });

  const primary = missing.length > 0
    ? <button className={form ? "" : "primary"} onClick={() => { setMerging(false); setForm(form ? null : { title, body: "", draft: false }); }} disabled={!!busy} aria-expanded={!!form}
        title="Push every branch and open one PR per repo, each linking the others">
        <GitPullRequest size={15} /><span className="pb-label">{missing.length === rows.length ? `Create ${rows.length} PRs` : `Create the other ${missing.length === 1 ? "PR" : `${missing.length} PRs`}`}</span>
      </button>
    : !allMerged && <button className={state.ready && !merging ? "good-btn" : ""} onClick={() => { setForm(null); setMerging((m) => !m); }} disabled={!!busy} aria-expanded={merging}
        title={state.ready ? "Pick the order, then merge every PR" : `Not every PR is green: ${state.waiting.join("; ")}`}>
        <GitMerge size={15} /><span className="pb-label">Merge all</span>
      </button>;

  return (
    <div className="prbar task-prbar">
      <span className="branch task-branch" title={`${main.branch} into ${main.target_branch}, in every repo`}>
        <GitBranch size={14} /><span className="branch-text">{main.branch} → {main.target_branch}</span>
      </span>
      <span className="task-prs" aria-label="Pull requests in each repo">
        {rows.map(({ w, name, pr }) => {
          const { cls, Icon, why } = pillOf(pr);
          return (
            <button key={w.id} className={`pill ${cls} task-pr`} onClick={() => onShowRepo(w.id)}
              title={`${name}: ${pr ? `${shortRef(pr.url, pr.number)}, ${why ?? "green"}` : "no PR yet"}. Click for its changes.`}>
              {pr ? <Icon size={13} /> : <GitPullRequest size={13} />}
              <span>{name}</span>
              {pr && <span className="num">#{pr.number}</span>}
            </button>
          );
        })}
      </span>
      <span className="spacer" />
      {!allMerged && (
        <span className="git-actions">
          {primary}
          <span className="git-menu">
            <button className="icon-btn" aria-label="More git actions" aria-expanded={menu} onClick={() => setMenu((m) => !m)} disabled={!!busy}>
              {busy ? <span className="spinner" /> : <ChevronDown size={15} />}
            </button>
            {menu && (
              <span className="menu" role="menu" onMouseLeave={() => setMenu(false)}>
                <button role="menuitem" onClick={() => { setMenu(false); onSendAgent(commitPrompt(members, nameOf)); }}>
                  <Upload size={15} /><span>Ask the agent to commit in every repo</span>
                </button>
                <button role="menuitem" disabled={missing.length === 0} onClick={() => { setMenu(false); setMerging(false); setForm({ title, body: "", draft: false }); }}>
                  <GitPullRequest size={15} /><span>Create PRs</span>
                </button>
                <button role="menuitem" disabled={missing.length > 0} onClick={() => { setMenu(false); setForm(null); setMerging(true); }}>
                  <GitMerge size={15} /><span>Merge all</span>
                </button>
                {rows.filter((r) => r.pr).map((r) => (
                  <button key={r.w.id} role="menuitem" onClick={() => { setMenu(false); openUrl(r.pr!.url).catch(() => {}); }}>
                    <GitPullRequest size={15} /><span>Open {shortRef(r.pr!.url, r.pr!.number)}</span>
                  </button>
                ))}
              </span>
            )}
          </span>
        </span>
      )}
      {allMerged && (
        <div className="merged-banner" role="status">
          <Mascot size={36} mood="happy" title="Merged" />
          <span><b>Merged in every repo.</b> Archive the task to tidy up all {rows.length} workspaces. Their branches are kept.</span>
          <span className="spacer" />
          <button className="primary" onClick={onArchive}>Archive task</button>
        </div>
      )}
      {form && (
        <div className="pr-form">
          <input placeholder="PR title" aria-label="PR title" value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} autoFocus />
          <textarea placeholder="What changed and why" aria-label="PR description" rows={4} value={form.body} onChange={(e) => setForm({ ...form, body: e.target.value })} />
          <p className="muted task-note">
            Pushes {members[0].branch} in {rows.map((r) => r.name).join(", ")} and opens a PR in each. Every description ends with "Part of a change across" and links to all of them. Open PRs are kept, not opened twice. Commit your work first: only commits are pushed.
          </p>
          <div className="row">
            <label className="check"><input type="checkbox" checked={form.draft} onChange={(e) => setForm({ ...form, draft: e.target.checked })} /> Open as drafts</label>
            <span className="spacer" />
            <button className="quiet" onClick={() => setForm(null)}>Cancel</button>
            <button className="primary" disabled={!!busy || !form.title.trim()} onClick={create}>
              {busy === "pr" ? <span className="spinner" /> : <GitPullRequest size={16} />}
              {busy === "pr" ? "Pushing…" : `Push and create ${missing.length || rows.length} PRs`}
            </button>
          </div>
        </div>
      )}
      {merging && !allMerged && (
        <div className="pr-form merge-order" role="group" aria-label="Merge order">
          <p className="task-note">Merges in this order, and only when every PR is green. If one fails, the rest wait.</p>
          <ol className="merge-list">
            {ordered.map((r, i) => {
              const { cls, Icon, why } = pillOf(r.pr);
              const step = steps?.find((s) => s.workspace_id === r.w.id);
              return (
                <li key={r.w.id} className="merge-row">
                  <span className="merge-n num">{i + 1}</span>
                  <span className="merge-name">{r.name}</span>
                  {r.pr && <button className="link num" onClick={() => openUrl(r.pr!.url).catch(() => {})}>{shortRef(r.pr.url, r.pr.number)}</button>}
                  <span className={`pill ${cls}`}><Icon size={13} />{why ?? "green"}</span>
                  {step && !(cls === "merged" && (step.outcome === "merged" || step.outcome === "already")) && (
                    <span className={`muted merge-step ${step.outcome}`} title={step.detail || undefined}>
                      {step.outcome === "merged" || step.outcome === "already" ? "Merged" : step.outcome === "failed" ? `Failed: ${step.detail.split("\n")[0]}` : "Waiting, not merged"}
                    </span>
                  )}
                  <span className="spacer" />
                  <button className="icon" aria-label={`Move ${r.name} up`} disabled={i === 0 || !!busy} onClick={() => setOrder((o) => move(o, i, -1))}><ArrowUp size={14} /></button>
                  <button className="icon" aria-label={`Move ${r.name} down`} disabled={i === ordered.length - 1 || !!busy} onClick={() => setOrder((o) => move(o, i, 1))}><ArrowDown size={14} /></button>
                </li>
              );
            })}
          </ol>
          <div className="row">
            {!state.ready && <span className="muted task-note">Waiting on {state.waiting.join("; ")}.</span>}
            <span className="spacer" />
            <button className="quiet" onClick={() => setMerging(false)}>Cancel</button>
            <button className="good-btn" disabled={!!busy || !state.ready} onClick={mergeAll}
              title={state.ready ? undefined : `Not every PR is green: ${state.waiting.join("; ")}`}>
              {busy === "merge" ? <span className="spinner" /> : <GitMerge size={15} />}
              {busy === "merge" ? "Merging…" : `Merge ${state.left === rows.length ? "all" : state.left} in this order`}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** For "Ask the agent to commit in every repo": the folders, so it doesn't miss one. */
function commitPrompt(members: Workspace[], nameOf: (w: Workspace) => string): string {
  const list = members.map((w) => `- ${nameOf(w)}: ${w.path}`).join("\n");
  return `Commit the work for this task in every repo it touches, with a clear message in each. Don't push. The repos:\n${list}\n\nRun \`git status\` in each folder first, and say which ones had nothing to commit.`;
}
