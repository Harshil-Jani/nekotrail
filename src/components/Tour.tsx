import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { TOUR } from "../onboarding";
import { keyHint } from "../platform";
import { Mascot } from "./Mascot";

type Box = { top: number; left: number; width: number; height: number };
const CARD_W = 300;
const GAP = 12;

/** Where the card goes: beside the target if it fits, else below, else above; always on screen. */
function place(t: Box | null, cardH: number): { top: number; left: number } {
  const vw = window.innerWidth, vh = window.innerHeight;
  const clampX = (x: number) => Math.min(Math.max(GAP, x), vw - CARD_W - GAP);
  const clampY = (y: number) => Math.min(Math.max(GAP, y), vh - cardH - GAP);
  if (!t) return { top: clampY((vh - cardH) / 2), left: clampX((vw - CARD_W) / 2) };
  if (t.left + t.width + GAP + CARD_W <= vw - GAP && t.height > cardH * 0.6) return { top: clampY(t.top + 16), left: t.left + t.width + GAP };
  if (t.top + t.height + GAP + cardH <= vh - GAP) return { top: t.top + t.height + GAP, left: clampX(t.left + t.width / 2 - CARD_W / 2) };
  return { top: clampY(t.top - GAP - cardH), left: clampX(t.left + t.width / 2 - CARD_W / 2) };
}

/**
 * Coach marks: a ring around one part of the window and a card that says what it is for.
 * Esc or Skip ends it; ← and → step through. The caller marks it seen before it shows.
 */
export function Tour({ onClose }: { onClose: () => void }) {
  const [i, setI] = useState(0);
  const [box, setBox] = useState<Box | null>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const next = useRef<HTMLButtonElement>(null);
  const stop = TOUR[i];
  const last = i === TOUR.length - 1;

  useLayoutEffect(() => {
    const measure = () => {
      const el = stop.target ? document.querySelector<HTMLElement>(`[data-tour="${stop.target}"]`) : null;
      const r = el?.getBoundingClientRect();
      const b = r && r.width > 0 && r.height > 0 ? { top: r.top - 4, left: r.left - 4, width: r.width + 8, height: r.height + 8 } : null;
      setBox(b);
      setPos(place(b, card.current?.offsetHeight ?? 180));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [i, stop.target]);
  useEffect(() => { next.current?.focus(); }, [i]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onClose(); }
      else if (e.key === "ArrowRight") { e.preventDefault(); if (last) onClose(); else setI((x) => x + 1); }
      else if (e.key === "ArrowLeft") { e.preventDefault(); setI((x) => Math.max(0, x - 1)); }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [last, onClose]);

  return (
    <div className="tour">
      {/* The dim with a hole where the target is; clicks on the dim do nothing, so the card is the way out. */}
      {box
        ? <div className="tour-ring" style={{ top: box.top, left: box.left, width: box.width, height: box.height }} />
        : <div className="tour-dim" />}
      <div
        ref={card}
        className="tour-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="tour-title"
        aria-describedby="tour-body"
        style={{ width: CARD_W, top: pos?.top ?? -9999, left: pos?.left ?? -9999 }}
      >
        <div className="row">
          <span className="tour-count num">{i + 1} of {TOUR.length}</span>
          <span className="spacer" />
          <button className="icon" aria-label="Skip the tour" title="Skip the tour (Esc)" onClick={onClose}><X size={14} /></button>
        </div>
        {last && <Mascot size={40} mood="happy" />}
        <h2 id="tour-title">{keyHint(stop.title)}</h2>
        <p id="tour-body">{keyHint(stop.body)}</p>
        <div className="row tour-actions">
          <span className="tour-dots" aria-hidden>{TOUR.map((_, n) => <span key={n} className={n === i ? "on" : ""} />)}</span>
          <span className="spacer" />
          {i > 0 && <button className="quiet" onClick={() => setI(i - 1)}>Back</button>}
          <button ref={next} className="primary" onClick={() => (last ? onClose() : setI(i + 1))}>{last ? "Done" : "Next"}</button>
        </div>
        {last && <p className="muted tour-again">{keyHint("Show it again any time: ⌘K, then \"Show the tour\".")}</p>}
      </div>
    </div>
  );
}
