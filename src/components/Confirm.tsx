import { useEffect, useRef, useState } from "react";

export type Ask = {
  title: string;
  body?: string;
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
        const b = Array.from(box.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
        const i = b.indexOf(document.activeElement as HTMLButtonElement);
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
        {cur.body && <p id="confirm-body" className="muted">{cur.body}</p>}
        <div className="row end">
          {!cur.notice && <button onClick={() => done(false)}>{cur.cancel ?? "Cancel"}</button>}
          <button className={cur.danger ? "danger-fill" : "primary"} onClick={() => done(true)}>{cur.ok}</button>
        </div>
      </div>
    </div>
  );
}
