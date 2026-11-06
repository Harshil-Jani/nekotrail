import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowDown, ArrowUp, Check, GitCommitHorizontal, Lock, Minus, Plus, RefreshCw, Sparkles, Split, Undo2, X } from "lucide-react";
import { api, errText, type HeadInfo, type SplitPlan, type StageDiff, type StageFile, type StageStatus } from "../api";
import { keyHint, mod } from "../platform";
import { changeNumbers, hunkChanges, linesWord, moveGroup, moveHunk, pickLine, planBlocker, protectedIn, subjectHint, wholeOnly } from "../stage";
import { parseDiff } from "./Diff";
import { ask } from "./Confirm";
import { Mascot } from "./Mascot";

type Open = { path: string; staged: boolean; old: string | null };

const msgKey = (id: string) => `nekotrail.commitmsg.${id}`;

/** "Stage a protected file?" before staging any. Resolves true to go ahead. */
async function okToStage(paths: string[]): Promise<boolean> {
  if (!paths.length) return true;
  return ask({
    title: paths.length === 1 ? "Stage a protected file?" : `Stage ${paths.length} protected files?`,
    body: `${paths.join(", ")} ${paths.length === 1 ? "is" : "are"} protected in this repo, so agents may not change ${paths.length === 1 ? "it" : "them"}. Check the change is one you want before it goes into a commit.`,
    ok: "Stage anyway",
  });
}

/**
 * Changes, Commit: the staged and unstaged files, a diff where hunks and single lines can be
 * staged, and the commit message box with Draft message, Split into commits, Amend and Undo.
 * Everything here changes only git's staging area and commits; files on disk stay as they are.
 */
export function CommitView({ workspaceId, refreshKey, scope }: { workspaceId: string; refreshKey: number; scope: React.ReactNode }) {
  const [status, setStatus] = useState<StageStatus | null>(null);
  const [head, setHead] = useState<HeadInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [open, setOpen] = useState<Open | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessageRaw] = useState(() => localStorage.getItem(msgKey(workspaceId)) ?? "");
  const setMessage = (m: string) => { setMessageRaw(m); if (m) localStorage.setItem(msgKey(workspaceId), m); else localStorage.removeItem(msgKey(workspaceId)); };
  const [amend, setAmend] = useState(false);
  const [plan, setPlan] = useState<SplitPlan | null>(null);
  const [rev, setRev] = useState(0);

  useEffect(() => { setMessageRaw(localStorage.getItem(msgKey(workspaceId)) ?? ""); setAmend(false); setNote(null); setNotice(null); setOpen(null); setPlan(null); }, [workspaceId]);

  const load = useCallback(() => {
    setRev((r) => r + 1);
    api.stageStatus(workspaceId).then((s) => {
      setStatus(s);
      setError(null);
      setOpen((cur) => {
        const list = cur ? (cur.staged ? s.staged : s.unstaged) : [];
        if (cur && list.some((f) => f.path === cur.path)) return cur;
        // The file moved to the other list (staged in full): follow it there.
        const moved = cur && (cur.staged ? s.unstaged : s.staged).find((f) => f.path === cur.path);
        if (moved) return { path: moved.path, staged: !cur!.staged, old: moved.old_path };
        const first = s.unstaged[0] ?? s.staged[0];
        return first ? { path: first.path, staged: !s.unstaged.length, old: first.old_path } : null;
      });
    }).catch((e) => setError(errText(e)));
    api.headCommit(workspaceId).then(setHead).catch(() => setHead(null));
  }, [workspaceId]);
  useEffect(load, [load, refreshKey]);

  /** Run one git action, then reload. Errors show above the list. */
  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(label);
    setNotice(null);
    try { await fn(); } catch (e) { setNotice(errText(e)); } finally { setBusy(null); load(); }
  };

  const stageFile = async (f: StageFile, staged: boolean) => {
    if (!staged && f.protected && !(await okToStage([f.path]))) return;
    run("file", () => api.stageApply(workspaceId, f.path, staged, f.old_path));
  };
  const stageAll = async (stage: boolean) => {
    if (stage && !(await okToStage(protectedIn(status?.unstaged ?? [])))) return;
    run("all", () => api.stageEverything(workspaceId, stage));
  };

  const staged = status?.staged ?? [];
  const unstaged = status?.unstaged ?? [];
  const hint = subjectHint(message);
  const canCommit = !!message.trim() && (staged.length > 0 || amend) && !busy;
  const stagedProtected = protectedIn(staged);

  const commit = async () => {
    if (!canCommit) return;
    if (stagedProtected.length && !(await ask({
      title: `Commit ${stagedProtected.length === 1 ? "a protected file" : `${stagedProtected.length} protected files`}?`,
      body: `${stagedProtected.join(", ")} ${stagedProtected.length === 1 ? "is" : "are"} protected in this repo.`,
      ok: "Commit anyway",
    }))) return;
    setBusy("commit");
    setNotice(null);
    try {
      const sha = await api.commitStaged(workspaceId, message, amend);
      setNote(`${amend ? "Amended" : "Committed"} ${sha}: ${message.split("\n")[0].trim()}`);
      setMessage("");
      setAmend(false);
    } catch (e) {
      setNotice(errText(e));
    } finally {
      setBusy(null);
      load();
    }
  };

  const draft = async () => {
    setBusy("draft");
    setNotice(null);
    setNote(null);
    try {
      const d = await api.draftCommitMessage(workspaceId);
      setMessage(d.message);
      setNote(d.note);
    } catch (e) {
      setNotice(errText(e));
    } finally {
      setBusy(null);
    }
  };

  const split = async () => {
    setBusy("split");
    setNotice(null);
    try { setPlan(await api.splitPlan(workspaceId)); } catch (e) { setNotice(errText(e)); } finally { setBusy(null); }
  };

  const undo = async () => {
    if (!head) return;
    const ok = await ask({
      title: "Undo the last commit?",
      body: `"${head.subject}" (${head.short}) is taken off the branch. Its changes come back staged, so nothing is lost and you can commit them again.`,
      ok: "Undo commit",
    });
    if (!ok) return;
    run("undo", async () => {
      const subject = await api.undoCommit(workspaceId);
      if (!message.trim()) setMessage(head.message);
      setNote(`Undid "${subject}". Its changes are staged again.`);
    });
  };

  const toggleAmend = (on: boolean) => {
    setAmend(on);
    if (on && !message.trim() && head) setMessage(head.message);
  };

  // ⌘↵ in the message box commits.
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && mod(e)) { e.preventDefault(); commit(); }
  };

  const section = (title: string, files: StageFile[], isStaged: boolean) => (
    <div className="stage-section" role="group" aria-label={title}>
      <div className="stage-section-head">
        <span>{title}</span>
        <span className="stage-count num">{files.length}</span>
        <span className="spacer" />
        {files.length > 0 && (
          <button className="quiet" onClick={() => stageAll(!isStaged)} disabled={!!busy}>
            {isStaged ? "Unstage all" : "Stage all"}
          </button>
        )}
      </div>
      {files.length === 0 && (
        <p className="muted stage-empty">{isStaged ? "Nothing staged. Stage files, hunks or lines from below." : "No other changes."}</p>
      )}
      {files.map((f) => {
        const here = open?.path === f.path && open.staged === isStaged;
        return (
          <div key={f.path} className={`file stage-file ${here ? "selected" : ""}`}>
            <button onClick={() => setOpen({ path: f.path, staged: isStaged, old: f.old_path })} title={f.old_path ? `${f.old_path} → ${f.path}` : f.path}>
              <span className={`status s-${f.status === "?" ? "A" : f.status}`} title={f.status === "?" ? "New, not tracked yet" : undefined}>{f.status === "?" ? "A" : f.status}</span>
              <span className="file-path">
                <span className="file-name">{f.path.split("/").pop()}</span>
                {f.path.includes("/") && <span className="file-dir">{f.path.slice(0, f.path.lastIndexOf("/"))}</span>}
              </span>
              {f.protected && <Lock size={12} className="stage-lock" aria-label="Protected" />}
              {!f.binary && <><span className="add">+{f.additions}</span><span className="del">-{f.deletions}</span></>}
            </button>
            <button
              className="icon stage-toggle"
              aria-label={`${isStaged ? "Unstage" : "Stage"} ${f.path}`}
              title={isStaged ? "Unstage file" : "Stage file"}
              onClick={() => stageFile(f, isStaged)}
              disabled={!!busy}
            >
              {isStaged ? <Minus size={14} /> : <Plus size={14} />}
            </button>
          </div>
        );
      })}
    </div>
  );

  const nothing = status && !staged.length && !unstaged.length;

  return (
    <div className="diff-view commit-view">
      <div className="file-list">
        <div className="file-list-head">
          {scope}
          <span className="spacer" /><button className="icon" onClick={load} title="Refresh" aria-label="Refresh"><RefreshCw size={15} /></button>
        </div>
        {notice && <div className="banner error file-notice" role="alert">{notice}</div>}
        {error ? <div className="banner error">{error}</div> : !status ? (
          <div className="empty"><span className="spinner" /></div>
        ) : (
          <div className="stage-lists">
            {section("Staged", staged, true)}
            {section("Changes", unstaged, false)}
          </div>
        )}
        <div className="commit-box">
          <textarea
            className="commit-msg"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={onKey}
            placeholder={"Subject line\n\nWhat changed and why"}
            aria-label="Commit message"
            rows={4}
            spellCheck
          />
          <span className={`subject-hint t-${hint.tone}`} aria-live="polite">{hint.text}</span>
          <div className="commit-tools">
            <button className="quiet" onClick={draft} disabled={!!busy || !staged.length} title={staged.length ? "Ask the agent for a message from the staged changes, in a side question that stays out of your chat" : "Stage something first"}>
              {busy === "draft" ? <span className="spinner" /> : <Sparkles size={13} />}Draft message
            </button>
            <button className="quiet" onClick={split} disabled={!!busy || !!nothing} title="The agent groups your uncommitted changes into commits for you to check">
              {busy === "split" ? <span className="spinner" /> : <Split size={13} />}Split into commits
            </button>
          </div>
          {note && <p className="commit-note muted" role="status">{note}</p>}
          {stagedProtected.length > 0 && (
            <p className="commit-warn" role="note"><Lock size={12} />{stagedProtected.length === 1 ? `Protected file staged: ${stagedProtected[0]}` : `${stagedProtected.length} protected files staged`}</p>
          )}
          <label className="check commit-amend" title={head?.block ?? (head ? `Replace ${head.short} with what's staged and this message` : "There's no commit yet")}>
            <input type="checkbox" checked={amend} disabled={!head || !!head.block} onChange={(e) => toggleAmend(e.target.checked)} />
            Amend last commit
          </label>
          <div className="commit-actions">
            <button className="primary" onClick={commit} disabled={!canCommit} title={keyHint(amend ? "Amend the last commit (⌘↵)" : "Commit the staged changes (⌘↵)")}>
              {busy === "commit" ? <span className="spinner" /> : <GitCommitHorizontal size={15} />}
              {amend ? "Amend" : staged.length ? `Commit ${staged.length} ${staged.length === 1 ? "file" : "files"}` : "Commit"}
            </button>
          </div>
          {head && (
            <div className="last-commit">
              <span className="muted">Last</span>
              <code>{head.short}</code>
              <span className="last-subject" title={head.subject}>{head.subject}</span>
              {head.pushed && <span className="chip pushed" title={head.block ?? undefined}>Pushed</span>}
              <button className="quiet" onClick={undo} disabled={!!busy || !!head.block} title={head.block ?? "Take this commit off the branch and keep its changes staged"}>
                <Undo2 size={13} />Undo
              </button>
            </div>
          )}
        </div>
      </div>
      <div className="file-diff">
        {nothing ? (
          <div className="empty"><Mascot size={64} mood="happy" /><h2>Nothing to commit</h2><p>Every change is committed.</p></div>
        ) : open ? (
          <StagePane key={`${open.staged}:${open.path}`} workspaceId={workspaceId} open={open} rev={rev}
            file={(open.staged ? staged : unstaged).find((f) => f.path === open.path)}
            busy={!!busy} run={run} />
        ) : (
          <div className="empty"><Mascot size={64} mood="sleep" /><h2>No file selected</h2><p>Pick a file to stage its hunks or lines.</p></div>
        )}
      </div>
      {plan && <SplitSheet workspaceId={workspaceId} plan={plan} onClose={(done) => { setPlan(null); if (done) { setNote(done); load(); } }} />}
    </div>
  );
}

/** One file's staged or unstaged diff, with Stage hunk on each hunk and pickable change lines. */
function StagePane({ workspaceId, open, file, rev, busy, run }: {
  workspaceId: string;
  open: Open;
  file?: StageFile;
  rev: number;
  busy: boolean;
  run: (label: string, fn: () => Promise<unknown>) => Promise<void>;
}) {
  const [d, setD] = useState<StageDiff | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [sel, setSel] = useState<Set<number>>(new Set());
  const anchor = useRef<number | null>(null);
  useEffect(() => {
    let on = true;
    api.stageDiff(workspaceId, open.path, open.staged, open.old).then((x) => { if (on) { setD(x); setErr(null); } }).catch((e) => on && setErr(errText(e)));
    return () => { on = false; };
  }, [workspaceId, open.path, open.staged, open.old, rev]);
  // A new diff numbers its lines afresh: drop the old selection.
  useEffect(() => { setSel(new Set()); anchor.current = null; }, [d?.sig]);
  const lines = useMemo(() => parseDiff(d?.text ?? ""), [d?.text]);
  const nums = useMemo(() => changeNumbers(lines), [lines]);
  const verb = open.staged ? "Unstage" : "Stage";

  const apply = async (picked: number[]) => {
    if (!d || !picked.length) return;
    if (!open.staged && file?.protected && !(await okToStage([open.path]))) return;
    await run("lines", () => api.stageApply(workspaceId, open.path, open.staged, open.old, { sig: d.sig, lines: picked }));
  };
  const whole = async () => {
    if (!file) return;
    if (!open.staged && file.protected && !(await okToStage([open.path]))) return;
    await run("file", () => api.stageApply(workspaceId, open.path, open.staged, open.old));
  };

  const head = (
    <div className="file-head">
      <code title={open.old ? `${open.old} → ${open.path}` : open.path}>{open.old ? `${open.old} → ${open.path}` : open.path}</code>
      <span className={`chip ${open.staged ? "staged" : "unstaged"}`}>{open.staged ? "Staged" : "Not staged"}</span>
      {file?.protected && <span className="chip protected" title="Protected in this repo: agents may not change it"><Lock size={11} />Protected</span>}
      <span className="spacer" />
      {sel.size > 0 && (
        <>
          <button className="quiet" onClick={() => setSel(new Set())} title="Clear the selected lines"><X size={13} />Clear</button>
          <button className="primary" onClick={() => apply([...sel])} disabled={busy}>{open.staged ? <Minus size={13} /> : <Plus size={13} />}{verb} {linesWord(sel.size)}</button>
        </>
      )}
      <button className={sel.size ? "quiet" : ""} onClick={whole} disabled={busy || !file}>{verb} file</button>
    </div>
  );

  if (err) return <>{head}<div className="banner error">{err}</div></>;
  if (!d) return <>{head}<div className="empty"><span className="spinner" /></div></>;
  if (d.binary || (file && wholeOnly(file))) {
    return <>{head}<div className="empty"><h2>{file?.status === "U" ? "This file has conflicts" : "Binary file"}</h2><p>{file?.status === "U" ? "Resolve them, then stage the file whole to mark it resolved." : "It can only be staged or unstaged whole."}</p></div></>;
  }
  if (!lines.length) {
    return <>{head}<div className="empty"><h2>No line changes</h2><p>Only the file's mode or name changed. Stage or unstage it whole.</p></div></>;
  }

  return (
    <>
      {head}
      <div className="diff stage-diff" role="table" aria-label={`${open.staged ? "Staged" : "Unstaged"} changes in ${open.path}`}>
        {lines.map((l, i) => {
          if (l.kind === "hunk") {
            const nums = hunkChanges(lines, i);
            return (
              <div key={i} className="dl hunk stage-hunk">
                <span className="ln" /><span className="ln" />
                <div className="hunk-bar">
                  <code>{l.text}</code>
                  <button className="quiet hunk-btn" onClick={() => apply(nums)} disabled={busy || !nums.length}>{open.staged ? <Minus size={12} /> : <Plus size={12} />}{verb} hunk</button>
                </div>
              </div>
            );
          }
          const n = nums[i];
          const picked = n !== null && sel.has(n);
          const pick = (shift: boolean) => { const from = anchor.current; setSel((s) => pickLine(s, lines, i, from, shift)); anchor.current = i; };
          return (
            <div key={i} className={`dl ${l.kind}${picked ? " picked" : ""}`}>
              <span className="ln">{l.old ?? ""}</span>
              <span className="ln">{l.new ?? ""}</span>
              {n === null ? <span /> : (
                <button
                  className="pick-line"
                  aria-pressed={picked}
                  aria-label={`Select ${l.kind === "add" ? "added" : "removed"} line ${l.kind === "add" ? l.new : l.old}`}
                  title="Select this line. Shift-click selects a range."
                  onMouseDown={(e) => { if (e.shiftKey) e.preventDefault(); }}
                  onClick={(e) => pick(e.shiftKey)}
                >
                  {picked && <Check size={11} strokeWidth={3} />}
                </button>
              )}
              <code>{l.kind === "add" ? "+" : l.kind === "del" ? "-" : " "}{l.text || " "}</code>
            </div>
          );
        })}
      </div>
    </>
  );
}

/**
 * Review the proposed split: edit each message, move hunks between commits, reorder, then commit
 * them in order. Stops at the first commit that fails; the ones before it stay.
 */
function SplitSheet({ workspaceId, plan, onClose }: { workspaceId: string; plan: SplitPlan; onClose: (done?: string) => void }) {
  const [groups, setGroups] = useState(plan.groups);
  const [running, setRunning] = useState<number | null>(null);
  const [made, setMade] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [peek, setPeek] = useState<number | null>(null);
  const blocker = planBlocker(groups);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && running === null) { e.stopPropagation(); onClose(made.length ? `Made ${made.length} ${made.length === 1 ? "commit" : "commits"}.` : undefined); } };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [running, made, onClose]);

  const go = async () => {
    if (blocker || running !== null) return;
    const protectedFiles = [...new Set(groups.flatMap((g) => g.hunks.map((h) => plan.hunks[h])).filter((h) => h.protected).map((h) => h.path))];
    if (!(await okToStage(protectedFiles))) return;
    setError(null);
    const shas = [...made];
    for (let i = made.length; i < groups.length; i++) {
      setRunning(i);
      try {
        const g = groups[i];
        shas.push(await api.splitCommit(workspaceId, g.message, g.hunks.map((h) => ({ path: plan.hunks[h].path, key: plan.hunks[h].key }))));
        setMade([...shas]);
      } catch (e) {
        setError(`Commit ${i + 1} failed: ${errText(e)}${i ? ` The ${i === 1 ? "one" : `${i}`} before it ${i === 1 ? "is" : "are"} committed.` : ""}`);
        setRunning(null);
        return;
      }
    }
    setRunning(null);
    onClose(`Made ${shas.length} ${shas.length === 1 ? "commit" : "commits"}.`);
  };

  const locked = running !== null || made.length > 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget && running === null) onClose(made.length ? `Made ${made.length} commits.` : undefined); }}>
      <div className="modal split-sheet" role="dialog" aria-modal="true" aria-label="Split into commits">
        <h2>Split into commits</h2>
        <p className="muted split-lead">
          {plan.note ?? `The agent proposed ${groups.length} ${groups.length === 1 ? "commit" : "commits"}. Check each message and which changes go in it.`}
          {" "}They are committed top to bottom. What you staged by hand is replaced; your files don't change.
        </p>
        <ol className="split-groups">
          {groups.map((g, gi) => {
            const hint = subjectHint(g.message);
            const done = gi < made.length;
            return (
              <li key={gi} className={`split-group${done ? " done" : ""}${running === gi ? " running" : ""}`}>
                <div className="split-group-head">
                  <span className="split-num num" aria-hidden>{done ? <Check size={12} strokeWidth={3} /> : running === gi ? <span className="spinner" /> : gi + 1}</span>
                  <textarea
                    value={g.message}
                    rows={Math.min(4, Math.max(1, g.message.split("\n").length))}
                    aria-label={`Message for commit ${gi + 1}`}
                    placeholder="type(scope): what this commit does"
                    disabled={locked}
                    onChange={(e) => setGroups(groups.map((x, k) => (k === gi ? { ...x, message: e.target.value } : x)))}
                  />
                  <button className="icon" aria-label={`Move commit ${gi + 1} up`} title="Move up" disabled={locked || gi === 0} onClick={() => setGroups(moveGroup(groups, gi, -1))}><ArrowUp size={13} /></button>
                  <button className="icon" aria-label={`Move commit ${gi + 1} down`} title="Move down" disabled={locked || gi === groups.length - 1} onClick={() => setGroups(moveGroup(groups, gi, 1))}><ArrowDown size={13} /></button>
                </div>
                {!done && <span className={`subject-hint t-${hint.tone}`}>{hint.tone === "empty" ? "Needs a message" : hint.text}</span>}
                {done && <span className="subject-hint t-ok">Committed {made[gi]}</span>}
                <ul className="split-hunks">
                  {g.hunks.map((h) => {
                    const x = plan.hunks[h];
                    return (
                      <li key={h}>
                        <div className="split-hunk">
                          <button className="link split-hunk-name" onClick={() => setPeek(peek === h ? null : h)} aria-expanded={peek === h} title={x.whole ? x.path : "Show this change"}>
                            <code>{x.path.split("/").pop()}</code>
                            <span className={`muted${x.whole ? " prose" : ""}`}>{x.whole ? x.header : x.header.replace(/^@@ .*? @@\s*/, "") || `line ${x.header.match(/\+(\d+)/)?.[1] ?? ""}`}</span>
                          </button>
                          {x.protected && <Lock size={12} className="stage-lock" aria-label="Protected" />}
                          {!x.whole && <span className="num"><span className="add">+{x.additions}</span> <span className="del">-{x.deletions}</span></span>}
                          <span className="spacer" />
                          <select
                            aria-label={`Move ${x.path} change to commit`}
                            value={gi}
                            disabled={locked}
                            onChange={(e) => setGroups(moveHunk(groups, h, +e.target.value))}
                          >
                            {groups.map((_, k) => <option key={k} value={k}>Commit {k + 1}</option>)}
                            <option value={groups.length}>New commit</option>
                          </select>
                        </div>
                        {peek === h && !x.whole && (
                          <pre className="split-peek" aria-label={`${x.path}, ${x.header}`}>
                            <span className="pk-hunk">{x.header}</span>
                            {x.preview.split("\n").map((ln, k) => <span key={k} className={ln.startsWith("+") ? "pk-add" : ln.startsWith("-") ? "pk-del" : undefined}>{"\n"}{ln}</span>)}
                          </pre>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </li>
            );
          })}
        </ol>
        {error && <div className="banner error" role="alert">{error}</div>}
        <div className="row">
          <span className="muted split-why">{blocker ?? ""}</span>
          <span className="spacer" />
          <button className="quiet" onClick={() => onClose(made.length ? `Made ${made.length} ${made.length === 1 ? "commit" : "commits"}.` : undefined)} disabled={running !== null}>{made.length ? "Close" : "Cancel"}</button>
          <button className="primary" onClick={go} disabled={!!blocker || running !== null} title={blocker ?? undefined}>
            {running !== null ? <span className="spinner" /> : <GitCommitHorizontal size={15} />}
            {made.length ? `Commit the other ${groups.length - made.length}` : `Commit ${groups.length} in order`}
          </button>
        </div>
      </div>
    </div>
  );
}
