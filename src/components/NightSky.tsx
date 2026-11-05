import { useMemo } from "react";
import type { Sky, Workspace } from "../api";
import { Mascot } from "./Mascot";

/** Faint background stars, the same every time (seeded), so the sky feels deep but never shifts. */
function dust(n: number) {
  let seed = 7;
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  return Array.from({ length: n }, () => ({ x: rnd() * 1200, y: rnd() * 640, r: rnd() * 0.9 + 0.3, o: rnd() * 0.35 + 0.1 }));
}
const DUST = dust(220);

/**
 * The quest. A workspace lights its star; a merged PR makes it shine; a line appears
 * between two stars only when both have shipped. Finish every star and the cat is complete.
 */
export function NightSky({
  sky,
  lit,
  shining,
  workspaces,
  onOpen,
}: {
  sky: Sky;
  lit: Set<string>;
  shining: Set<string>;
  workspaces: Workspace[];
  onOpen: (workspaceId: string) => void;
}) {
  const wsByStar = useMemo(() => {
    const m = new Map<string, Workspace>();
    for (const w of workspaces) m.set(w.name.replace(/-\d+$/, ""), w);
    return m;
  }, [workspaces]);
  const pos = useMemo(() => new Map(sky.stars.map((s) => [s.slug, s])), [sky]);

  const stats = sky.constellations.map((c) => {
    const stars = sky.stars.filter((s) => s.constellation === c.slug);
    const nLit = stars.filter((s) => lit.has(s.slug)).length;
    const nShining = stars.filter((s) => shining.has(s.slug)).length;
    const cx = stars.reduce((a, s) => a + s.x, 0) / stars.length;
    const maxY = Math.max(...stars.map((s) => s.y));
    return { c, total: stars.length, nLit, nShining, cx, labelY: maxY + 34, complete: nShining === stars.length };
  });
  const totalShining = sky.stars.filter((s) => shining.has(s.slug)).length;
  const done = stats.filter((s) => s.complete).length;

  return (
    <div className="sky-view">
      <header className="sky-head">
        <Mascot size={64} mood={done > 0 ? "happy" : "idle"} follow />
        <div>
          <h1>Night sky</h1>
          <p className="muted num">
            {lit.size} of {sky.stars.length} stars lit, {totalShining} shining, {done} of {sky.constellations.length} cats complete.
            New workspaces light a star; merged PRs make it shine and join it to its neighbours.
          </p>
        </div>
      </header>

      <svg className="sky" viewBox="0 0 1200 640" role="img" aria-label="Constellation map">
        <defs>
          <filter id="glow" x="-200%" y="-200%" width="500%" height="500%">
            <feGaussianBlur stdDeviation="3" result="b" />
            <feMerge><feMergeNode in="b" /><feMergeNode in="SourceGraphic" /></feMerge>
          </filter>
        </defs>
        {DUST.map((d, i) => <circle key={i} cx={d.x} cy={d.y} r={d.r} fill="#fff" opacity={d.o} />)}

        {sky.constellations.map((c) =>
          c.lines.map(([a, b]) => {
            const A = pos.get(a)!, B = pos.get(b)!;
            const both = shining.has(a) && shining.has(b);
            const seen = lit.has(a) && lit.has(b);
            if (!both && !seen) return null;
            return (
              <line
                key={`${a}-${b}`}
                x1={A.x} y1={A.y} x2={B.x} y2={B.y}
                className={both ? "sky-line shipped" : "sky-line hinted"}
              />
            );
          }),
        )}

        {sky.stars.map((s, i) => {
          const w = wsByStar.get(s.slug);
          const state = shining.has(s.slug) ? "shining" : lit.has(s.slug) ? "lit" : "dark";
          const r = state === "shining" ? 5.5 : state === "lit" ? 4 : 2;
          const label = `${s.name}: ${state === "dark" ? "not lit yet" : state === "lit" ? "lit, PR not merged yet" : "shining, PR merged"}${w ? ` (${w.branch})` : ""}`;
          const open = w && w.state === "active" ? () => onOpen(w.id) : undefined;
          return (
            <g
              key={s.slug}
              className={`sky-star ${state} ${open ? "openable" : ""}`}
              onClick={open}
              onKeyDown={(e) => { if (open && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); open(); } }}
              tabIndex={open ? 0 : -1}
              role={open ? "button" : undefined}
              aria-label={label}
              style={{ animationDelay: `${(i % 7) * 0.6}s` }}
            >
              <title>{label}</title>
              <circle cx={s.x} cy={s.y} r={12} fill="transparent" />
              <circle cx={s.x} cy={s.y} r={r} filter={state === "dark" ? undefined : "url(#glow)"} />
              {state !== "dark" && <text x={s.x + 9} y={s.y - 8}>{s.name}</text>}
            </g>
          );
        })}

        {stats.map((st) => (
          <text key={st.c.slug} x={st.cx} y={st.labelY} className={`sky-label ${st.complete ? "complete" : ""}`} textAnchor="middle">
            {st.c.name} {st.nShining}/{st.total}
          </text>
        ))}
      </svg>

      <div className="constellations">
        {stats.map((st) => (
          <article key={st.c.slug} className={`constellation ${st.complete ? "complete" : ""}`}>
            <h3>{st.c.name}</h3>
            <p>{st.c.story}</p>
            <div className="meter" aria-label={`${st.nShining} of ${st.total} shining`}>
              {Array.from({ length: st.total }, (_, i) => (
                <span key={i} className={i < st.nShining ? "on" : i < st.nLit ? "half" : ""} />
              ))}
            </div>
            <span className="muted num">{st.nLit} lit · {st.nShining} shining</span>
          </article>
        ))}
      </div>
    </div>
  );
}
