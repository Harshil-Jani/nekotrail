import { useEffect, useRef, useState } from "react";

export type Ask = {
  title: string;
  /** Plain text; each "\n" starts a new paragraph. */
  body?: string;
  /** Short label and value pairs under the body ("Repo", "pawprint-api"), easier to check than a sentence. */
  facts?: [string, string][];
  /** Text shown exactly as given (a prompt from a link), set apart from the body and scrollable. */
  quote?: string;
  /** A line under the quote. */
  footer?: string;
  /** Label of the button that goes ahead, e.g. "Archive anyway". */
  ok: string;
  cancel?: string;
  /** Red go-ahead button, for anything that loses work. */
  danger?: boolean;
  /** Just tell the user something: one button, no Cancel. */
  notice?: boolean;
};

let show: ((a: Ask) => Promise<boolean>) | null = null;

/** A sheet that asks before a destructive action. Resolves true when the user goes ahead. */
export function ask(a: Ask): Promise<boolean> {
  return show ? show(a) : Promise.resolve(false);
}

/** Mount once near the root. Cancel has focus, so Return or Esc never destroys anything by accident. */
export function ConfirmHost() {
  const [cur, setCur] = useState<(Ask & { resolve: (ok: boolean) => void }) | null>(null);
  const box = useRef<HTMLDivElement>(null);
  // A quote taller than its box says so until it has been scrolled to the end, so nobody approves
  // a task from a link without the chance to see all of it.
  const quote = useRef<HTMLPreElement>(null);
  const [more, setMore] = useState(false);
  const checkMore = () => { const q = quote.current; setMore(!!q && q.scrollHeight - q.scrollTop - q.clientHeight > 2); };
  useEffect(checkMore, [cur]);
  useEffect(() => {
    show = (a) => new Promise<boolean>((resolve) => setCur({ ...a, resolve }));
    return () => { show = null; };
  }, []);
  const done = (ok: boolean) => { cur?.resolve(ok); setCur(null); };
  useEffect(() => {
    if (!cur) return;
    const back = document.activeElement as HTMLElement | null;
    box.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); done(false); }
      if (e.key === "Tab") {
        // Keep focus inside the sheet.
        // The buttons, and the task from a link so the keyboard can scroll it.
        const b = Array.from(box.current?.querySelectorAll<HTMLElement>("button, [tabindex='0']") ?? []);
        const i = b.indexOf(document.activeElement as HTMLElement);
        e.preventDefault();
        b[(i + (e.shiftKey ? -1 : 1) + b.length) % b.length]?.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => { window.removeEventListener("keydown", onKey, true); back?.focus?.(); };
  }, [cur]);
  if (!cur) return null;
  return (
    <div className="modal-backdrop confirm-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) done(false); }}>
      <div className="modal confirm" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby={cur.body ? "confirm-body" : undefined} ref={box}>
        <h2 id="confirm-title">{cur.title}</h2>
        {/* Each line of the body is its own paragraph, so a consequence can stand on its own line. */}
        {cur.body && <div id="confirm-body" className="confirm-body">{cur.body.split("\n").map((line, i) => <p key={i} className="muted">{line}</p>)}</div>}
        {cur.facts && (
          <dl className="confirm-facts">
            {cur.facts.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
          </dl>
        )}
        {cur.quote !== undefined && (
          <div className={`confirm-quote-wrap${more ? " more" : ""}`}>
            <pre className="confirm-quote" ref={quote} onScroll={checkMore} tabIndex={0} aria-label="Task">{cur.quote}</pre>
            {more && <p className="confirm-more" aria-hidden>Scroll to read the whole task</p>}
          </div>
        )}
        {cur.footer && <p className="muted confirm-footer">{cur.footer}</p>}
        <div className="row end">
          {!cur.notice && <button onClick={() => done(false)}>{cur.cancel ?? "Cancel"}</button>}
          <button className={cur.danger ? "danger-fill" : "primary"} onClick={() => done(true)}>{cur.ok}</button>
        </div>
      </div>
    </div>
  );
}
