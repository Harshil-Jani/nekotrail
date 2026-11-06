import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, MessageSquarePlus, Pencil, RefreshCw, Save, Search, Send, Undo2, X } from "lucide-react";
import { Avatar } from "./Avatar";
import { api, errText, type FileChange } from "../api";
import { Mascot } from "./Mascot";
import { ask } from "./Confirm";

/** A review comment on one diff line. `line` is the new-file line, or the old one for deletions. */
export type ReviewComment = { id: string; file: string; line: number; side: "old" | "new"; code: string; text: string };

/** A line comment from the PR on GitHub. */
export type GhComment = { id: number; path: string; line: number | null; side: "LEFT" | "RIGHT"; body: string; user: string; avatar: string; url: string; reply_to: number | null; outdated: boolean };

type Line = { kind: "hunk" | "add" | "del" | "ctx"; text: string; old?: number; new?: number };

/** Parse a unified diff into lines that know their old and new line numbers. */
export function parseDiff(text: string): Line[] {
  const out: Line[] = [];
  let o = 0, n = 0, inHunk = false;
  for (const l of text.split("\n")) {
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(l);
    if (h) {
      o = +h[1]; n = +h[2]; inHunk = true;
      out.push({ kind: "hunk", text: l });
    } else if (!inHunk) {
      continue; // file headers
    } else if (l.startsWith("+")) {
      out.push({ kind: "add", text: l.slice(1), new: n++ });
    } else if (l.startsWith("-")) {
      out.push({ kind: "del", text: l.slice(1), old: o++ });
    } else if (l.startsWith("\\")) {
      continue; // "\ No newline at end of file"
    } else if (l !== "" || out.length) {
      out.push({ kind: "ctx", text: l.slice(1), old: o++, new: n++ });
    }
  }
  while (out.length && out[out.length - 1].kind === "ctx" && out[out.length - 1].text === "") out.pop();
  return out;
}

const NONE: never[] = [];
const COMMENT_ICON = <MessageSquarePlus size={13} />;

/** The box for writing one review comment. Its draft lives here, so typing redraws only this box. */
function ReviewEdit({ onSave, onCancel }: { onSave: (text: string) => void; onCancel: () => void }) {
  const [draft, setDraft] = useState("");
  return (
    <div className="review-edit">
      <textarea
        autoFocus
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        placeholder="What should change here? ⌘↵ to add, Esc to cancel"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); onSave(draft); }
          if (e.key === "Escape") { e.stopPropagation(); onCancel(); }
        }}
      />
      <div className="row">
        <span className="spacer" />
        <button className="quiet" onClick={onCancel}>Cancel</button>
        <button className="primary" onClick={() => onSave(draft)} disabled={!draft.trim()}>Add comment</button>
      </div>
    </div>
  );
}

/** One diff line with its comments. Memoized: big diffs redraw only the lines that changed. */
const DiffRow = memo(function DiffRow({ l, i, here, ghHere, editing, onEdit, onSave, onRemove }: {
  l: Line;
  i: number;
  here: ReviewComment[];
  ghHere: GhComment[];
  editing: boolean;
  onEdit: (i: number | null) => void;
  onSave: (l: Line, text: string) => void;
  onRemove: (id: string) => void;
}) {
  if (l.kind === "hunk") return <div className="dl hunk"><span className="ln" /><span className="ln" /><code>{l.text}</code></div>;
  const num = l.kind === "del" ? l.old : l.new;
  return (
    <div>
      <div className={`dl ${l.kind}`}>
        <span className="ln">{l.old ?? ""}</span>
        <span className="ln">{l.new ?? ""}</span>
        <button className="add-comment" aria-label={`Comment on line ${num}`} onClick={() => onEdit(i)}>
          {COMMENT_ICON}
        </button>
        <code>{l.kind === "add" ? "+" : l.kind === "del" ? "-" : " "}{l.text || " "}</code>
      </div>
      {ghHere.map((c) => (
        <div key={c.id} className={`gh-comment ${c.reply_to ? "reply" : ""}`}>
          <Avatar src={c.avatar} size={18} label={c.user} />
          <div>
            <b>@{c.user}</b>
            <span className="gh-body">{c.body}</span>
          </div>
          <button className="icon" aria-label="Open on GitHub" title="Open on GitHub" onClick={() => openUrl(c.url)}><ExternalLink size={13} /></button>
        </div>
      ))}
      {here.map((c) => (
        <div key={c.id} className="review-comment">
          <span>{c.text}</span>
          <button className="icon" aria-label="Delete comment" onClick={() => onRemove(c.id)}><X size={13} /></button>
        </div>
      ))}
      {editing && <ReviewEdit onSave={(t) => onSave(l, t)} onCancel={() => onEdit(null)} />}
    </div>
  );
});

const DiffLines = memo(function DiffLines({
  file,
  text,
  comments,
  gh,
  split,
  onAdd,
  onRemove,
}: {
  file: string;
  text: string;
  comments: ReviewComment[];
  gh: GhComment[];
  onAdd: (c: ReviewComment) => void;
  onRemove: (id: string) => void;
  split: boolean;
}) {
  const lines = useMemo(() => parseDiff(text), [text]);
  const [editing, setEditing] = useState<number | null>(null);
  useEffect(() => { setEditing(null); }, [file]);
  // Comments by side and line number, so each row looks up its own instead of filtering the list.
  const byLine = useMemo(() => {
    const ours = new Map<string, ReviewComment[]>();
    for (const c of comments) { const k = `${c.side}:${c.line}`; ours.set(k, [...(ours.get(k) ?? []), c]); }
    const theirs = new Map<string, GhComment[]>();
    for (const c of gh) { const k = `${c.side === "LEFT" ? "old" : "new"}:${c.line}`; theirs.set(k, [...(theirs.get(k) ?? []), c]); }
    return { ours, theirs };
  }, [comments, gh]);
  // Stable callbacks keep the memoized rows from redrawing when the parent does.
  const cb = useRef({ onAdd, onRemove });
  cb.current = { onAdd, onRemove };
  const save = useCallback((l: Line, draft: string) => {
    const t = draft.trim();
    if (t) {
      const side = l.kind === "del" ? "old" : "new";
      cb.current.onAdd({ id: crypto.randomUUID(), file, line: (side === "old" ? l.old : l.new)!, side, code: l.text.trim(), text: t });
    }
    setEditing(null);
  }, [file]);
  const remove = useCallback((id: string) => cb.current.onRemove(id), []);
  if (split) return <SplitLines lines={lines} file={file} />;

  return (
    <div className="diff" role="table" aria-label={`Changes in ${file}`}>
      {lines.map((l, i) => {
        const k = `${l.kind === "del" ? "old" : "new"}:${l.kind === "del" ? l.old : l.new}`;
        return (
          <DiffRow
            key={i} l={l} i={i}
            here={l.kind === "hunk" ? NONE : byLine.ours.get(k) ?? NONE}
            ghHere={l.kind === "hunk" ? NONE : byLine.theirs.get(k) ?? NONE}
            editing={editing === i} onEdit={setEditing} onSave={save} onRemove={remove}
          />
        );
      })}
    </div>
  );
});

/** Side by side: old on the left, new on the right; removed and added runs are paired up. */
const SplitLines = memo(function SplitLines({ lines, file }: { lines: Line[]; file: string }) {
  type Cell = Line | null;
  const rows: [Cell, Cell][] = [];
  for (let i = 0; i < lines.length; ) {
    const l = lines[i];
    if (l.kind === "hunk" || l.kind === "ctx") { rows.push([l, l]); i++; continue; }
    const dels: Line[] = [], adds: Line[] = [];
    while (i < lines.length && lines[i].kind === "del") dels.push(lines[i++]);
    while (i < lines.length && lines[i].kind === "add") adds.push(lines[i++]);
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) rows.push([dels[k] ?? null, adds[k] ?? null]);
  }
  const cell = (c: Cell, side: "old" | "new") =>
    !c ? <div className="sl empty-cell" /> :
    c.kind === "hunk" ? <div className="sl hunk"><code>{c.text}</code></div> :
    <div className={`sl ${c.kind}`}><span className="ln">{side === "old" ? c.old : c.new}</span><code>{c.text || " "}</code></div>;
  return (
    <div className="diff split" role="table" aria-label={`Changes in ${file}, side by side`}>
      {rows.map(([a, b], i) => (
        a?.kind === "hunk" ? <div key={i} className="split-row full">{cell(a, "old")}</div> :
        <div key={i} className="split-row">{cell(a, "old")}{cell(b, "new")}</div>
      ))}
    </div>
  );
});

/** Turn review comments into one message the agent can act on. */
export function reviewPrompt(comments: ReviewComment[]): string {
  const lines = comments.map((c, i) => `${i + 1}. ${c.file}:${c.line}${c.side === "old" ? " (removed line)" : ""}${c.code ? `\n   \`${c.code.slice(0, 120)}\`` : ""}\n   ${c.text}`);
  return `Address these review comments on your changes:\n\n${lines.join("\n\n")}`;
}

/** Turn GitHub review comments into one message for the agent. */
export function githubPrompt(comments: GhComment[]): string {
  const top = comments.filter((c) => !c.reply_to);
  const lines = top.map((c, i) => {
    const replies = comments.filter((r) => r.reply_to === c.id).map((r) => `   @${r.user}: ${r.body}`);
    return [`${i + 1}. ${c.path}${c.line ? `:${c.line}` : ""} (@${c.user}${c.outdated ? ", on an older version" : ""})`, `   ${c.body}`, ...replies].join("\n");
  });
  return `Address these review comments from the PR on GitHub. Fix what's right, and tell me which ones you disagree with and why:\n\n${lines.join("\n\n")}`;
}

/** Editor for one file: plain, fast, monospace. ⌘S saves. */
function FileEditor({ workspaceId, path, onDone }: { workspaceId: string; path: string; onDone: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const [saved, setSaved] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setText(null);
    api.readFile(workspaceId, path).then((t) => { setText(t); setSaved(t); }).catch((e) => setError(errText(e)));
  }, [workspaceId, path]);
  const dirty = text !== null && text !== saved;
  const save = async () => {
    if (text === null) return;
    try { await api.writeFile(workspaceId, path, text); setSaved(text); } catch (e) { setError(errText(e)); }
  };
  return (
    <div className="file-editor">
      <div className="file-head">
        <code>{path}</code>{dirty && <span className="muted">· edited</span>}
        <span className="spacer" />
        <button onClick={save} disabled={!dirty} title="Save file (⌘S)"><Save size={14} />Save</button>
        <button className="quiet" onClick={async () => { if (!dirty || await ask({ title: "Discard your edits?", body: `Unsaved changes to ${path} are lost.`, ok: "Discard edits", danger: true })) onDone(); }}>Done</button>
      </div>
      {error && <div className="banner error">{error}</div>}
      {text === null && !error ? <div className="empty"><span className="spinner" /></div> : (
        <textarea
          className="code-edit"
          spellCheck={false}
          value={text ?? ""}
          aria-label={`Edit ${path}`}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.metaKey && e.key === "s") { e.preventDefault(); save(); }
            if (e.key === "Tab") {
              e.preventDefault();
              const el = e.currentTarget, a = el.selectionStart;
              setText(el.value.slice(0, a) + "  " + el.value.slice(el.selectionEnd));
              requestAnimationFrame(() => el.setSelectionRange(a + 2, a + 2));
            }
          }}
        />
      )}
    </div>
  );
}

export function Diff({
  workspaceId,
  refreshKey,
  onSendReview,
  focusFile,
}: {
  workspaceId: string;
  refreshKey: number;
  onSendReview: (text: string) => void;
  /** Open this file in the editor (quick open, ⌘P). */
  focusFile?: { path: string; nonce: number };
}) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [diff, setDiff] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [uncommitted, setUncommitted] = useState(false);
  const [split, setSplit] = useState(() => localStorage.getItem("nekotrail.split") === "1");
  const toggleSplit = () => setSplit((v) => { localStorage.setItem("nekotrail.split", v ? "0" : "1"); return !v; });
  const [editing, setEditing] = useState<string | null>(null);
  const [gh, setGh] = useState<GhComment[]>([]);
  // Files with uncommitted edits: only these can be discarded (committed work never is).
  const [dirty, setDirty] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [discarding, setDiscarding] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => setNotice(null), [workspaceId]);
  // Draft comments survive tab switches and restarts until they are sent.
  const key = `nekotrail.review.${workspaceId}`;
  const [comments, setComments] = useState<ReviewComment[]>(() => JSON.parse(localStorage.getItem(key) || "[]"));
  useEffect(() => {
    const load = () => setComments(JSON.parse(localStorage.getItem(key) || "[]"));
    load();
    window.addEventListener("nekotrail-review", load);
    return () => window.removeEventListener("nekotrail-review", load);
  }, [key]);
  const saveComments = (c: ReviewComment[]) => { setComments(c); localStorage.setItem(key, JSON.stringify(c)); };
  // Stable handlers for the diff, so a re-render here (a turn ending elsewhere) skips its 5,000 rows.
  const latest = useRef({ comments, saveComments });
  latest.current = { comments, saveComments };
  const addComment = useCallback((c: ReviewComment) => latest.current.saveComments([...latest.current.comments, c]), []);
  const removeComment = useCallback((id: string) => latest.current.saveComments(latest.current.comments.filter((c) => c.id !== id)), []);
  // "Viewed" ticks reset when a file changes again (its +/- counts move).
  const vkey = `nekotrail.viewed.${workspaceId}`;
  const [viewed, setViewed] = useState<Record<string, string>>(() => JSON.parse(localStorage.getItem(vkey) || "{}"));
  useEffect(() => setViewed(JSON.parse(localStorage.getItem(vkey) || "{}")), [vkey]);
  const sig = (f: FileChange) => `${f.additions}/${f.deletions}`;
  const toggleViewed = (f: FileChange) => {
    const next = { ...viewed };
    if (next[f.path] === sig(f)) delete next[f.path]; else next[f.path] = sig(f);
    setViewed(next);
    localStorage.setItem(vkey, JSON.stringify(next));
  };

  const load = () => {
    api.changes(workspaceId, uncommitted).then((f) => {
      setFiles(f);
      setError(null);
      setOpen((cur) => (cur && f.some((x) => x.path === cur) ? cur : f[0]?.path ?? null));
      if (uncommitted) setDirty(new Set(f.map((x) => x.path)));
    }).catch((e) => setError(errText(e)));
    if (!uncommitted) api.changes(workspaceId, true).then((f) => setDirty(new Set(f.map((x) => x.path)))).catch(() => setDirty(new Set()));
  };
  useEffect(() => setQuery(""), [workspaceId]);

  /** Discard one file's uncommitted changes, after asking. Its current copy goes to the Trash. */
  const discard = async (path: string) => {
    const f = files?.find((x) => x.path === path);
    const isNew = f?.status === "A" || f?.status === "?";
    const ok = await ask({
      title: `Discard changes to ${path.split("/").pop()}?`,
      body: isNew
        ? `${path} is a new file. It moves to the Trash, so you can still get it back from there. No other file is touched.`
        : `${path} goes back to its last commit. The edited copy moves to the Trash, so you can still get it back from there. Committed changes and other files stay as they are.`,
      ok: "Discard changes",
      danger: true,
    });
    if (!ok) return;
    setNotice(null);
    setDiscarding(path);
    try {
      await api.discardFile(workspaceId, path);
      setEditing((cur) => (cur === path ? null : cur));
    } catch (err) {
      setNotice(`Couldn't discard ${path}: ${errText(err)}`);
    } finally {
      setDiscarding(null);
      load();
    }
  };

  useEffect(load, [workspaceId, refreshKey, uncommitted]);
  useEffect(() => {
    api.prReviewComments(workspaceId).then(setGh).catch(() => setGh([]));
  }, [workspaceId, refreshKey]);
  useEffect(() => {
    if (open) api.fileDiff(workspaceId, open, uncommitted).then(setDiff).catch((e) => setDiff(errText(e)));
    else setDiff("");
  }, [open, workspaceId, refreshKey, uncommitted]);
  useEffect(() => { if (focusFile) setEditing(focusFile.path); }, [focusFile?.nonce]);

  // The list as filtered by the box above it; ↑/↓ move through what you can see.
  const q = query.trim().toLowerCase();
  const shown = useMemo(() => (files && q ? files.filter((f) => f.path.toLowerCase().includes(q)) : files ?? []), [files, q]);

  // ↑/↓ move between files, ⌘⌥U switches all changes / uncommitted.
  const nav = useRef({ files: shown, open });
  nav.current = { files: shown, open };
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (root.current?.closest(".pane")?.hasAttribute("hidden")) return;
      if (e.metaKey && e.altKey && e.code === "KeyU") { e.preventDefault(); setUncommitted((u) => !u); return; }
      const t = e.target as HTMLElement;
      if (t.closest("textarea, input, select") || e.metaKey || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
      const { files: fs, open: cur } = nav.current;
      if (!fs?.length) return;
      e.preventDefault();
      const i = fs.findIndex((f) => f.path === cur);
      const next = fs[Math.min(fs.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
      setOpen(next.path);
      setEditing(null);
      root.current?.querySelector(`[data-file="${CSS.escape(next.path)}"]`)?.scrollIntoView({ block: "nearest" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Same arrays between renders, so the memoized diff below skips work when nothing it shows changed.
  const fileComments = useMemo(() => comments.filter((c) => c.file === open), [comments, open]);
  const fileGh = useMemo(() => gh.filter((c) => c.path === open), [gh, open]);

  if (error) return <div className="banner error">{error}</div>;
  if (!files) return <div className="empty"><span className="spinner" /></div>;

  const add = files.reduce((n, f) => n + f.additions, 0);
  const del = files.reduce((n, f) => n + f.deletions, 0);
  const ghTop = gh.filter((c) => !c.reply_to);
  const seen = files.filter((f) => viewed[f.path] === sig(f)).length;
  const scope = (
    <span className="seg scope" role="tablist" aria-label="Which changes">
      <button role="tab" aria-selected={!uncommitted} className={!uncommitted ? "on" : ""} onClick={() => setUncommitted(false)}>All</button>
      <button role="tab" aria-selected={uncommitted} className={uncommitted ? "on" : ""} onClick={() => setUncommitted(true)} title="Show uncommitted changes only (⌘⌥U)">Uncommitted</button>
    </span>
  );

  return (
    <div className="diff-view" ref={root}>
      <div className="file-list">
        <div className="file-list-head">
          {scope}
          <span className="spacer" /><button className="icon" onClick={load} title="Refresh changes" aria-label="Refresh changes"><RefreshCw size={15} /></button>
        </div>
        <div className="file-list-sum num">
          {files.length} {files.length === 1 ? "file" : "files"} <span className="add">+{add}</span> <span className="del">-{del}</span>
          {files.length > 0 && <span className="muted"> · {seen} viewed</span>}
        </div>
        {notice && <div className="banner error file-notice" role="alert">{notice}</div>}
        {(files.length > 7 || query) && (
          <label className="search file-filter">
            <Search size={14} />
            <input
              placeholder="Filter files"
              value={query}
              aria-label="Filter changed files"
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Escape" && query) { e.stopPropagation(); setQuery(""); } }}
            />
          </label>
        )}
        {files.length === 0 && (
          <p className="muted file-list-empty">{uncommitted ? "No uncommitted changes. Everything is committed." : "Nothing changed yet. Edits the agent makes show up here, compared with the target branch."}</p>
        )}
        {files.length > 0 && shown.length === 0 && (
          <p className="muted file-list-empty">No changed file matches "{query.trim()}".</p>
        )}
        {shown.map((f) => {
          const n = comments.filter((c) => c.file === f.path).length + gh.filter((c) => c.path === f.path && !c.reply_to).length;
          const isViewed = viewed[f.path] === sig(f);
          return (
            <div key={f.path} data-file={f.path} className={`file ${open === f.path ? "selected" : ""} ${isViewed ? "viewed" : ""}`}>
              <input type="checkbox" checked={isViewed} onChange={() => toggleViewed(f)} aria-label={`Mark ${f.path} as viewed`} title="Mark as viewed" />
              <button onClick={() => { setOpen(f.path); setEditing(null); }}>
                <span className={`status s-${f.status}`}>{f.status}</span>
                <span className="file-path" title={f.path}>{f.path}</span>
                {n > 0 && <span className="comment-count num" title={`${n} ${n === 1 ? "comment" : "comments"}`}>{n}</span>}
                <span className="add">+{f.additions}</span>
                <span className="del">-{f.deletions}</span>
              </button>
            </div>
          );
        })}
        <div className="review-bar">
          {ghTop.length > 0 && (
            <button onClick={() => onSendReview(githubPrompt(gh))}>
              <Send size={14} /> Send {ghTop.length} GitHub {ghTop.length === 1 ? "comment" : "comments"} to agent
            </button>
          )}
          {comments.length === 0 ? (
            <p className="muted">Hover a line and click the bubble to comment. ↑/↓ moves between files.</p>
          ) : (
            <>
              <button className="accent" onClick={() => { onSendReview(reviewPrompt(comments)); saveComments([]); }}>
                <Send size={14} /> Send {comments.length} {comments.length === 1 ? "comment" : "comments"} to agent
              </button>
              <button className="quiet" onClick={() => saveComments([])}>Discard comments</button>
            </>
          )}
        </div>
      </div>
      <div className="file-diff">
        {editing ? (
          <FileEditor workspaceId={workspaceId} path={editing} onDone={() => { setEditing(null); load(); }} />
        ) : open ? (
          <>
            <div className="file-head">
              <code>{open}</code>
              <span className="spacer" />
              <button className="quiet" onClick={toggleSplit} title="Switch diff layout">{split ? "Show unified" : "Show side by side"}</button>
              <button className="quiet" onClick={() => setEditing(open)}><Pencil size={13} />Edit file</button>
              <button
                className="quiet"
                onClick={() => discard(open)}
                disabled={!dirty.has(open) || discarding !== null}
                title={dirty.has(open) ? "Discard the uncommitted changes to this file" : "Only uncommitted changes can be discarded. This file's changes are committed."}
              >
                {discarding === open ? <span className="spinner" /> : <Undo2 size={13} />}Discard
              </button>
            </div>
            <DiffLines
              file={open}
              text={diff}
              comments={fileComments}
              gh={fileGh}
              split={split}
              onAdd={addComment}
              onRemove={removeComment}
            />
          </>
        ) : (
          <div className="empty"><Mascot size={64} mood="sleep" /><h2>No file selected</h2><p>Pick a file from the list, or press ⌘P to open any file.</p></div>
        )}
      </div>
    </div>
  );
}
