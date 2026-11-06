import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { TOUR } from "../onboarding";
import { keyHint } from "../platform";
import { Mascot } from "./Mascot";

type Box = { top: number; left: number; width: number; height: number };
const CARD_W = 300;
const GAP = 12;

type Side = "right" | "below" | "above" | "center";
type Pos = { top: number; left: number; side: Side; arrow: number };
const ARROW = 14;
const GAP_ARROW = GAP + ARROW / 2;

/**
 * Where the card goes: beside the target if it fits, else below, else above; always on screen.
 * `arrow` is how far along the card's near edge its arrow sits, so it points at the target.
 */
function place(t: Box | null, cardH: number): Pos {
  const vw = window.innerWidth, vh = window.innerHeight;
  const clampX = (x: number) => Math.min(Math.max(GAP, x), vw - CARD_W - GAP);
  const clampY = (y: number) => Math.min(Math.max(GAP, y), vh - cardH - GAP);
  const along = (at: number, size: number) => Math.min(Math.max(20, at), size - 20);
  if (!t) return { top: clampY((vh - cardH) / 2), left: clampX((vw - CARD_W) / 2), side: "center", arrow: 0 };
  if (t.left + t.width + GAP_ARROW + CARD_W <= vw - GAP && t.height > cardH * 0.6) {
    const top = clampY(t.top + 16);
    return { top, left: t.left + t.width + GAP_ARROW, side: "right", arrow: along(Math.min(t.top + t.height / 2, top + 32) - top, cardH) };
  }
  const left = clampX(t.left + t.width / 2 - CARD_W / 2);
  const x = along(t.left + t.width / 2 - left, CARD_W);
  if (t.top + t.height + GAP_ARROW + cardH <= vh - GAP) return { top: t.top + t.height + GAP_ARROW, left, side: "below", arrow: x };
  return { top: clampY(t.top - GAP_ARROW - cardH), left, side: "above", arrow: x };
}

/**
 * Coach marks: a ring around one part of the window and a card that says what it is for.
 * Esc or Skip ends it; ← and → step through. The caller marks it seen before it shows.
 */
export function Tour({ onClose }: { onClose: () => void }) {
  const [i, setI] = useState(0);
  const [box, setBox] = useState<Box | null>(null);
  const [pos, setPos] = useState<Pos | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const next = useRef<HTMLButtonElement>(null);
  const stop = TOUR[i];
  const last = i === TOUR.length - 1;

  useLayoutEffect(() => {
    const measure = () => {
      const el = stop.target ? document.querySelector<HTMLElement>(`[data-tour="${stop.target}"]`) : null;
      const r = el?.getBoundingClientRect();
      // The ring stays inside the window, so a target that runs to the edge still gets all four corners.
      const vw = window.innerWidth, vh = window.innerHeight;
      const edge = (n: number, max: number) => Math.min(Math.max(3, n), max - 3);
      const b = r && r.width > 0 && r.height > 0 ? (() => {
        const top = edge(r.top - 4, vh), left = edge(r.left - 4, vw);
        return { top, left, width: edge(r.right + 4, vw) - left, height: edge(r.bottom + 4, vh) - top };
      })() : null;
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
        {pos && pos.side !== "center" && (
          <span className={`tour-arrow ${pos.side}`} aria-hidden style={pos.side === "right" ? { top: pos.arrow } : { left: pos.arrow }} />
        )}
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
