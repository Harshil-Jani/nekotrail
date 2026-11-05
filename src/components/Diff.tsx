import { memo, useEffect, useState } from "react";
import { MessageSquarePlus, RefreshCw, Send, X } from "lucide-react";
import { api, errText, type FileChange } from "../api";
import { Mascot } from "./Mascot";

/** A review comment on one diff line. `line` is the new-file line, or the old one for deletions. */
export type ReviewComment = { id: string; file: string; line: number; side: "old" | "new"; code: string; text: string };

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
  onAdd,
  onRemove,
}: {
  file: string;
  text: string;
  comments: ReviewComment[];
  onAdd: (c: ReviewComment) => void;
  onRemove: (id: string) => void;
}) {
  const lines = parseDiff(text);
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  useEffect(() => { setEditing(null); setDraft(""); }, [file]);

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

/** Turn review comments into one message the agent can act on. */
export function reviewPrompt(comments: ReviewComment[]): string {
  const lines = comments.map((c, i) => `${i + 1}. ${c.file}:${c.line}${c.side === "old" ? " (removed line)" : ""}\n   \`${c.code.slice(0, 120)}\`\n   ${c.text}`);
  return `Please address these review comments on your changes:\n\n${lines.join("\n\n")}`;
}

export function Diff({ workspaceId, refreshKey, onSendReview }: { workspaceId: string; refreshKey: number; onSendReview: (text: string) => void }) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [diff, setDiff] = useState("");
  const [error, setError] = useState<string | null>(null);
  // Draft comments survive tab switches and restarts until they are sent.
  const key = `nekotrail.review.${workspaceId}`;
  const [comments, setComments] = useState<ReviewComment[]>(() => JSON.parse(localStorage.getItem(key) || "[]"));
  useEffect(() => setComments(JSON.parse(localStorage.getItem(key) || "[]")), [key]);
  const saveComments = (c: ReviewComment[]) => { setComments(c); localStorage.setItem(key, JSON.stringify(c)); };

  const load = () => {
    api.changes(workspaceId).then((f) => {
      setFiles(f);
      setError(null);
      setOpen((cur) => (cur && f.some((x) => x.path === cur) ? cur : f[0]?.path ?? null));
    }).catch((e) => setError(errText(e)));
  };

  useEffect(load, [workspaceId, refreshKey]);
  useEffect(() => {
    if (open) api.fileDiff(workspaceId, open).then(setDiff).catch((e) => setDiff(errText(e)));
    else setDiff("");
  }, [open, workspaceId, refreshKey]);

  if (error) return <div className="banner error">{error}</div>;
  if (!files) return <div className="empty"><span className="spinner" /></div>;
  if (files.length === 0)
    return (
      <div className="empty">
        <Mascot size={64} mood="sleep" />
        <h2>Nothing changed yet</h2>
        <p>Edits the agent makes in this workspace show up here, compared with the target branch.</p>
      </div>
    );

  const add = files.reduce((n, f) => n + f.additions, 0);
  const del = files.reduce((n, f) => n + f.deletions, 0);
  const fileComments = comments.filter((c) => c.file === open);
  return (
    <div className="diff-view">
      <div className="file-list">
        <div className="file-list-head">
          <span className="num">{files.length} {files.length === 1 ? "file" : "files"}</span> <span className="add">+{add}</span> <span className="del">-{del}</span>
          <span className="spacer" /><button className="icon" onClick={load} title="Refresh" aria-label="Refresh changes"><RefreshCw size={16} /></button>
        </div>
        {files.map((f) => {
          const n = comments.filter((c) => c.file === f.path).length;
          return (
            <button key={f.path} className={`file ${open === f.path ? "selected" : ""}`} onClick={() => setOpen(f.path)}>
              <span className={`status s-${f.status}`}>{f.status}</span>
              <span className="file-path" title={f.path}>{f.path}</span>
              {n > 0 && <span className="comment-count num" title={`${n} review comments`}>{n}</span>}
              <span className="add">+{f.additions}</span>
              <span className="del">-{f.deletions}</span>
            </button>
          );
        })}
        <div className="review-bar">
          {comments.length === 0 ? (
            <p className="muted">Hover a line and click the speech bubble to leave a comment. Send them all to the agent at once.</p>
          ) : (
            <>
              <button className="accent" onClick={() => { onSendReview(reviewPrompt(comments)); saveComments([]); }}>
                <Send size={14} /> Send {comments.length} {comments.length === 1 ? "comment" : "comments"} to agent
              </button>
              <button className="quiet" onClick={() => saveComments([])}>Discard</button>
            </>
          )}
        </div>
      </div>
      <div className="file-diff">
        {open && (
          <DiffLines
            file={open}
            text={diff}
            comments={fileComments}
            onAdd={(c) => saveComments([...comments, c])}
            onRemove={(id) => saveComments(comments.filter((c) => c.id !== id))}
          />
        )}
      </div>
    </div>
  );
}
