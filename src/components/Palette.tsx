import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";

export type Command = {
  id: string;
  label: string;
  group: string;
  /** Extra words that should match, e.g. the branch of a workspace. */
  keywords?: string;
  hint?: string;
  icon?: React.ReactNode;
  run: () => void;
};

/** Score a command against the query: label prefix > word prefix > substring > in-order letters. */
function score(c: Command, q: string): number {
  if (!q) return 0;
  const label = c.label.toLowerCase();
  const hay = `${label} ${(c.keywords ?? "").toLowerCase()} ${c.group.toLowerCase()}`;
  if (label.startsWith(q)) return 1;
  if (label.split(/[\s/·-]+/).some((w) => w.startsWith(q))) return 2;
  if (hay.includes(q)) return 3;
  let i = 0;
  for (const ch of hay) if (ch === q[i]) i++;
  return i === q.length ? 4 : -1;
}

/** ⌘K: one box to jump to any workspace or run any action. */
export function Palette({ commands, onClose }: { commands: Command[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  const results = useMemo(() => {
    const query = q.trim().toLowerCase();
    return commands
      .map((c, i) => ({ c, s: score(c, query), i }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => a.s - b.s || a.i - b.i)
      .slice(0, 60)
      .map((x) => x.c);
  }, [commands, q]);

  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    list.current?.querySelector(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const go = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };

  let lastGroup = "";
  return (
    <div className="palette-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette">
        <div className="palette-input">
          <Search size={16} />
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Jump to a workspace or run a command"
            aria-label="Search commands"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(results.length - 1, s + 1)); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
              else if (e.key === "Enter") { e.preventDefault(); go(results[sel]); }
              else if (e.key === "Escape") { e.preventDefault(); onClose(); }
            }}
          />
          <kbd>esc</kbd>
        </div>
        <div className="palette-list" id="palette-list" role="listbox" ref={list}>
          {results.length === 0 && <p className="muted palette-empty">No match. The cat looked everywhere.</p>}
          {results.map((c, i) => {
            const head = !q.trim() && c.group !== lastGroup ? c.group : null;
            lastGroup = c.group;
            return (
              <div key={c.id}>
                {head && <div className="palette-group">{head}</div>}
                <button
                  data-i={i}
                  role="option"
                  aria-selected={i === sel}
                  className={i === sel ? "on" : ""}
                  onMouseMove={() => setSel(i)}
                  onClick={() => go(c)}
                >
                  <span className="palette-icon">{c.icon}</span>
                  <span className="palette-label">{c.label}</span>
                  {q.trim() ? <span className="palette-hint">{c.group}</span> : c.hint && <span className="palette-hint">{c.hint}</span>}
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
