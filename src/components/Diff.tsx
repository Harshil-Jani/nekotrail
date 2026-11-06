import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronDown, ExternalLink, MessageSquarePlus, Pencil, RefreshCw, Save, Search, Send, Undo2, X } from "lucide-react";
import { Avatar } from "./Avatar";
import { api, errText, type FileChange } from "../api";
import { keyHint, mod } from "../platform";
import { Mascot } from "./Mascot";
import { ask } from "./Confirm";

/**
 * A draft review comment on lines `line` to `end_line` of one file. `side` is "old" only when every
 * commented line was removed. `code` holds the commented diff lines with their +, - or space marker,
 * which is how a comment finds its lines again, and knows it is outdated when the agent changed them.
 * Comments from a review finding have no code and stay on their line number.
 */
export type ReviewComment = { id: string; file: string; side: "old" | "new"; line: number; end_line: number; code: string; text: string };

/** A line comment from the PR on GitHub. */
export type GhComment = { id: number; path: string; line: number | null; side: "LEFT" | "RIGHT"; body: string; user: string; avatar: string; url: string; reply_to: number | null; outdated: boolean };

export type Line = { kind: "hunk" | "add" | "del" | "ctx"; text: string; old?: number; new?: number };

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

/** A diff line as it is quoted in a comment: its marker and its text. */
const mark = (l: Line) => (l.kind === "add" ? "+" : l.kind === "del" ? "-" : " ") + l.text;

/** Which side and line numbers a run of diff lines covers. */
export function spanOf(rows: Line[]): { side: "old" | "new"; line: number; end_line: number } | null {
  const body = rows.filter((l) => l.kind !== "hunk");
  if (!body.length) return null;
  const side = body.every((l) => l.kind === "del") ? "old" : "new";
  const nums = side === "old" ? body.map((l) => l.old!) : body.flatMap((l) => (l.new === undefined ? [] : [l.new]));
  return { side, line: nums[0], end_line: nums[nums.length - 1] };
}

/** "line 21" or "lines 20 to 22", for labels. */
const linesLabel = (c: { line: number; end_line: number }) => (c.end_line > c.line ? `lines ${c.line} to ${c.end_line}` : `line ${c.line}`);
/** `path:21` or `path:20-22`, for the message to the agent. */
const where = (c: ReviewComment) => `${c.file}:${c.line}${c.end_line > c.line ? `-${c.end_line}` : ""}`;

/**
 * Find a comment's lines in a parsed diff. Returns the index of its last line, or -1 when its quoted
 * lines are gone (the agent changed them). Lines that only moved are found again, nearest first.
 */
export function locate(lines: Line[], c: ReviewComment): number {
  if (!c.code) {
    return lines.findIndex((l) => (c.side === "old" ? l.kind === "del" && l.old === c.end_line : l.kind !== "hunk" && l.kind !== "del" && l.new === c.end_line));
  }
  const want = c.code.split("\n");
  let best = -1, dist = Infinity;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].kind === "hunk" || mark(lines[i]) !== want[0]) continue;
    let j = i, k = 0;
    while (j < lines.length && k < want.length) {
      if (lines[j].kind === "hunk") { j++; continue; }
      if (mark(lines[j]) !== want[k]) break;
      j++; k++;
    }
    if (k < want.length) continue;
    const s = spanOf([lines[i]]);
    const d = Math.abs((s?.line ?? 0) - c.line);
    if (d < dist) {
      dist = d;
      best = j - 1;
      while (lines[best].kind === "hunk") best--;
    }
    if (d === 0) break;
  }
  return best;
}

const NONE: never[] = [];
const COMMENT_ICON = <MessageSquarePlus size={13} />;

/** The box for writing one review comment. Its draft lives here, so typing redraws only this box. */
function ReviewEdit({ label, draft, onSave, onCancel }: {
  label: string;
  /** Kept outside, so the text survives when shift-click moves the box to a longer range. */
  draft: { current: string };
  onSave: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState(draft.current);
  return (
    <div className="review-edit" data-review-edit>
      <span className="review-where">Comment on {label}</span>
      <textarea
        autoFocus
        value={text}
        aria-label={`Comment on ${label}`}
        onChange={(e) => { setText(e.target.value); draft.current = e.target.value; }}
        placeholder="What should change here?"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); onSave(text); }
          if (e.key === "Escape") { e.stopPropagation(); onCancel(); }
        }}
      />
      <div className="row">
        <span className="muted review-keys">{keyHint("⌘↵ to add, Esc to cancel")}</span>
        <span className="spacer" />
        <button className="quiet" onClick={onCancel}>Cancel</button>
        <button className="primary" onClick={() => onSave(text)} disabled={!text.trim()}>Add comment</button>
      </div>
    </div>
  );
}

/** Our draft comment, shown under the last line it covers. */
function CommentCard({ c, onRemove }: { c: ReviewComment; onRemove: (id: string) => void }) {
  return (
    <div className="review-comment" data-comment={c.id}>
      <div className="review-comment-body">
        {c.end_line > c.line && <span className="review-where">On {linesLabel(c)}</span>}
        <span className="review-text">{c.text}</span>
      </div>
      <button className="icon" aria-label="Delete comment" title="Delete comment" onClick={() => onRemove(c.id)}><X size={13} /></button>
    </div>
  );
}

/** What hangs under a diff line: GitHub comments, our drafts, and the comment box when open. */
function Under({ gh, ours, box, draft, onSave, onCancel, onRemove }: {
  gh: GhComment[];
  ours: ReviewComment[];
  box: string | null;
  draft: { current: string };
  onSave: (text: string) => void;
  onCancel: () => void;
  onRemove: (id: string) => void;
}) {
  return (
    <>
      {gh.map((c) => (
        <div key={c.id} className={`gh-comment ${c.reply_to ? "reply" : ""}`}>
          <Avatar src={c.avatar} size={18} label={c.user} />
          <div>
            <b>@{c.user}</b>
            <span className="gh-body">{c.body}</span>
          </div>
          <button className="icon" aria-label="Open on GitHub" title="Open on GitHub" onClick={() => openUrl(c.url)}><ExternalLink size={13} /></button>
        </div>
      ))}
      {ours.map((c) => <CommentCard key={c.id} c={c} onRemove={onRemove} />)}
      {box !== null && <ReviewEdit label={box} draft={draft} onSave={onSave} onCancel={onCancel} />}
    </>
  );
}

/** Handlers shared by every row. One stable object, so rows redraw only when their own props change. */
type RowActions = {
  /** Mouse down on a line number: start a selection, or extend it with shift. */
  down: (i: number, shift: boolean) => void;
  /** The comment button (keyboard friendly): comment on this line, or extend with shift. */
  pick: (i: number, shift: boolean) => void;
  /** The pointer moved over a line: extends the selection while dragging. */
  enter: (i: number) => void;
  save: (text: string) => void;
  cancel: () => void;
  remove: (id: string) => void;
  draft: { current: string };
};

const GUTTER_TIP = "Click to comment. Shift-click or drag for more lines.";

/** One diff line with its comments. Memoized: big diffs redraw only the lines that changed. */
const DiffRow = memo(function DiffRow({ l, i, here, ghHere, selected, box, act }: {
  l: Line;
  i: number;
  here: ReviewComment[];
  ghHere: GhComment[];
  selected: boolean;
  box: string | null;
  act: RowActions;
}) {
  if (l.kind === "hunk") return <div className="dl hunk"><span className="ln" /><span className="ln" /><code>{l.text}</code></div>;
  const num = l.kind === "del" ? l.old : l.new;
  const gutter = (n?: number) => (
    <span className="ln pick" title={GUTTER_TIP} onMouseDown={(e) => { if (e.button === 0) { e.preventDefault(); act.down(i, e.shiftKey); } }}>{n ?? ""}</span>
  );
  return (
    <div>
      <div className={`dl ${l.kind}${selected ? " sel" : ""}`} onMouseEnter={() => act.enter(i)}>
        {gutter(l.old)}
        {gutter(l.new)}
        <button className="add-comment" aria-label={`Comment on line ${num}`} title={GUTTER_TIP} onClick={(e) => act.pick(i, e.shiftKey)}>
          {COMMENT_ICON}
        </button>
        <code>{l.kind === "add" ? "+" : l.kind === "del" ? "-" : " "}{l.text || " "}</code>
      </div>
      <Under gh={ghHere} ours={here} box={box} draft={act.draft} onSave={act.save} onCancel={act.cancel} onRemove={act.remove} />
    </div>
  );
});

/** Side by side row: old on the left, new on the right, comments under both. */
const SplitRow = memo(function SplitRow({ a, b, ia, ib, here, ghHere, selA, selB, box, act }: {
  a: Line | null;
  b: Line | null;
  ia: number;
  ib: number;
  here: ReviewComment[];
  ghHere: GhComment[];
  selA: boolean;
  selB: boolean;
  box: string | null;
  act: RowActions;
}) {
  if (a?.kind === "hunk") return <div className="split-row full"><div className="sl hunk"><code>{a.text}</code></div></div>;
  const cell = (c: Line | null, i: number, side: "old" | "new", sel: boolean) =>
    !c ? <div className="sl empty-cell" /> : (
      <div className={`sl ${c.kind}${sel ? " sel" : ""}`} onMouseEnter={() => act.enter(i)}>
        <span className="ln pick" title={GUTTER_TIP} onMouseDown={(e) => { if (e.button === 0) { e.preventDefault(); act.down(i, e.shiftKey); } }}>
          {side === "old" ? c.old : c.new}
        </span>
        <code>{c.text || " "}</code>
      </div>
    );
  return (
    <div>
      <div className="split-row">{cell(a, ia, "old", selA)}{cell(b, ib, "new", selB)}</div>
      <Under gh={ghHere} ours={here} box={box} draft={act.draft} onSave={act.save} onCancel={act.cancel} onRemove={act.remove} />
    </div>
  );
});

type Sel = { a: number; b: number };

const DiffLines = memo(function DiffLines({
  file,
  text,
  comments,
  gh,
  outdated,
  split,
  onAdd,
  onRemove,
}: {
  file: string;
  text: string;
  comments: ReviewComment[];
  gh: GhComment[];
  outdated: Set<string>;
  onAdd: (c: ReviewComment) => void;
  onRemove: (ids: string[]) => void;
  split: boolean;
}) {
  const lines = useMemo(() => parseDiff(text), [text]);

  // The selected lines (indexes into `lines`, a is where it started) and whether a drag is going on.
  const [sel, setSel] = useState<Sel | null>(null);
  const [dragging, setDragging] = useState(false);
  const draft = useRef("");
  const live = useRef({ sel, lines, file, dragging: false, onAdd, onRemove });
  live.current = { ...live.current, sel, lines, file, onAdd, onRemove };
  useEffect(() => { setSel(null); draft.current = ""; }, [file]);
  // New text for the same file (the agent's turn ended): keep the open box on the same code if it is still there.
  const prevLines = useRef(lines);
  useEffect(() => {
    const before = prevLines.current;
    prevLines.current = lines;
    setSel((s) => {
      if (!s || before === lines) return s;
      const lo = Math.min(s.a, s.b), hi = Math.max(s.a, s.b);
      const rows = before.slice(lo, hi + 1);
      const span = spanOf(rows);
      if (!span) return null;
      const end = locate(lines, { id: "", file: "", ...span, code: rows.filter((l) => l.kind !== "hunk").map(mark).join("\n"), text: "" });
      return end < 0 ? null : { a: end - (hi - lo), b: end };
    });
  }, [lines]);
  useEffect(() => {
    const up = () => { if (live.current.dragging) { live.current.dragging = false; setDragging(false); } };
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, []);

  const act = useMemo<RowActions>(() => ({
    down: (i, shift) => {
      const s = live.current.sel;
      if (shift && s) setSel({ a: s.a, b: i });
      else { setSel({ a: i, b: i }); draft.current = ""; }
      live.current.dragging = true;
      setDragging(true);
    },
    pick: (i, shift) => {
      const s = live.current.sel;
      if (shift && s) setSel({ a: s.a, b: i });
      else { setSel({ a: i, b: i }); draft.current = ""; }
    },
    enter: (i) => {
      if (!live.current.dragging || live.current.lines[i]?.kind === "hunk") return;
      setSel((s) => (s && s.b !== i ? { a: s.a, b: i } : s));
    },
    save: (t) => {
      const { sel: s, lines: ls, file: f } = live.current;
      const note = t.trim();
      if (!s || !note) return;
      const rows = ls.slice(Math.min(s.a, s.b), Math.max(s.a, s.b) + 1).filter((l) => l.kind !== "hunk");
      const span = spanOf(rows);
      if (!span) return;
      live.current.onAdd({ id: crypto.randomUUID(), file: f, ...span, code: rows.map(mark).join("\n"), text: note });
      setSel(null);
      draft.current = "";
    },
    cancel: () => { setSel(null); draft.current = ""; },
    remove: (id) => live.current.onRemove([id]),
    draft,
  }), []);

  // Each comment hangs under the last line it covers; ones whose lines are gone are listed above the diff.
  const placed = useMemo(() => {
    const ours = new Map<number, ReviewComment[]>();
    const loose: ReviewComment[] = [];
    for (const c of comments) {
      const at = locate(lines, c);
      if (at < 0) loose.push(c);
      else ours.set(at, [...(ours.get(at) ?? []), c]);
    }
    const byKey = new Map<string, GhComment[]>();
    for (const c of gh) { const k = `${c.side === "LEFT" ? "old" : "new"}:${c.line}`; byKey.set(k, [...(byKey.get(k) ?? []), c]); }
    const theirs = new Map<number, GhComment[]>();
    if (byKey.size) lines.forEach((l, i) => {
      if (l.kind === "hunk") return;
      const hit = byKey.get(l.kind === "del" ? `old:${l.old}` : `new:${l.new}`);
      if (hit) theirs.set(i, hit);
    });
    return { ours, theirs, loose };
  }, [comments, gh, lines]);

  const lo = sel ? Math.min(sel.a, sel.b) : -1;
  const hi = sel ? Math.max(sel.a, sel.b) : -1;
  const boxLabel = useMemo(() => {
    if (!sel) return null;
    const span = spanOf(lines.slice(lo, hi + 1));
    return span ? linesLabel(span) : null;
  }, [sel, lines]);
  const boxAt = sel && !dragging && boxLabel ? hi : -1;
  const isSel = (i: number) => i >= lo && i <= hi;

  const looseList = placed.loose.length > 0 && (
    <div className="review-loose">
      {placed.loose.map((c) => (
        <div key={c.id} className="review-comment loose" data-comment={c.id}>
          <div className="review-comment-body">
            <span className="review-where">
              {outdated.has(c.id) ? <span className="chip outdated">Outdated</span> : <span className="chip">Not in this view</span>}
              {c.end_line > c.line ? "Lines" : "Line"} {c.line}{c.end_line > c.line ? ` to ${c.end_line}` : ""}
            </span>
            {c.code && <pre className="review-quote">{c.code}</pre>}
            <span className="review-text">{c.text}</span>
          </div>
          <button className="icon" aria-label="Delete comment" title="Delete comment" onClick={() => onRemove([c.id])}><X size={13} /></button>
        </div>
      ))}
    </div>
  );

  if (split) {
    type Cell = { l: Line; i: number } | null;
    const rows: [Cell, Cell][] = [];
    for (let i = 0; i < lines.length; ) {
      const l = lines[i];
      if (l.kind === "hunk" || l.kind === "ctx") { rows.push([{ l, i }, { l, i }]); i++; continue; }
      const dels: Cell[] = [], adds: Cell[] = [];
      while (i < lines.length && lines[i].kind === "del") { dels.push({ l: lines[i], i }); i++; }
      while (i < lines.length && lines[i].kind === "add") { adds.push({ l: lines[i], i }); i++; }
      for (let k = 0; k < Math.max(dels.length, adds.length); k++) rows.push([dels[k] ?? null, adds[k] ?? null]);
    }
    return (
      <>
        {looseList}
        <div className="diff split" role="table" aria-label={`Changes in ${file}, side by side`}>
          {rows.map(([a, b], k) => {
            const ia = a?.i ?? -1, ib = b?.i ?? -1;
            const here = ia === ib ? placed.ours.get(ia) ?? NONE : [...(placed.ours.get(ia) ?? NONE), ...(placed.ours.get(ib) ?? NONE)];
            const ghHere = ia === ib ? placed.theirs.get(ia) ?? NONE : [...(placed.theirs.get(ia) ?? NONE), ...(placed.theirs.get(ib) ?? NONE)];
            return (
              <SplitRow
                key={k} a={a?.l ?? null} b={b?.l ?? null} ia={ia} ib={ib}
                here={here.length ? here : NONE} ghHere={ghHere.length ? ghHere : NONE}
                selA={ia >= 0 && isSel(ia)} selB={ib >= 0 && isSel(ib)}
                box={boxAt >= 0 && (boxAt === ia || boxAt === ib) ? boxLabel : null} act={act}
              />
            );
          })}
        </div>
      </>
    );
  }

  return (
    <>
      {looseList}
      <div className={`diff${dragging ? " dragging" : ""}`} role="table" aria-label={`Changes in ${file}`}>
        {lines.map((l, i) => (
          <DiffRow
            key={i} l={l} i={i}
            here={placed.ours.get(i) ?? NONE}
            ghHere={placed.theirs.get(i) ?? NONE}
            selected={isSel(i)} box={i === boxAt ? boxLabel : null} act={act}
          />
        ))}
      </div>
    </>
  );
});

/**
 * A workspace's draft comments. Drafts from before 0.11 lived in this window's local storage: they move
 * into the database once (as line comments without quoted code, so they never show as outdated).
 */
export async function loadReview(workspaceId: string): Promise<ReviewComment[]> {
  const key = `nekotrail.review.${workspaceId}`;
  const old = localStorage.getItem(key);
  if (old) {
    let list: { id?: string; file?: string; line?: number; side?: string; text?: string }[] = [];
    try { list = JSON.parse(old); } catch { /* unreadable: nothing to move */ }
    const moved: ReviewComment[] = (Array.isArray(list) ? list : [])
      .filter((c) => c && c.file && c.line && c.text?.trim())
      .map((c) => ({ id: c.id || crypto.randomUUID(), file: c.file!, side: c.side === "old" ? "old" : "new", line: c.line!, end_line: c.line!, code: "", text: c.text! }));
    if (moved.length) await api.addReviewComments(workspaceId, moved);
    localStorage.removeItem(key);
  }
  return api.reviewComments(workspaceId);
}

/** Turn review comments into one message the agent can act on. */
export function reviewPrompt(comments: ReviewComment[], outdated: Set<string> = new Set()): string {
  const MAX = 30;
  const items = comments.map((c, i) => {
    const notes = [c.side === "old" ? "removed lines" : "", outdated.has(c.id) ? "these lines changed since I commented" : ""].filter(Boolean);
    const head = `${i + 1}. \`${where(c)}\`${notes.length ? ` (${notes.join(", ")})` : ""}`;
    const code = c.code.split("\n");
    const quote = !c.code ? [] : [
      "   ```",
      ...code.slice(0, MAX).map((l) => `   ${l}`),
      ...(code.length > MAX ? [`   ... ${code.length - MAX} more lines`] : []),
      "   ```",
    ];
    return [head, ...quote, ...c.text.split("\n").map((l) => `   ${l}`)].join("\n");
  });
  return `Here's my review of your changes. Please address each comment:\n\n${items.join("\n\n")}`;
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
        <button onClick={save} disabled={!dirty} title={keyHint("Save file (⌘S)")}><Save size={14} />Save</button>
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
            if (mod(e) && e.key === "s") { e.preventDefault(); save(); }
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
  /** Hand text to the chat. With `send`, it goes to the agent right away when the agent is free. */
  onSendReview: (text: string, send?: boolean) => void;
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
  // Draft comments live in the app's database, per workspace, so they survive restarts until sent.
  const [comments, setComments] = useState<ReviewComment[]>([]);
  useEffect(() => {
    let on = true;
    setComments([]);
    const load = (e?: Event) => {
      const for_ = (e as CustomEvent | undefined)?.detail;
      if (for_ && for_ !== workspaceId) return;
      loadReview(workspaceId).then((c) => { if (on) setComments(c); }).catch((err) => on && setNotice(`Couldn't load your review comments: ${errText(err)}`));
    };
    load();
    window.addEventListener("nekotrail-review", load);
    return () => { on = false; window.removeEventListener("nekotrail-review", load); };
  }, [workspaceId]);
  // Stable handlers for the diff, so a re-render here (a turn ending elsewhere) skips its 5,000 rows.
  // The list updates right away; the database write follows, and a failure puts the list back.
  const latest = useRef({ workspaceId, comments });
  latest.current = { workspaceId, comments };
  const addComment = useCallback((c: ReviewComment) => {
    const { workspaceId: id, comments: before } = latest.current;
    setComments([...before, c]);
    api.addReviewComments(id, [c]).catch((err) => { setComments(before); setNotice(`Couldn't save the comment: ${errText(err)}`); });
  }, []);
  const removeComments = useCallback((ids: string[]) => {
    const { workspaceId: id, comments: before } = latest.current;
    const gone = new Set(ids);
    setComments(before.filter((c) => !gone.has(c.id)));
    api.removeReviewComments(id, ids).catch((err) => { setComments(before); setNotice(`Couldn't remove the comment: ${errText(err)}`); });
  }, []);
  const [reviewOpen, setReviewOpen] = useState(false);
  useEffect(() => setReviewOpen(false), [workspaceId]);
  const [jump, setJump] = useState<string | null>(null);
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

  // Bumped by every reload, so the open diff and the outdated check follow a discard or a refresh too.
  const [rev, setRev] = useState(0);
  const load = () => {
    setRev((r) => r + 1);
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
  }, [open, workspaceId, rev, uncommitted]);

  // Outdated: the lines a comment quotes are no longer in the file's full diff (the agent changed them,
  // or they were discarded). Checked once per refresh, for the files that have comments only.
  const [outdated, setOutdated] = useState<Set<string>>(() => new Set());
  const commented = useMemo(() => [...new Set(comments.filter((c) => c.code).map((c) => c.file))].sort().join("\n"), [comments]);
  useEffect(() => {
    let on = true;
    const list = commented ? commented.split("\n") : [];
    Promise.all(list.map((f) => api.fileDiff(workspaceId, f, false).then(parseDiff).catch(() => null))).then((diffs) => {
      if (!on) return;
      const out = new Set<string>();
      list.forEach((f, k) => {
        const ls = diffs[k];
        if (ls) for (const c of latest.current.comments) if (c.file === f && c.code && locate(ls, c) < 0) out.add(c.id);
      });
      setOutdated((cur) => (cur.size === out.size && [...out].every((id) => cur.has(id)) ? cur : out));
    });
    return () => { on = false; };
  }, [workspaceId, rev, commented]);
  const stale = comments.filter((c) => outdated.has(c.id));

  // Jump to a comment picked in the Review list once its file's diff is on screen.
  useEffect(() => {
    if (!jump) return;
    const el = root.current?.querySelector(`[data-comment="${CSS.escape(jump)}"]`);
    if (el) { el.scrollIntoView({ block: "center" }); setJump(null); }
  }, [jump, diff, comments]);

  const sendReview = () => {
    if (!comments.length) return;
    onSendReview(reviewPrompt(comments, outdated), true);
    removeComments(comments.map((c) => c.id));
    setReviewOpen(false);
  };
  useEffect(() => { if (focusFile) setEditing(focusFile.path); }, [focusFile?.nonce]);

  // The list as filtered by the box above it; ↑/↓ move through what you can see.
  const q = query.trim().toLowerCase();
  const shown = useMemo(() => (files && q ? files.filter((f) => f.path.toLowerCase().includes(q)) : files ?? []), [files, q]);
  // The diff always belongs to a row you can see: when the filter hides the open file, the first
  // match opens instead. With no match the diff pane says so rather than showing a hidden file.
  const openShown = !q || shown.some((f) => f.path === open);
  useEffect(() => {
    if (!openShown && shown.length > 0) setOpen(shown[0].path);
  }, [openShown, shown]);

  // ↑/↓ move between files, ⌘⌥U switches all changes / uncommitted.
  const nav = useRef({ files: shown, open });
  nav.current = { files: shown, open };
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (root.current?.closest(".pane")?.hasAttribute("hidden")) return;
      if (mod(e) && e.altKey && e.code === "KeyU") { e.preventDefault(); setUncommitted((u) => !u); return; }
      const t = e.target as HTMLElement;
      if (t.closest("textarea, input, select") || mod(e) || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
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
  const byFile = useMemo(() => {
    const m = new Map<string, number>();
    for (const c of comments) m.set(c.file, (m.get(c.file) ?? 0) + 1);
    return m;
  }, [comments]);

  if (error) return <div className="banner error">{error}</div>;
  if (!files) return <div className="empty"><span className="spinner" /></div>;

  const add = files.reduce((n, f) => n + f.additions, 0);
  const del = files.reduce((n, f) => n + f.deletions, 0);
  const ghTop = gh.filter((c) => !c.reply_to);
  const seen = files.filter((f) => viewed[f.path] === sig(f)).length;
  const scope = (
    <span className="seg scope" role="tablist" aria-label="Which changes">
      <button role="tab" aria-selected={!uncommitted} className={!uncommitted ? "on" : ""} onClick={() => setUncommitted(false)}>All</button>
      <button role="tab" aria-selected={uncommitted} className={uncommitted ? "on" : ""} onClick={() => setUncommitted(true)} title={keyHint("Show uncommitted changes only (⌘⌥U)")}>Uncommitted</button>
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
          const n = (byFile.get(f.path) ?? 0) + gh.filter((c) => c.path === f.path && !c.reply_to).length;
          const isViewed = viewed[f.path] === sig(f);
          return (
            <div key={f.path} data-file={f.path} className={`file ${open === f.path ? "selected" : ""} ${isViewed ? "viewed" : ""}`}>
              <input type="checkbox" checked={isViewed} onChange={() => toggleViewed(f)} aria-label={`Mark ${f.path} as viewed`} title="Mark as viewed" />
              <button onClick={() => { setOpen(f.path); setEditing(null); }}>
                <span className={`status s-${f.status}`}>{f.status}</span>
                <span className="file-path" title={f.path}>
                  <span className="file-name">{f.path.split("/").pop()}</span>
                  {f.path.includes("/") && <span className="file-dir">{f.path.slice(0, f.path.lastIndexOf("/"))}</span>}
                </span>
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
              <Send size={14} /> Send {ghTop.length} GitHub {ghTop.length === 1 ? "comment" : "comments"}
            </button>
          )}
          {comments.length === 0 ? (
            <p className="muted">Click a line number to comment. Shift-click or drag for more lines. ↑/↓ moves between files.</p>
          ) : (
            <>
              {reviewOpen && (
                <div className="review-pop" role="dialog" aria-label="Your review">
                  <div className="review-pop-list">
                    {comments.map((c) => (
                      <div key={c.id} className="review-item">
                        <button
                          className="review-item-go"
                          title={`Show ${where(c)}`}
                          onClick={() => { setOpen(c.file); setEditing(null); setQuery(""); setJump(c.id); setReviewOpen(false); }}
                        >
                          <span className="review-item-where">
                            <code>{c.file.split("/").pop()}:{c.line}{c.end_line > c.line ? `-${c.end_line}` : ""}</code>
                            {outdated.has(c.id) && <span className="chip outdated">Outdated</span>}
                          </span>
                          <span className="review-item-text">{c.text}</span>
                        </button>
                        <button className="icon" aria-label={`Delete comment on ${where(c)}`} title="Delete comment" onClick={() => removeComments([c.id])}><X size={13} /></button>
                      </div>
                    ))}
                  </div>
                  <div className="review-pop-foot">
                    {stale.length > 0 && <button className="quiet" onClick={() => removeComments(stale.map((c) => c.id))}>Clear {stale.length} outdated</button>}
                    <span className="spacer" />
                    <button
                      className="quiet"
                      onClick={async () => {
                        if (await ask({ title: `Delete all ${comments.length} comments?`, body: "Your review drafts for this workspace go away. Nothing is sent to the agent.", ok: "Delete comments", danger: true })) {
                          removeComments(comments.map((c) => c.id));
                          setReviewOpen(false);
                        }
                      }}
                    >
                      Delete all
                    </button>
                  </div>
                </div>
              )}
              {stale.length > 0 && !reviewOpen && (
                <p className="review-stale">
                  <span>{stale.length} {stale.length === 1 ? "comment is" : "comments are"} outdated. The agent changed those lines.</span>
                  <button className="quiet" onClick={() => removeComments(stale.map((c) => c.id))}>Clear</button>
                </p>
              )}
              <div className="review-ctl">
                <button className="review-toggle" aria-expanded={reviewOpen} onClick={() => setReviewOpen((o) => !o)} title="See your comments">
                  Review ({comments.length})<ChevronDown size={13} className={reviewOpen ? "flip" : ""} />
                </button>
                <button className="accent" onClick={sendReview} title="Send all comments to the agent in one message">
                  <Send size={14} /> Send review
                </button>
              </div>
            </>
          )}
        </div>
      </div>
      <div className="file-diff">
        {editing ? (
          <FileEditor workspaceId={workspaceId} path={editing} onDone={() => { setEditing(null); load(); }} />
        ) : !openShown && shown.length === 0 ? (
          <div className="empty"><h2>The filter hides every file</h2><button onClick={() => setQuery("")}>Clear filter</button></div>
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
              outdated={outdated}
              split={split}
              onAdd={addComment}
              onRemove={removeComments}
            />
          </>
        ) : (
          <div className="empty"><Mascot size={64} mood="sleep" /><h2>No file selected</h2><p>{keyHint("Pick a file from the list, or press ⌘P to open any file.")}</p></div>
        )}
      </div>
    </div>
  );
}
