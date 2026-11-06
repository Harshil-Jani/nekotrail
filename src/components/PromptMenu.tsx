import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Lock } from "lucide-react";
import { api, errText } from "../api";
import { activeRange, applyEdit, loadPrompts, personalOf, render, SCOPE_LABEL, startFill, step, stops, unfilled, type Builtins, type Fill, type PromptEntry, type Seg } from "../prompts";

/** One row in a `/` menu: a chat command, a saved prompt, or a Claude Code command. */
export type SlashItem = { cmd: string; desc: string; prompt?: PromptEntry };

/** A prompt's text with its blanks drawn as chips, for previews. */
export function SegText({ segs, max = 600 }: { segs: Seg[]; max?: number }) {
  let left = max;
  const out: React.ReactNode[] = [];
  for (const [i, s] of segs.entries()) {
    if (left <= 0) { out.push("…"); break; }
    if (s.t === "text") {
      out.push(s.text.length > left ? s.text.slice(0, left) + "…" : s.text);
      left -= s.text.length;
    } else {
      out.push(<span key={i} className={`blank ${s.builtin ? "builtin" : ""}`}>{s.name}{s.default ? `: ${s.default}` : ""}</span>);
      left -= s.name.length;
    }
  }
  return <>{out}</>;
}

/** The `/` list: names on the left, the selected prompt's text underneath. */
export function SlashList({ items, sel, onPick, onHover, label = "Commands and prompts" }: {
  items: SlashItem[]; sel: number; onPick: (it: SlashItem) => void; onHover?: (i: number) => void; label?: string;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => { list.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" }); }, [sel]);
  const cur = items[sel]?.prompt;
  return (
    <div className="mention slash" role="presentation">
      <div className="slash-list" role="listbox" aria-label={label} ref={list}>
        {items.map((it, i) => (
          <button key={`${it.cmd}-${it.prompt?.scope ?? "cmd"}`} data-i={i} role="option" aria-selected={i === sel} className={i === sel ? "on" : ""}
            onMouseDown={(e) => { e.preventDefault(); onPick(it); }} onMouseMove={() => onHover?.(i)}>
            <b>{it.cmd}</b>
            <span className="muted slash-desc">{it.desc}</span>
            {it.prompt && it.prompt.scope !== "personal" && <span className="scope-chip">{SCOPE_LABEL[it.prompt.scope]}</span>}
          </button>
        ))}
      </div>
      {cur && (
        <div className="slash-preview" aria-live="polite">
          <div className="slash-preview-text"><SegText segs={cur.segs} /></div>
          <div className="muted slash-preview-foot">
            {cur.scope === "claude" ? "Claude Code runs this command itself." : cur.read_only ? <><Lock size={11} aria-hidden /> {cur.source}, shared through git</> : "Your prompt"}
            {cur.segs.some((s) => s.t === "field" && !s.builtin) && cur.scope !== "claude" ? ". Tab moves between the blanks." : ""}
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * Fills a prompt's blanks right in a textarea: the first blank is selected, typing replaces it,
 * Tab and ⇧Tab move between blanks, and the same blank elsewhere in the text follows along.
 * Typing outside a blank, Esc, or Tab past the last one leaves you with plain text.
 */
export function usePromptFill(ref: React.RefObject<HTMLTextAreaElement | null>, draft: string, setDraft: (s: string) => void) {
  const [fill, setFill] = useState<Fill | null>(null);
  const want = useRef<[number, number] | null>(null);
  const suffix = useRef(""); // text after the prompt (e.g. "/fix-bug the login page" adds "the login page")
  useLayoutEffect(() => {
    const el = ref.current;
    if (!want.current || !el) return;
    el.focus();
    el.setSelectionRange(want.current[0], want.current[1]);
    want.current = null;
  }, [draft, fill]);

  const select = (f: Fill) => { want.current = activeRange(f); };
  const start = (segs: Seg[], known: Builtins = {}, after = "") => {
    const r = startFill(segs, known);
    suffix.current = after;
    const text = r.text + after;
    setDraft(text);
    setFill(r.fill);
    if (r.fill) select(r.fill);
    else want.current = [text.length, text.length];
  };
  const stop = () => setFill(null);
  /** Call from onChange. Returns the text to show. */
  const change = (next: string): string => {
    if (!fill) return next;
    const f = applyEdit(fill, draft, next);
    if (!f) { setFill(null); return next; }
    const caret = ref.current?.selectionStart ?? next.length;
    setFill(f);
    want.current = [caret, caret];
    return render(f).text + suffix.current;
  };
  const go = (to: number) => {
    if (!fill) return;
    const f = { ...fill, at: to };
    setFill(f);
    select(f);
  };
  /** Tab / ⇧Tab / Esc while filling. True when it handled the key. */
  const key = (e: React.KeyboardEvent) => {
    if (!fill) return false;
    if (e.key === "Tab") {
      e.preventDefault();
      const f = step(fill, e.shiftKey ? -1 : 1);
      if (!f) { setFill(null); want.current = [draft.length, draft.length]; return true; }
      setFill(f);
      select(f);
      return true;
    }
    if (e.key === "Escape") { e.preventDefault(); setFill(null); return true; }
    return false;
  };
  /** Before sending: the first blank never filled, selected so you can type it. */
  const missing = (): string | null => {
    if (!fill) return null;
    const left = unfilled(fill);
    if (!left.length) return null;
    const idx = stops(fill).findIndex((i) => { const s = fill.segs[i]; return s.t === "field" && s.name === left[0]; });
    go(idx);
    return left[0];
  };
  return { fill, start, stop, change, key, missing, go };
}

/** The strip above the chat box while blanks are being filled: one chip per blank. */
export function FillBar({ fill, onGo }: { fill: Fill; onGo: (i: number) => void }) {
  const st = stops(fill);
  return (
    <div className="fill-bar" role="toolbar" aria-label="Blanks to fill">
      <span className="muted">Fill in</span>
      {st.map((i, k) => {
        const s = fill.segs[i];
        if (s.t !== "field") return null;
        const done = fill.filled.includes(s.name);
        return (
          <button key={s.name} className={`blank ${k === fill.at ? "on" : ""} ${done ? "done" : ""}`} aria-pressed={k === fill.at} onMouseDown={(e) => { e.preventDefault(); onGo(k); }}>
            {s.name}
          </button>
        );
      })}
      <span className="spacer" />
      <span className="muted fill-keys">Tab next · Esc done</span>
    </div>
  );
}

/** Save some text as a personal prompt: name, optional description, the text itself. */
export function SavePromptSheet({ text, where, onClose, onSaved }: {
  text: string; where: { id?: string; repo?: string }; onClose: () => void; onSaved?: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [body, setBody] = useState(text);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const all = await loadPrompts(where, true);
      const mine = personalOf(all);
      if (mine.some((p) => p.name.toLowerCase() === name.trim().toLowerCase())) throw new Error(`You already have /${name.trim()}. Pick another name.`);
      await api.promptsSave([...mine, { name: name.trim(), description: description.trim(), text: body }]);
      window.dispatchEvent(new Event("nekotrail-prompts"));
      onSaved?.(name.trim());
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : errText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal prompt-sheet" role="dialog" aria-modal="true" aria-labelledby="save-prompt-title">
        <h2 id="save-prompt-title">Save as prompt</h2>
        <PromptFields name={name} description={description} text={body} onName={setName} onDescription={setDescription} onText={setBody} autoFocus />
        {error && <p className="bad setting-note" role="alert">{error}</p>}
        <div className="row end">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={save} disabled={busy || !name.trim() || !body.trim()}>Save prompt</button>
        </div>
      </div>
    </div>
  );
}

/** Name, description and text, shared by the save sheet and the Settings editor. */
export function PromptFields({ name, description, text, onName, onDescription, onText, autoFocus }: {
  name: string; description: string; text: string;
  onName: (s: string) => void; onDescription: (s: string) => void; onText: (s: string) => void; autoFocus?: boolean;
}) {
  return (
    <div className="prompt-fields">
      <label>
        <span>Name</span>
        <span className="prompt-name-input">
          <span className="muted" aria-hidden>/</span>
          <input value={name} autoFocus={autoFocus} placeholder="review-api" aria-label="Name, typed after /" spellCheck={false}
            onChange={(e) => onName(e.target.value.replace(/^\//, "").replace(/\s+/g, "-").replace(/[^\w-]/g, ""))} />
        </span>
      </label>
      <label>
        <span>Description</span>
        <input value={description} placeholder="Optional, shown in the / menu" onChange={(e) => onDescription(e.target.value)} />
      </label>
      <label className="prompt-text">
        <span>Text</span>
        <textarea rows={6} value={text} onChange={(e) => onText(e.target.value)} placeholder="What the agent should do" />
      </label>
      <p className="muted setting-note">
        <code>{"{{ticket}}"}</code> leaves a blank you fill in before sending, <code>{"{{env:staging}}"}</code> gives it a starting value.
        {" "}<code>{"{{branch}}"}</code>, <code>{"{{base}}"}</code>, <code>{"{{files_changed}}"}</code> and <code>{"{{selection}}"}</code> fill themselves. Write <code>{"\\{{"}</code> for literal braces.
      </p>
    </div>
  );
}
