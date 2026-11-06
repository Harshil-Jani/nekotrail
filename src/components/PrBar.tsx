import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Bot, TriangleAlert, ArrowDownToLine, ChevronDown, CircleCheck, CircleDashed, CircleX, ExternalLink, GitBranch, GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, MessageSquare, ScanSearch, Sparkles, Upload } from "lucide-react";
import { api, avatar, errText, type Pr, type Repo, type Workspace } from "../api";
import { Mascot } from "./Mascot";
import { Avatar } from "./Avatar";
import { getSettings } from "../settings";

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

/** Prompts for the git actions the agent does for you. The repo's own instructions are added. */
export const agentPrompts = {
  createPr: (ws: Workspace, repo?: Repo) =>
    `Open a pull request for this workspace:
1. Commit any uncommitted work with a clear message.
2. Push the branch \`${ws.branch}\` to origin.
3. Run \`gh pr create --base ${ws.target_branch}\` with a short title and a description of what changed, why, and how you tested it.
Reply with the PR link.${repo?.prefs?.pr_prompt ? `\n\n${repo.prefs.pr_prompt}` : ""}`,
  review: (ws: Workspace, repo?: Repo) =>
    `Review the changes on this branch against origin/${ws.target_branch}: run \`git fetch origin ${ws.target_branch}\` and \`git diff origin/${ws.target_branch}...HEAD\`, and check uncommitted files too. Do not change any code.
List real problems first (bugs, security, missing tests), each with file:line and a suggested fix. Then smaller nits. End with whether it is ready to merge.${repo?.prefs?.review_prompt ? `\n\n${repo.prefs.review_prompt}` : ""}`,
  secondOpinion: (ws: Workspace, repo?: Repo) =>
    `Another agent wrote the changes on this branch. Review them against origin/${ws.target_branch}: run \`git fetch origin ${ws.target_branch}\` and \`git diff origin/${ws.target_branch}...HEAD\`, and look at uncommitted files. Do not change any code.
Report each real problem on its own line in exactly this form, most important first:
- path/to/file.ext:LINE: the problem, and the fix
Then one line saying whether it is ready to merge.${repo?.prefs?.review_prompt ? `\n\n${repo.prefs.review_prompt}` : ""}`,
  fixErrors: (prNumber: number, logs: string) =>
    `CI is failing on PR #${prNumber}. Failing logs:\n\n\`\`\`\n${logs.slice(-12000)}\n\`\`\`\n\nFind the cause, fix it, run the relevant tests locally, then commit and push.`,
  conflicts: (ws: Workspace) =>
    `Merging origin/${ws.target_branch} into this branch left conflicts (see \`git status\`). Resolve each one keeping the intent of both sides, run the tests, and commit the merge. Then push.`,
};

type Action = { id: string; label: string; keys: string; icon: React.ReactNode; run: () => void; disabled?: boolean };

const mergeLabel = (repo?: Repo) =>
  repo?.prefs?.merge_method === "merge" ? "Merge" : repo?.prefs?.merge_method === "rebase" ? "Rebase and merge" : "Squash and merge";

export function PrBar({
  ws,
  repo,
  name,
  refreshKey,
  overlapKey,
  onSendAgent,
  onArchive,
  onSay,
  autopilot,
  nameOf,
}: {
  ws: Workspace;
  repo?: Repo;
  name: string;
  refreshKey: number;
  /** Bumps when any workspace's agent finishes: overlapping files can change from either side. */
  overlapKey: number;
  /** Send a prompt to the agent right away. */
  onSendAgent: (prompt: string) => void;
  /** `auto` is true when archiving by itself after a merge; `id` is set once it is no longer the open workspace. */
  onArchive: (auto?: boolean, id?: string) => void;
  onSay: (text: string, error?: boolean) => void;
  /** Agent turns autopilot has used on this PR; undefined when it's off. */
  autopilot?: number;
  nameOf: (workspaceId: string) => string;
}) {
  const [pr, setPr] = useState<Pr | undefined>(undefined);
  const [form, setForm] = useState<{ title: string; body: string; draft: boolean } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [overlap, setOverlap] = useState<{ workspace_id: string; files: string[] }[]>([]);
  useEffect(() => { setOverlap([]); api.overlaps(ws.id).then(setOverlap).catch(() => {}); }, [ws.id, overlapKey]);

  const load = () =>
    api.prStatus(ws.id).then((p) => { setPr(p); setError(null); }).catch((e) => { setPr(null); setError(errText(e)); });

  useEffect(() => {
    setPr(undefined);
    setError(null);
    setForm(null);
    setMenu(false);
    load();
    // Poll GitHub only while you can see the window.
    const t = setInterval(() => { if (!document.hidden) load(); }, 60_000);
    return () => clearInterval(t);
  }, [ws.id, refreshKey]);

  // Files changed against the target branch: Create PR needs at least one.
  const [fileCount, setFileCount] = useState<number | null>(null);
  useEffect(() => { setFileCount(null); api.changes(ws.id).then((c) => setFileCount(c.length)).catch(() => {}); }, [ws.id, refreshKey]);

  // Optional: tidy up by itself once the PR merges. Only when this session watched it go from open to
  // merged (not for a PR that was already merged when you opened it), and only once you have moved on
  // to another workspace, so the merged banner never vanishes under you.
  const sawOpen = useRef<Set<string>>(new Set());
  const pendingArchive = useRef<string | null>(null);
  const archiveRef = useRef(onArchive);
  archiveRef.current = onArchive;
  useEffect(() => {
    if (pr?.state === "OPEN") sawOpen.current.add(ws.id);
    if (pr?.state === "MERGED" && sawOpen.current.has(ws.id) && getSettings().archiveOnMerge) {
      sawOpen.current.delete(ws.id);
      pendingArchive.current = ws.id;
    }
  }, [pr?.state, ws.id]);
  useEffect(() => {
    const id = pendingArchive.current;
    if (id && id !== ws.id) { pendingArchive.current = null; archiveRef.current(true, id); }
    // Leaving for the board, the sky or the welcome unmounts the bar: archive then too.
    return () => { if (pendingArchive.current === ws.id) { pendingArchive.current = null; archiveRef.current(true, ws.id); } };
  }, [ws.id]);

  const task = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(label);
    setError(null);
    try {
      await fn();
    } catch (e) {
      onSay(errText(e), true);
    } finally {
      setBusy(null);
      load();
    }
  };

  const create = async () => {
    if (!form) return;
    await task("pr", async () => {
      await api.createPr(ws.id, form.title, form.body, form.draft);
      setForm(null);
    });
  };

  const checks = pr ? checkSummary(pr) : null;
  const open = pr?.state === "OPEN";
  const conflicting = open && pr?.mergeable === "CONFLICTING";
  const CheckIcon = checks?.cls === "good" ? CircleCheck : checks?.cls === "bad" ? CircleX : CircleDashed;
  const reviewers = pr
    ? [
        ...pr.latestReviews.map((r) => ({ login: r.author.login, state: r.state })),
        ...pr.reviewRequests.filter((r) => r.login).map((r) => ({ login: r.login!, state: "PENDING" })),
      ]
    : [];

  const actions: Record<string, Action> = {
    createPr: { id: "createPr", label: "Create PR", keys: "⌘⇧P", icon: <GitPullRequest size={15} />, run: () => onSendAgent(agentPrompts.createPr(ws, repo)), disabled: (!!pr && pr.state !== "CLOSED") || fileCount === 0 },
    manualPr: { id: "manualPr", label: "Write the PR myself", keys: "", icon: <GitPullRequest size={15} />, run: () => setForm({ title: "", body: "", draft: false }), disabled: (!!pr && pr.state !== "CLOSED") || fileCount === 0 },
    commitPush: { id: "commitPush", label: "Commit and push", keys: "⌘⇧Y", icon: <Upload size={15} />, run: () => task("push", async () => onSay(`Pushed: ${await api.commitPush(ws.id)}`)) },
    pull: {
      id: "pull", label: `Pull latest ${ws.target_branch}`, keys: "⌘⇧L", icon: <ArrowDownToLine size={15} />,
      run: () => task("pull", async () => {
        const r = await api.pullTarget(ws.id);
        if (r === "conflicts") { onSay(`Merging ${ws.target_branch} hit conflicts. The agent is resolving them.`); onSendAgent(agentPrompts.conflicts(ws)); }
        else onSay(r === "merged" ? `Merged the latest ${ws.target_branch}.` : `Already up to date with ${ws.target_branch}.`);
      }),
    },
    review: { id: "review", label: "Review changes", keys: "⌘⇧R", icon: <ScanSearch size={15} />, run: () => onSendAgent(agentPrompts.review(ws, repo)) },
    second: {
      id: "second", label: "Get a second opinion from the other agent", keys: "", icon: <ScanSearch size={15} />,
      run: () => task("second", async () => {
        const last = (await api.sessions(ws.id)).pop();
        const other = last?.agent === "codex" ? "claude" : "codex";
        await api.startChat(ws.id, other, "plan", `Review by ${other === "codex" ? "Codex" : "Claude"}`, agentPrompts.secondOpinion(ws, repo));
        onSay(`${other === "codex" ? "Codex" : "Claude"} is reviewing in a new read-only chat. Add its findings to your review from there.`);
      }),
    },
    fix: {
      id: "fix", label: "Fix failing checks", keys: "⌘⇧X", icon: <Sparkles size={15} />, disabled: !(open && checks?.cls === "bad"),
      run: () => task("fix", async () => onSendAgent(agentPrompts.fixErrors(pr!.number, await api.failingLogs(ws.id)))),
    },
    conflicts: { id: "conflicts", label: "Resolve conflicts", keys: "", icon: <Sparkles size={15} />, disabled: !conflicting, run: () => actions.pull.run() },
    merge: {
      id: "merge", label: mergeLabel(repo), keys: "⌘⇧M", icon: <GitMerge size={15} />,
      disabled: !open || !!pr?.isDraft || conflicting || checks?.cls === "bad" || checks?.cls === "wait",
      run: () => task("merge", async () => { await api.mergePr(ws.id); onSay(`Merged #${pr!.number}. ${name}'s star shines.`); }),
    },
    mergeWhenGreen: {
      id: "mergeWhenGreen", label: "Merge when checks pass", keys: "", icon: <GitMerge size={15} />,
      disabled: !open || !!pr?.isDraft || conflicting || checks?.cls !== "wait",
      run: () => task("merge", async () => { await api.mergePr(ws.id, true); onSay(`GitHub will merge #${pr!.number} when its checks pass.`); }),
    },
    autopilot: {
      id: "autopilot", label: autopilot !== undefined ? "Turn off autopilot" : "Autopilot: get this PR to green", keys: "", icon: <Bot size={15} />,
      disabled: !open,
      run: () => task("autopilot", async () => {
        await api.setAutopilot(ws.id, autopilot === undefined);
        onSay(autopilot === undefined
          ? "Autopilot is on. It fixes failing checks, conflicts and requested changes, up to 3 agent turns, and pings you when the PR is green."
          : "Autopilot is off.");
      }),
    },
    openPr: { id: "openPr", label: "Open on GitHub", keys: "⌘⇧G", icon: <ExternalLink size={15} />, disabled: !pr, run: () => pr && openUrl(pr.url) },
  };
  // The one button you most likely want next.
  const primary =
    !pr || pr.state === "CLOSED" ? actions.createPr
    : conflicting ? actions.conflicts
    : open && checks?.cls === "bad" ? actions.fix
    : open && !actions.merge.disabled ? actions.merge
    : open && !actions.mergeWhenGreen.disabled ? actions.mergeWhenGreen
    : null;
  // Why Create PR is greyed out, same pattern as Merge below.
  const noChanges = fileCount === 0 && (!pr || pr.state === "CLOSED") ? "No changes yet. Ask the agent for a change first." : null;
  // Why Merge is greyed out, said in words.
  const mergeBlocked = !open ? null
    : pr?.isDraft ? "Draft PR. Mark it ready on GitHub first."
    : conflicting ? "Conflicts with the target branch"
    : checks?.cls === "bad" ? `${checks.text} checks`
    : checks?.cls === "wait" ? `${checks.text.replace(" running", "")} checks still running`
    : null;

  // Conductor's shortcuts, so muscle memory carries over.
  const ref = useRef(actions);
  ref.current = actions;
  useEffect(() => {
    const keys: Record<string, string> = { p: "createPr", y: "commitPush", l: "pull", r: "review", x: "fix", m: "merge", g: "openPr" };
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || !e.shiftKey || e.altKey) return;
      const a = ref.current[keys[e.key.toLowerCase()]];
      if (!a) return;
      e.preventDefault();
      if (!a.disabled) a.run();
    };
    // Slash commands from the chat (/pr, /review...) run the same actions.
    const slash: Record<string, string> = { pr: "createPr", review: "review", "second-opinion": "second", merge: "merge", pull: "pull", "fix-ci": "fix" };
    const onAction = (e: Event) => {
      const a = ref.current[slash[(e as CustomEvent<string>).detail]];
      if (!a) return;
      if (a.disabled) onSay(a.id === "merge" ? "This PR can't merge yet. Open the PR actions menu to see why." : `${a.label} isn't available for this PR yet.`);
      else a.run();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("nekotrail-action", onAction);
    return () => { window.removeEventListener("keydown", onKey); window.removeEventListener("nekotrail-action", onAction); };
  }, []);

  const renameBranch = async () => {
    const next = renaming?.trim();
    setRenaming(null);
    if (next && next !== ws.branch) await task("rename", () => api.renameBranch(ws.id, next));
  };

  return (
    <div className="prbar">
      {renaming !== null ? (
        <input
          className="branch-edit"
          autoFocus
          value={renaming}
          aria-label="Branch name"
          onChange={(e) => setRenaming(e.target.value)}
          onBlur={renameBranch}
          onKeyDown={(e) => { if (e.key === "Enter") renameBranch(); if (e.key === "Escape") setRenaming(null); }}
        />
      ) : (
        <button className="branch" title={pr ? `${ws.branch} into ${ws.target_branch}` : "Rename branch"} onClick={() => !pr && setRenaming(ws.branch)}>
          <GitBranch size={14} /><span className="branch-text">{ws.branch} → {ws.target_branch}</span>
        </button>
      )}
      {overlap.length > 0 && (
        <span className="pill wait overlap" title={overlap.map((o) => `${nameOf(o.workspace_id)}: ${o.files.join(", ")}`).join("\n")}>
          <TriangleAlert size={13} /><span className="pill-text">Same files as {nameOf(overlap[0].workspace_id)}{overlap.length > 1 ? ` and ${overlap.length - 1} more` : ""}</span>
        </span>
      )}
      <span className="spacer" />
      {pr === undefined && <span className="muted">Looking for a PR…</span>}
      {pr && (
        <>
          <Avatar src={avatar(pr.author.login)} size={24} label={pr.author.login} title={`Opened by @${pr.author.login}`} />
          <PrState pr={pr} />
          <button className="link num" onClick={() => openUrl(pr.url)} title={pr.title}>#{pr.number}</button>
          <span className="num"><span className="add">+{pr.additions}</span> <span className="del">-{pr.deletions}</span></span>
          {pr.comments.length > 0 && <span className="check num pr-comments" title={`${pr.comments.length} comments`}><MessageSquare size={14} />{pr.comments.length}</span>}
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
          {checks && <span className={`pill ${checks.cls}`} title={checks.names.join(", ")}><CheckIcon size={14} />CI {checks.text}</span>}
          {open && checks?.cls === "bad" && autopilot === undefined && <Mascot size={28} mood="worried" title="CI is failing" />}
          {autopilot !== undefined && (
            <button className="pill autopilot" onClick={() => actions.autopilot.run()} title="Autopilot keeps this PR moving. Click to turn it off.">
              <Bot size={13} />Autopilot · {autopilot} of 3 tries
            </button>
          )}
        </>
      )}
      {pr !== undefined && pr?.state !== "MERGED" && (
        <span className="git-actions">
          {primary && (
            <button className={primary.id === "merge" ? "good-btn" : "primary"} onClick={primary.run} disabled={!!busy || !!primary.disabled}
              title={primary.id === "createPr" && noChanges ? noChanges : primary.keys ? `${primary.label} (${primary.keys})` : undefined}>
              {busy ? <span className="spinner" /> : primary.icon}{primary.label}
            </button>
          )}
          <span className="git-menu">
            <button className="icon-btn" aria-label="More git actions" aria-expanded={menu} onClick={() => setMenu((m) => !m)} disabled={!!busy && !primary}>
              {busy && !primary ? <span className="spinner" /> : <ChevronDown size={15} />}
            </button>
            {menu && (
              <span className="menu" role="menu" onMouseLeave={() => setMenu(false)}>
                {Object.values(actions).filter((a) => a.id !== primary?.id && a.id !== "conflicts" && !(a.id === "mergeWhenGreen" && a.disabled)).map((a) => (
                  <button key={a.id} role="menuitem" disabled={a.disabled || !!busy} onClick={() => { setMenu(false); a.run(); }}
                    title={a.id === "merge" && mergeBlocked ? mergeBlocked : a.id === "manualPr" && noChanges ? noChanges : undefined}>
                    {a.icon}
                    <span>{a.label}{a.id === "merge" && mergeBlocked && <small className="why">{mergeBlocked}</small>}{a.id === "manualPr" && noChanges && <small className="why">{noChanges}</small>}</span>
                    {a.keys && <kbd>{a.keys}</kbd>}
                  </button>
                ))}
              </span>
            )}
          </span>
        </span>
      )}
      {pr?.state === "MERGED" && (
        <div className="merged-banner" role="status">
          <Mascot size={36} mood="happy" title="Merged" />
          <span><b>Merged.</b> {name}'s work is in {ws.target_branch}. Archive it to tidy up. The branch is kept.</span>
          <span className="spacer" />
          <button className="primary" onClick={() => onArchive()}>Archive {name}</button>
        </div>
      )}
      {form && (
        <div className="pr-form">
          <input placeholder="PR title" aria-label="PR title" value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} autoFocus />
          <textarea placeholder="What changed and why" aria-label="PR description" rows={4} value={form.body} onChange={(e) => setForm({ ...form, body: e.target.value })} />
          <div className="row">
            <label className="check"><input type="checkbox" checked={form.draft} onChange={(e) => setForm({ ...form, draft: e.target.checked })} /> Open as draft</label>
            <span className="spacer" />
            <button className="quiet" onClick={() => setForm(null)}>Cancel</button>
            <button className="primary" disabled={!!busy || !form.title.trim()} onClick={create}>
              {busy === "pr" ? <span className="spinner" /> : <GitPullRequest size={16} />}
              {busy === "pr" ? "Pushing…" : "Push and create PR"}
            </button>
          </div>
        </div>
      )}
      {error && /auth|log ?in|token|credentials/i.test(error) ? (
        <button className="accent" onClick={() => window.dispatchEvent(new CustomEvent("nekotrail-terminal", { detail: "gh auth login" }))} title={error}>
          <CircleX size={14} />Sign in to GitHub
        </button>
      ) : error && <span className="pill bad" title={error}><CircleX size={14} />GitHub: {error.split("\n")[0].slice(0, 60)}</span>}
    </div>
  );
}
