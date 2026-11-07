import { useMemo, useState } from "react";
import type { Sky, Workspace } from "../api";

/** Faint background stars, the same every time (seeded), so the sky feels deep but never shifts. */
function dust(n: number, w: number, h: number) {
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  return Array.from({ length: n }, () => ({ x: rnd() * w, y: rnd() * h, r: rnd() * 0.9 + 0.25, o: rnd() * 0.4 + 0.08 }));
}

/**
 * The quest, as a full sky rather than a dashboard. A workspace lights its star; a merged PR makes it
 * shine; a line appears between two stars only when both have shipped. Pick a constellation (in the
 * sky or in the index below) to read its story; the rest of the sky dims.
 */
export function NightSky({
  sky,
  lit,
  shining,
  workspaces,
  onOpen,
  onNew,
  hasRepos,
}: {
  sky: Sky;
  lit: Set<string>;
  shining: Set<string>;
  workspaces: Workspace[];
  onOpen: (workspaceId: string) => void;
  /** Offered while no star is lit yet. */
  onNew?: () => void;
  /** With no repos yet, the first step is adding one. */
  hasRepos?: boolean;
}) {
  const [focus, setFocus] = useState<string | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const active = hover ?? focus;

  const wsByStar = useMemo(() => {
    const m = new Map<string, Workspace>();
    for (const w of workspaces) m.set(w.name.replace(/-\d+$/, ""), w);
    return m;
  }, [workspaces]);
  const pos = useMemo(() => new Map(sky.stars.map((s) => [s.slug, s])), [sky]);

  // Frame the sky around the stars themselves, so nothing is ever cut off.
  const box = useMemo(() => {
    if (!sky.stars.length) return { x: 0, y: 0, w: 1200, h: 640 };
    const xs = sky.stars.map((s) => s.x), ys = sky.stars.map((s) => s.y);
    const pad = 90;
    const x = Math.min(...xs) - pad, y = Math.min(...ys) - pad;
    return { x, y, w: Math.max(...xs) - x + pad, h: Math.max(...ys) - y + pad + 30 };
  }, [sky]);
  const DUST = useMemo(() => dust(320, box.w * 1.6, box.h * 1.6), [box]);

  const stats = sky.constellations.map((c) => {
    const stars = sky.stars.filter((s) => s.constellation === c.slug);
    const nLit = stars.filter((s) => lit.has(s.slug)).length;
    const nShining = stars.filter((s) => shining.has(s.slug)).length;
    const cx = stars.reduce((a, s) => a + s.x, 0) / stars.length;
    const maxY = Math.max(...stars.map((s) => s.y));
    return { c, total: stars.length, nLit, nShining, cx, labelY: maxY + 30, complete: nShining === stars.length };
  });
  const totalShining = sky.stars.filter((s) => shining.has(s.slug)).length;
  const current = stats.find((s) => s.c.slug === active) ?? null;
  const dim = (slug: string) => (active && active !== slug ? "dim" : "");

  return (
    <div className="sky-view">
      <svg
        className="sky"
        viewBox={`${box.x} ${box.y} ${box.w} ${box.h}`}
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label="Your night sky"
        onClick={(e) => { if (e.target === e.currentTarget) setFocus(null); }}
      >
        <defs>
          <filter id="glow" x="-200%" y="-200%" width="500%" height="500%">
            <feGaussianBlur stdDeviation="3.2" result="b" />
            <feMerge><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge>
          </filter>
        </defs>
        <g transform={`translate(${box.x - box.w * 0.3} ${box.y - box.h * 0.3})`} aria-hidden>
          {DUST.map((d, i) => <circle key={i} cx={d.x} cy={d.y} r={d.r} fill="#fff" opacity={d.o} />)}
        </g>

        {sky.constellations.map((c) => (
          <g key={c.slug} className={`sky-cat ${dim(c.slug)}`}>
            {c.lines.map(([a, b]) => {
              const A = pos.get(a)!, B = pos.get(b)!;
              const cls = shining.has(a) && shining.has(b) ? "shipped" : lit.has(a) && lit.has(b) ? "hinted" : "ghost";
              return <line key={`${a}-${b}`} x1={A.x} y1={A.y} x2={B.x} y2={B.y} className={`sky-line ${cls}`} />;
            })}
          </g>
        ))}

        {sky.stars.map((s, i) => {
          const w = wsByStar.get(s.slug);
          const state = shining.has(s.slug) ? "shining" : lit.has(s.slug) ? "lit" : "dark";
          const r = state === "shining" ? 5.5 : state === "lit" ? 4 : 2.6;
          const label = `${s.name}: ${state === "dark" ? "not lit yet" : state === "lit" ? "lit, PR not merged yet" : "shining, PR merged"}${w ? ` (${w.title || w.branch})` : ""}`;
          const open = w && w.state === "active" ? () => onOpen(w.id) : undefined;
          return (
            <g
              key={s.slug}
              className={`sky-star ${state} ${open ? "openable" : ""} ${dim(s.constellation)}`}
              onClick={open ?? (() => setFocus(s.constellation))}
              onMouseEnter={() => setHover(s.constellation)}
              onMouseLeave={() => setHover(null)}
              onKeyDown={(e) => { if (open && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); open(); } }}
              tabIndex={open ? 0 : -1}
              role={open ? "button" : undefined}
              aria-label={label}
              style={{ animationDelay: `${(i % 7) * 0.6}s` }}
            >
              <title>{label}</title>
              <circle cx={s.x} cy={s.y} r={14} fill="transparent" />
              <circle cx={s.x} cy={s.y} r={r} filter={state === "dark" ? undefined : "url(#glow)"} />
              {(state !== "dark" || active === s.constellation) && <text x={s.x + 9} y={s.y - 8}>{s.name}</text>}
            </g>
          );
        })}

        {stats.map((st) => (
          <text
            key={st.c.slug}
            x={st.cx}
            y={st.labelY}
            textAnchor="middle"
            className={`sky-label ${st.complete ? "complete" : ""} ${dim(st.c.slug)} ${active === st.c.slug ? "on" : ""}`}
            onMouseEnter={() => setHover(st.c.slug)}
            onMouseLeave={() => setHover(null)}
            onClick={() => setFocus((f) => (f === st.c.slug ? null : st.c.slug))}
          >
            {st.c.name}
          </text>
        ))}
      </svg>

      <header className="sky-title" data-tauri-drag-region>
        <h1>Night sky</h1>
        <p className="num">
          {lit.size} of {sky.stars.length} stars lit, {totalShining} shining
        </p>
      </header>

      <div className="sky-caption" aria-live="polite">
        {current ? (
          <>
            <h2>{current.c.name}</h2>
            <p className="story">{current.c.story}</p>
            <p className="num count">
              {current.nLit} of {current.total} lit · {current.nShining} shining
              {current.complete ? " · complete" : ""}
            </p>
          </>
        ) : (
          <>
            <p className="story">Every workspace lights a star. Merge its PR and the star shines; when two neighbours both shine, the line between them appears. Finish a constellation to complete its cat.</p>
            <p className="legend">
              <svg width="10" height="10" aria-hidden><circle cx="5" cy="5" r="3" fill="none" stroke="currentColor" strokeWidth="1.2" /></svg>not lit
              <svg width="10" height="10" aria-hidden><circle cx="5" cy="5" r="3.5" style={{ fill: "var(--star)" }} /></svg>lit
              <svg width="12" height="12" aria-hidden><circle cx="6" cy="6" r="4.5" style={{ fill: "var(--star-shipped)" }} /></svg>shining
            </p>
            {lit.size === 0 && onNew && <button className="primary sky-first" onClick={onNew}>{hasRepos === false ? "Add a repo to light your first star" : "Create a workspace to light your first star"}</button>}
          </>
        )}
      </div>

      <nav className="sky-index" aria-label="Constellations">
        {stats.map((st) => (
          <button
            key={st.c.slug}
            className={`${active === st.c.slug ? "on" : ""} ${st.complete ? "complete" : ""}`}
            onMouseEnter={() => setHover(st.c.slug)}
            onMouseLeave={() => setHover(null)}
            onClick={() => setFocus((f) => (f === st.c.slug ? null : st.c.slug))}
            aria-pressed={focus === st.c.slug}
          >
            {st.c.name}
            <span className="num">{st.nShining}/{st.total}</span>
          </button>
        ))}
      </nav>
    </div>
  );
}
