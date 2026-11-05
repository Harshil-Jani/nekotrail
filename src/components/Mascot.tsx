/**
 * nekotrail's mascot: a marmalade loaf cat whose tail ends in a git merge
 * (two branches joining at a commit dot). Original artwork, drawn here as SVG.
 */
import { useEffect, useRef, useState } from "react";

/**
 * Moods map to real app states, never decoration:
 * idle = nothing running, working = an agent is running (tail wags),
 * happy = something shipped, sleep = idle for a while or an empty screen,
 * worried = CI failing or a turn ended in an error.
 */
export type Mood = "idle" | "working" | "happy" | "sleep" | "worried";

const FUR = "#ff9a4d";
const FUR_DARK = "#e5762a";
const INK = "#1a1006";
const BLUSH = "#ff6f6f";

function Eyes({ mood, look }: { mood: Mood; look: { x: number; y: number } }) {
  if (mood === "sleep") {
    return (
      <g stroke={INK} strokeWidth="3" strokeLinecap="round" fill="none">
        <path d="M37 58 q6 5 12 0" />
        <path d="M71 58 q6 5 12 0" />
      </g>
    );
  }
  if (mood === "happy") {
    return (
      <g stroke={INK} strokeWidth="3.2" strokeLinecap="round" fill="none">
        <path d="M37 60 q6 -8 12 0" />
        <path d="M71 60 q6 -8 12 0" />
      </g>
    );
  }
  // Pupils glance at the work while busy, otherwise follow the cursor.
  const dx = mood === "working" ? 3 : look.x;
  const dy = mood === "working" ? 0 : look.y;
  return (
    <g>
      {mood === "worried" && (
        <g stroke={INK} strokeWidth="2.6" strokeLinecap="round">
          <path d="M36 44 L48 47" />
          <path d="M84 44 L72 47" />
        </g>
      )}
      <ellipse cx={43 + dx} cy={57 + dy} rx="6.5" ry={mood === "worried" ? 6.5 : 8} fill={INK} />
      <ellipse cx={77 + dx} cy={57 + dy} rx="6.5" ry={mood === "worried" ? 6.5 : 8} fill={INK} />
      <circle cx={45.5 + dx} cy={53.5 + dy} r="2.4" fill="#fff" />
      <circle cx={79.5 + dx} cy={53.5 + dy} r="2.4" fill="#fff" />
    </g>
  );
}

export function Mascot({
  size = 48,
  mood = "idle",
  title = "nekotrail cat",
  follow = false,
  purring = false,
  onClick,
}: {
  size?: number;
  mood?: Mood;
  title?: string;
  /** Eyes follow the mouse pointer. */
  follow?: boolean;
  purring?: boolean;
  onClick?: () => void;
}) {
  const ref = useRef<SVGSVGElement>(null);
  const [look, setLook] = useState({ x: 0, y: 0 });
  useEffect(() => {
    if (!follow) return;
    let frame = 0;
    const onMove = (e: MouseEvent) => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const r = ref.current?.getBoundingClientRect();
        if (!r) return;
        const ax = e.clientX - (r.left + r.width / 2);
        const ay = e.clientY - (r.top + r.height / 2);
        const d = Math.hypot(ax, ay) || 1;
        const reach = Math.min(1, d / 200); // farther pointer, fuller glance
        setLook({ x: (ax / d) * 2.6 * reach, y: (ay / d) * 2 * reach });
      });
    };
    window.addEventListener("mousemove", onMove);
    return () => { window.removeEventListener("mousemove", onMove); cancelAnimationFrame(frame); };
  }, [follow]);

  return (
    <svg
      ref={ref}
      width={size}
      height={size}
      viewBox="0 0 140 120"
      role="img"
      aria-label={title}
      className={`mascot mascot-${mood} ${purring ? "purring" : ""} ${onClick ? "pettable" : ""}`}
      onClick={onClick}
    >
      {/* merge tail: two branches join at a commit dot */}
      <g className="mascot-tail" stroke={FUR_DARK} strokeWidth="7" strokeLinecap="round" fill="none">
        <path d="M100 100 C 122 100, 128 84, 124 66" />
        <path d="M124 66 C 121 54, 112 50, 112 40" />
        <path d="M124 66 C 128 56, 134 52, 134 42" />
      </g>
      <circle cx="124" cy="66" r="6" fill="#fff" stroke={FUR_DARK} strokeWidth="3" />
      <circle cx="112" cy="38" r="5" fill="#fff" stroke={FUR_DARK} strokeWidth="3" />
      <circle cx="134" cy="40" r="5" fill="#fff" stroke={FUR_DARK} strokeWidth="3" />
      {/* ears */}
      <path d="M22 44 L30 12 Q32 8 36 11 L55 30 Z" fill={FUR} stroke={FUR} strokeWidth="4" strokeLinejoin="round" />
      <path d="M98 44 L90 12 Q88 8 84 11 L65 30 Z" fill={FUR} stroke={FUR} strokeWidth="4" strokeLinejoin="round" />
      <path d="M30 36 L34 19 L45 30 Z" fill={BLUSH} opacity="0.55" />
      <path d="M90 36 L86 19 L75 30 Z" fill={BLUSH} opacity="0.55" />
      {/* loaf body */}
      <path d="M14 70 C 14 36, 36 26, 60 26 C 84 26, 106 36, 106 70 L106 92 C 106 104, 98 110, 86 110 L34 110 C 22 110, 14 104, 14 92 Z" fill={FUR} />
      {/* forehead stripes */}
      <g stroke={FUR_DARK} strokeWidth="3.5" strokeLinecap="round">
        <path d="M60 31 L60 41" />
        <path d="M51 33 L53 41" />
        <path d="M69 33 L67 41" />
      </g>
      {/* tucked paws */}
      <ellipse cx="44" cy="108" rx="11" ry="6" fill="#ffd2ad" />
      <ellipse cx="76" cy="108" rx="11" ry="6" fill="#ffd2ad" />
      <Eyes mood={mood} look={look} />
      <ellipse cx="33" cy="71" rx="6" ry="3.5" fill={BLUSH} opacity="0.45" />
      <ellipse cx="87" cy="71" rx="6" ry="3.5" fill={BLUSH} opacity="0.45" />
      {/* nose and mouth */}
      <path d="M56 67 L64 67 L60 72 Z" fill="#ff7a8a" stroke="#ff7a8a" strokeWidth="1.5" strokeLinejoin="round" />
      <path d="M60 72 q-3 5 -7 3 M60 72 q3 5 7 3" stroke={INK} strokeWidth="2.2" strokeLinecap="round" fill="none" />
      {/* whiskers */}
      <g stroke={FUR_DARK} strokeWidth="2" strokeLinecap="round">
        <path d="M24 66 L6 62" />
        <path d="M24 72 L6 74" />
        <path d="M96 66 L114 62" />
        <path d="M96 72 L114 74" />
      </g>
    </svg>
  );
}
