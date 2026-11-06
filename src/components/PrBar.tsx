import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Bot, ArrowDownToLine, ChevronDown, CircleCheck, CircleDashed, CircleX, ExternalLink, GitBranch, GitMerge, GitPullRequest, GitPullRequestClosed, GitPullRequestDraft, Layers, MessageSquare, ScanSearch, Sparkles, SquareStack, Upload } from "lucide-react";
import { api, avatar, errText, prFromView, type Collision, type Pr, type PrView, type Repo, type StackParent, type Workspace } from "../api";
import { ask } from "./Confirm";
import { Mascot } from "./Mascot";
import { Avatar } from "./Avatar";
import { getSettings } from "../settings";
import { keyHint, mod } from "../platform";
import { agentName, otherAgent } from "../agents";
import { useFit, overflows } from "../useFit";

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

/** Why a workspace sits where it does in the merge order, in a few words. */
export function mergeReason(pr: string | undefined, lines: [number, number] | undefined): string {
  const n = lines ? lines[0] + lines[1] : 0;
  const state = pr === "OPEN" ? "PR ready" : pr === "DRAFT" ? "draft PR" : "no PR yet";
  return `${state}, ${n.toLocaleString()} ${n === 1 ? "line" : "lines"}`;
}

/** Collision radar details: which files overlap, and a suggested merge order for the group. */
function RadarPopover({ here, collision, overlap, nameOf, textNameOf, prStates, lines, onOpen, onClose }: {
  here: string;
  collision: Collision;
  overlap: { workspace_id: string; files: string[] }[];
  nameOf: (id: string) => string;
  textNameOf: (id: string) => string;
  prStates: Record<string, string>;
  lines: Record<string, [number, number]>;
  onOpen: (id: string) => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    box.current?.querySelector<HTMLElement>("button")?.focus();
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    const onDown = (e: MouseEvent) => { if (!box.current?.parentElement?.contains(e.target as Node)) onClose(); };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("mousedown", onDown);
    return () => { window.removeEventListener("keydown", onKey, true); window.removeEventListener("mousedown", onDown); };
  }, []);
  const first = collision.order[0];
  return (
    <div className="menu radar-pop" role="dialog" aria-label="Workspaces changing the same files" ref={box}>
      <div className="radar-sec">
        {overlap.map((o) => (
          <p key={o.workspace_id} className="radar-files">
            <b>{nameOf(o.workspace_id)}</b> also changes <code>{o.files.slice(0, 3).join(", ")}</code>
            {o.files.length > 3 && <> and {o.files.length - 3} more</>}
          </p>
        ))}
      </div>
      <div className="menu-label">Suggested merge order</div>
      <ol className="radar-order">
        {collision.order.map((id) => (
          <li key={id}>
            {id === here ? (
              <span className="radar-row here"><span className="radar-name">{nameOf(id)} <span className="muted">(this one)</span></span><small>{mergeReason(prStates[id], lines[id])}</small></span>
            ) : (
              <button className="radar-row" onClick={() => onOpen(id)} title={`Open ${nameOf(id)}`}>
                <span className="radar-name">{nameOf(id)}</span><small>{mergeReason(prStates[id], lines[id])}</small>
              </button>
            )}
          </li>
        ))}
      </ol>
      <p className="menu-hint muted">
        {first === here ? "Merge this one first, then pull it into the others." : `Merge ${textNameOf(first)} first, then pull it in here before you merge this one.`}
        {" "}Ready PRs go first, then the smaller change, so the next one rebases over less.
      </p>
    </div>
  );
}

/** Prompts for the git actions the agent does for you. The repo's own instructions are added. */
export const agentPrompts = {
  createPr: (ws: Workspace, repo?: Repo) =>
    `Open a pull request for this workspace:
1. Commit any uncommitted work with a clear message.
2. Push the branch \`${ws.branch}\` to origin.${ws.parent_id ? ` This PR is stacked: it goes into \`${ws.target_branch}\`, so push that branch to origin too if it isn't there yet.` : ""}
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
  restack: (ws: Workspace, parentName: string, base: string, fork: string) =>
    `This branch was stacked on \`${ws.target_branch}\` (the ${parentName} workspace), whose PR has now merged into \`${base}\`. Move this branch onto \`${base}\`, keeping only its own commits:
1. \`git fetch origin ${base}\`
2. \`git rebase --onto origin/${base} ${fork || ws.target_branch}\` (everything after that commit is this branch's own work).
3. Resolve each conflict keeping the intent of both sides, run the tests, and finish the rebase.
4. \`git push --force-with-lease origin ${ws.branch}\`
5. If this branch has a PR, run \`gh pr edit --base ${base}\`.
Then tell me it's done, and I'll press "Move onto ${base}" once more to update nekotrail.`,
};

type Action = { id: string; label: string; keys: string; icon: React.ReactNode; run: () => void; disabled?: boolean };

const mergeLabel = (repo?: Repo) =>
  repo?.prefs?.merge_method === "merge" ? "Merge" : repo?.prefs?.merge_method === "rebase" ? "Rebase and merge" : "Squash and merge";

export function PrBar({
  ws,
  repo,
  name,
  refreshKey,
  collision,
  prStates,
  prView,
  parentPr,
  lines,
  onOpenWorkspace,
  onSendAgent,
  onArchive,
  onSay,
  autopilot,
  nameOf,
  textNameOf,
  star,
  stackedOnThis,
  onRestacked,
}: {
  ws: Workspace;
  repo?: Repo;
  /** This workspace's name inside a sentence (a task title comes quoted). */
  name: string;
  /** Its star name, which shines when the PR merges. */
  star: string;
  refreshKey: number;
  /** This workspace's collision group, if another workspace changes the same files. */
  collision?: Collision;
  prStates: Record<string, string>;
  /** This workspace's PR from the background poller; undefined until its first look. */
  prView?: PrView;
  /** The PR of the workspace this one is stacked on, from the same poller. */
  parentPr?: Pr;
  lines: Record<string, [number, number]>;
  onOpenWorkspace: (id: string) => void;
  /** Send a prompt to the agent right away. */
  onSendAgent: (prompt: string) => void;
  /** `auto` is true when archiving by itself after a merge; `id` is set once it is no longer the open workspace. */
  onArchive: (auto?: boolean, id?: string) => void;
  onSay: (text: string, error?: boolean) => void;
  /** Agent turns autopilot has used on this PR; undefined when it's off. */
  autopilot?: number;
  /** Another workspace's name for a label or a list. */
  nameOf: (workspaceId: string) => string;
  /** Another workspace's name inside a sentence. */
  textNameOf: (workspaceId: string) => string;
  /** Active workspaces stacked on this one. */
  stackedOnThis: string[];
  /** The workspace moved off its stack: reload the workspace list. */
  onRestacked: () => void;
}) {
  // The PR comes from the poller (see prFromView for how problems show).
  const { pr, error } = prFromView(prView);
  const [form, setForm] = useState<{ title: string; body: string; draft: boolean } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [menu, setMenu] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [radarOpen, setRadarOpen] = useState(false);
  useEffect(() => setRadarOpen(false), [ws.id]);
  // Workspaces sharing files with this one directly, and which files.
  const overlap = (collision?.pairs ?? [])
    .filter((p) => p.a === ws.id || p.b === ws.id)
    .map((p) => ({ workspace_id: p.a === ws.id ? p.b : p.a, files: p.files }));

  // Stacked PRs: the workspace this one sits on, and its PR (from the poller's cache, so no gh call).
  const [parent, setParent] = useState<StackParent | undefined>(undefined);
  // Ask the poller for a look (it skips one if its last look is under 15 s old).
  const load = () => { api.prRefresh(ws.id).catch(() => {}); };
  useEffect(() => {
    setForm(null);
    setMenu(false);
    load();
  }, [ws.id, refreshKey]);
  useEffect(() => { setParent(undefined); }, [ws.id, ws.parent_id]);
  // "Add to PR description" from Export session: the summary goes into the draft, under what's there.
  // Nothing is pushed or posted until you press Push and create PR.
  useEffect(() => {
    const on = (e: Event) => {
      const { workspaceId, body } = (e as CustomEvent<{ workspaceId: string; body: string }>).detail;
      if (workspaceId !== ws.id) return;
      setForm((f) => f ? { ...f, body: f.body.trim() ? `${f.body.trim()}\n\n${body}` : body } : { title: ws.title, body, draft: false });
    };
    window.addEventListener("nekotrail-pr-draft", on);
    return () => window.removeEventListener("nekotrail-pr-draft", on);
  }, [ws.id, ws.title]);
  useEffect(() => {
    if (ws.parent_id) api.stackParent(ws.id).then(setParent).catch(() => setParent(null));
  }, [ws.id, ws.parent_id, parentPr?.state, parentPr?.number, parentPr?.baseRefName]);

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
    try {
      await fn();
    } catch (e) {
      onSay(errText(e), true);
    } finally {
      setBusy(null);
      load();
    }
  };

  // The parent PR merged: move this workspace onto where it merged, after one confirm.
  const parentName = parent ? nameOf(parent.id) : "";
  const parentText = parent ? textNameOf(parent.id) : "";
  const parentMerged = parent?.pr?.state === "MERGED";
  const newBase = parent?.pr?.baseRefName || parent?.target || "";
  const restack = async () => {
    if (!parent) return;
    const ok = await ask({
      title: `Move ${name} onto ${newBase}?`,
      body: `The PR for ${parentText} merged into ${newBase}. nekotrail replays only this workspace's own commits on top of ${newBase}, updates the branch on GitHub (a force push with lease, so someone else's push is never overwritten) and points the PR at ${newBase}. Commit any uncommitted work first. If git hits a conflict, nothing changes and the agent gets the job instead.`,
      ok: `Move onto ${newBase}`,
    });
    if (!ok) return;
    await task("restack", async () => {
      const r = await api.restackWorkspace(ws.id);
      if (r.result === "conflicts") {
        onSay(`Moving onto ${r.base} hit conflicts, so nothing changed. The agent is doing it by hand.`);
        onSendAgent(agentPrompts.restack(ws, parentText, r.base, r.fork ?? ""));
        return;
      }
      onSay(r.result === "rebased"
        ? `${name} now sits on ${r.base}${r.pushed ? ", pushed" : ""}${r.pr ? `, and PR #${r.pr} points at ${r.base}` : ""}.`
        : `${name} already had everything it needs, so it now targets ${r.base}${r.pr ? ` and PR #${r.pr} points there` : ""}.`);
      onRestacked();
    });
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
    createPr: { id: "createPr", label: "Create PR", keys: keyHint("⌘⇧P"), icon: <GitPullRequest size={15} />, run: () => onSendAgent(agentPrompts.createPr(ws, repo)), disabled: (!!pr && pr.state !== "CLOSED") || fileCount === 0 },
    manualPr: { id: "manualPr", label: "Write the PR myself", keys: "", icon: <GitPullRequest size={15} />, run: () => setForm({ title: ws.title, body: "", draft: false }), disabled: (!!pr && pr.state !== "CLOSED") || fileCount === 0 },
    commitPush: { id: "commitPush", label: "Commit and push", keys: keyHint("⌘⇧Y"), icon: <Upload size={15} />, run: () => task("push", async () => onSay(`Pushed: ${await api.commitPush(ws.id)}`)) },
    pull: {
      id: "pull", label: `Pull latest ${ws.target_branch}`, keys: keyHint("⌘⇧L"), icon: <ArrowDownToLine size={15} />,
      run: () => task("pull", async () => {
        const r = await api.pullTarget(ws.id);
        if (r === "conflicts") { onSay(`Merging ${ws.target_branch} hit conflicts. The agent is resolving them.`); onSendAgent(agentPrompts.conflicts(ws)); }
        else onSay(r === "merged" ? `Merged the latest ${ws.target_branch}.` : `Already up to date with ${ws.target_branch}.`);
      }),
    },
    review: { id: "review", label: "Review changes", keys: keyHint("⌘⇧R"), icon: <ScanSearch size={15} />, run: () => onSendAgent(agentPrompts.review(ws, repo)) },
    second: {
      id: "second", label: "Get a second opinion from the other agent", keys: "", icon: <ScanSearch size={15} />,
      run: () => task("second", async () => {
        const last = (await api.sessions(ws.id)).pop();
        const other = otherAgent(last?.agent);
        await api.startChat(ws.id, other, "plan", `Review by ${agentName(other)}`, agentPrompts.secondOpinion(ws, repo));
        onSay(`${agentName(other)} is reviewing in a new read-only chat. Add its findings to your review from there.`);
      }),
    },
    fix: {
      id: "fix", label: "Fix failing checks", keys: keyHint("⌘⇧X"), icon: <Sparkles size={15} />, disabled: !(open && checks?.cls === "bad"),
      run: () => task("fix", async () => onSendAgent(agentPrompts.fixErrors(pr!.number, await api.failingLogs(ws.id)))),
    },
    conflicts: { id: "conflicts", label: "Resolve conflicts", keys: "", icon: <Sparkles size={15} />, disabled: !conflicting, run: () => actions.pull.run() },
    merge: {
      id: "merge", label: mergeLabel(repo), keys: keyHint("⌘⇧M"), icon: <GitMerge size={15} />,
      disabled: !open || !!pr?.isDraft || conflicting || checks?.cls === "bad" || checks?.cls === "wait",
      run: () => task("merge", async () => { await api.mergePr(ws.id); onSay(`Merged #${pr!.number}. ${star} shines.`); }),
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
    openPr: { id: "openPr", label: "Open on GitHub", keys: keyHint("⌘⇧G"), icon: <ExternalLink size={15} />, disabled: !pr, run: () => pr && openUrl(pr.url) },
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
      if (!mod(e) || !e.shiftKey || e.altKey) return;
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

  // One line at any width, folding instead of clipping: 1 the branch goes (the toolbar subtitle has it)
  // once it would show only a few letters, 2 avatars and comments go, 3 counts and pill texts become
  // icons, 4 the overlap note keeps only the other workspace's name, 5 that and the main button become
  // icons too. Every hidden thing keeps its tooltip.
  const branchShown = (el: HTMLElement) => el.querySelector<HTMLElement>(":scope > button.branch .branch-text")?.clientWidth || 999;
  const [barRef, fold] = useFit(5, (el) => overflows(el) || branchShown(el) < 110,
    [ws.id, pr?.number, pr?.state, checks?.text, autopilot, collision?.pairs.length, stackedOnThis.length, !!parent, renaming !== null, !!form, primary?.id, busy]);

  const renameBranch = async () => {
    const next = renaming?.trim();
    setRenaming(null);
    if (next && next !== ws.branch) await task("rename", () => api.renameBranch(ws.id, next));
  };

  return (
    <div data-tour="prbar" className={["prbar", ...[1, 2, 3, 4, 5].filter((n) => fold >= n).map((n) => `pb-${n}`)].join(" ")} ref={barRef}>
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
      {ws.parent_id && parent && (
        <span className="stack-link" title={`Stacked on ${parentText}: this PR goes into its branch ${parent.branch}`}>
          <SquareStack size={13} />
          <span className="pill-text">On <button className="link stack-name" onClick={() => onOpenWorkspace(parent.id)} title={`Open ${parentText}`}>{parentName}</button></span>
          {parent.pr && (
            <button className={`link num ${parentMerged ? "merged" : ""}`} onClick={() => openUrl(parent.pr!.url)} title={`${parent.pr.title} (${parent.pr.state.toLowerCase()})`}>
              #{parent.pr.number}{parentMerged ? " merged" : ""}
            </button>
          )}
        </span>
      )}
      {stackedOnThis.length > 0 && (
        <span className="stack-link" title={`Stacked on this one: ${stackedOnThis.map(textNameOf).join(", ")}. Merge this PR first.`}>
          <SquareStack size={13} />
          <span className="pill-text">{stackedOnThis.length === 1 ? `${textNameOf(stackedOnThis[0])} builds on this` : `${stackedOnThis.length} build on this`}</span>
        </span>
      )}
      {collision && overlap.length > 0 && (
        <span className="git-menu overlap">
          <button
            className="pill wait"
            aria-expanded={radarOpen}
            aria-haspopup="dialog"
            onClick={() => setRadarOpen((o) => !o)}
            title="Another workspace changes the same files. Click for a merge order."
          >
            <Layers size={13} /><span className="pill-text"><span className="pb-long">Same files as </span>{textNameOf(overlap[0].workspace_id)}{overlap.length > 1 ? ` and ${overlap.length - 1} more` : ""}</span>
          </button>
          {radarOpen && (
            <RadarPopover
              here={ws.id}
              collision={collision}
              overlap={overlap}
              nameOf={nameOf}
              textNameOf={textNameOf}
              prStates={prStates}
              lines={lines}
              onOpen={(id) => { setRadarOpen(false); onOpenWorkspace(id); }}
              onClose={() => setRadarOpen(false)}
            />
          )}
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
          {pr.commentCount > 0 && <span className="check num pr-comments" title={`${pr.commentCount} comments`}><MessageSquare size={14} />{pr.commentCount}</span>}
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
          {checks && <span className={`pill ${checks.cls}`} title={`CI ${checks.text}: ${checks.names.join(", ")}`}><CheckIcon size={14} /><span className="pb-text">CI {checks.text}</span></span>}
          {open && checks?.cls === "bad" && autopilot === undefined && <Mascot size={28} mood="worried" title="CI is failing" />}
          {autopilot !== undefined && (
            <button className="pill autopilot" onClick={() => actions.autopilot.run()} title="Autopilot keeps this PR moving. Click to turn it off.">
              <Bot size={13} /><span className="pb-text">Autopilot · {autopilot} of 3 tries</span>
            </button>
          )}
        </>
      )}
      {pr !== undefined && pr?.state !== "MERGED" && (
        <span className="git-actions">
          {primary && (
            <button className={primary.id === "merge" ? "good-btn" : "primary"} onClick={primary.run} disabled={!!busy || !!primary.disabled} aria-label={primary.label}
              title={primary.id === "createPr" && noChanges ? noChanges : primary.keys ? `${primary.label} (${primary.keys})` : undefined}>
              {busy ? <span className="spinner" /> : primary.icon}<span className="pb-label">{primary.label}</span>
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
      {ws.parent_id && parentMerged && pr?.state !== "MERGED" && (
        <div className="merged-banner stack-banner" role="status">
          <SquareStack size={18} className="muted" aria-hidden />
          <span><b>The PR for {parentText} merged into {newBase}.</b> Move this workspace onto {newBase} so its PR shows only its own changes.</span>
          <span className="spacer" />
          <button className="primary" onClick={restack} disabled={!!busy}>
            {busy === "restack" ? <span className="spinner" /> : <SquareStack size={15} />}Move onto {newBase}
          </button>
        </div>
      )}
      {pr?.state === "MERGED" && (
        <div className="merged-banner" role="status">
          <Mascot size={36} mood="happy" title="Merged" />
          <span><b>Merged.</b> The work is in {ws.target_branch}. Archive the workspace to tidy up. The branch is kept.</span>
          <span className="spacer" />
          <button className="primary" onClick={() => onArchive()}>Archive workspace</button>
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
