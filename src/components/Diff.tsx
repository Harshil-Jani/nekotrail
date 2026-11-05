import { memo, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, MessageSquarePlus, Pencil, RefreshCw, Save, Send, X } from "lucide-react";
import { Avatar } from "./Avatar";
import { api, errText, type FileChange } from "../api";
import { Mascot } from "./Mascot";

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
  const lines = parseDiff(text);
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  useEffect(() => { setEditing(null); setDraft(""); }, [file]);
  if (split) return <SplitLines lines={lines} file={file} />;

  const save = (l: Line) => {
    const t = draft.trim();
    if (t) {
      const side = l.kind === "del" ? "old" : "new";
      onAdd({ id: crypto.randomUUID(), file, line: (side === "old" ? l.old : l.new)!, side, code: l.text.trim(), text: t });
    }
    setEditing(null);
    setDraft("");
  };

  return (
    <div className="diff" role="table" aria-label={`Changes in ${file}`}>
      {lines.map((l, i) => {
        if (l.kind === "hunk") return <div key={i} className="dl hunk"><span className="ln" /><span className="ln" /><code>{l.text}</code></div>;
        const side = l.kind === "del" ? "old" : "new";
        const num = side === "old" ? l.old : l.new;
        const here = comments.filter((c) => c.side === side && c.line === num);
        const ghHere = gh.filter((c) => c.line === num && (c.side === "LEFT") === (side === "old"));
        return (
          <div key={i}>
            <div className={`dl ${l.kind}`}>
              <span className="ln">{l.old ?? ""}</span>
              <span className="ln">{l.new ?? ""}</span>
              <button className="add-comment" aria-label={`Comment on line ${num}`} onClick={() => { setEditing(i); setDraft(""); }}>
                <MessageSquarePlus size={13} />
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
            {editing === i && (
              <div className="review-edit">
                <textarea
                  autoFocus
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  placeholder="What should change here? ⌘↵ to add, Esc to cancel"
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); save(l); }
                    if (e.key === "Escape") { e.stopPropagation(); setEditing(null); }
                  }}
                />
                <div className="row">
                  <span className="spacer" />
                  <button className="quiet" onClick={() => setEditing(null)}>Cancel</button>
                  <button className="primary" onClick={() => save(l)} disabled={!draft.trim()}>Add comment</button>
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
});

/** Side by side: old on the left, new on the right; removed and added runs are paired up. */
function SplitLines({ lines, file }: { lines: Line[]; file: string }) {
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
}

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
        <button className="quiet" onClick={() => { if (!dirty || confirm("Discard unsaved edits to this file?")) onDone(); }}>Done</button>
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
    }).catch((e) => setError(errText(e)));
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

  // ↑/↓ move between files, ⌘⌥U switches all changes / uncommitted.
  const nav = useRef({ files, open });
  nav.current = { files, open };
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

  if (error) return <div className="banner error">{error}</div>;
  if (!files) return <div className="empty"><span className="spinner" /></div>;

  const add = files.reduce((n, f) => n + f.additions, 0);
  const del = files.reduce((n, f) => n + f.deletions, 0);
  const fileComments = comments.filter((c) => c.file === open);
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
        {files.length === 0 && (
          <p className="muted file-list-empty">{uncommitted ? "No uncommitted changes. Everything is committed." : "Nothing changed yet. Edits the agent makes show up here, compared with the target branch."}</p>
        )}
        {files.map((f) => {
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
            </div>
            <DiffLines
              file={open}
              text={diff}
              comments={fileComments}
              gh={gh.filter((c) => c.path === open)}
              split={split}
              onAdd={(c) => saveComments([...comments, c])}
              onRemove={(id) => saveComments(comments.filter((c) => c.id !== id))}
            />
          </>
        ) : (
          <div className="empty"><Mascot size={64} mood="sleep" /><h2>No file selected</h2><p>Pick a file from the list, or press ⌘P to open any file.</p></div>
        )}
      </div>
    </div>
  );
}
