import { useEffect, useRef, useState } from "react";

export type Prompt = { row: number; text: string };

/**
 * One tick per message you sent, on the chat's right edge. The tick for the part of the chat on
 * screen is lit. Hover (or focus) opens the list of your messages; click one to jump to it.
 */
export function PromptRail({ prompts, current, onJump, onOpen }: {
  prompts: Prompt[];
  current: number; // index into prompts of the message on screen, -1 when none
  onJump: (row: number) => void;
  onOpen?: () => void; // the list opened: a chat that loaded only its tail can load the rest
}) {
  const [open, setOpen] = useState(false);
  const [hot, setHot] = useState(-1);
  const closeT = useRef<number | undefined>(undefined);
  const list = useRef<HTMLDivElement>(null);

  const show = () => { window.clearTimeout(closeT.current); if (!open) { setOpen(true); onOpen?.(); } };
  const hide = () => { window.clearTimeout(closeT.current); closeT.current = window.setTimeout(() => { setOpen(false); setHot(-1); }, 180); };
  useEffect(() => () => window.clearTimeout(closeT.current), []);
  // Opening scrolls the list to the message you are reading.
  useEffect(() => {
    if (open && current >= 0) list.current?.querySelector(`[data-i="${current}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open]);

  if (prompts.length < 2) return null;
  const jump = (i: number) => { onJump(prompts[i].row); setOpen(false); setHot(-1); };
  const firstLine = (t: string) => t.trim().split("\n")[0] || t.trim();

  return (
    <div className={`prompt-rail${open ? " open" : ""}`} onMouseEnter={show} onMouseLeave={hide} onFocus={show} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) hide(); }}>
      <div className="prompt-ticks" aria-hidden>
        {prompts.map((p, i) => (
          <span key={p.row} className={`tick${i === current ? " on" : ""}${i === hot ? " hot" : ""}`} onClick={() => jump(i)} />
        ))}
      </div>
      {open && (
        <nav className="prompt-list" ref={list} aria-label="Your messages">
          <div className="prompt-list-head">Your messages <span className="num">{prompts.length}</span></div>
          {prompts.map((p, i) => (
            <button key={p.row} data-i={i} className={i === current ? "on" : ""} title={p.text.slice(0, 400)}
              onMouseEnter={() => setHot(i)} onFocus={() => setHot(i)} onClick={() => jump(i)}>
              <span className="num">{i + 1}</span><span className="t">{firstLine(p.text)}</span>
            </button>
          ))}
        </nav>
      )}
    </div>
  );
}
