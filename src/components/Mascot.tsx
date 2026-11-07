/**
 * NekoTrail's mascot: the Starlight tabby, a silver loaf cat with indigo stripes whose tail ends in a
 * git merge (two branches joining at a commit). The commits are lantern-gold stars. Original artwork,
 * drawn here as SVG; scripts/brand.ts renders this same component into the brand kit and public/.
 */
import { useEffect, useRef, useState } from "react";

/**
 * Moods map to real app states, never decoration:
 * idle = nothing running, working = an agent is running (tail wags),
 * happy = something shipped, sleep = idle for a while or an empty screen,
 * worried = CI failing or a turn ended in an error.
 */
export type Mood = "idle" | "working" | "happy" | "sleep" | "worried";

/** The cat's own colours (brand/palette.json, group "mascot"). They stay the same in light and dark. */
export const CAT = {
  fur: "#ececff",
  stripe: "#3d3d85",
  ink: "#14143a",
  glint: "#ffffff",
  blush: "#ffb08a",
  nose: "#ff8fa0",
  paw: "#ffffff",
  tailStar: "#ffd27a",
  /** Ribbon edge on the parcel: the lantern's light-mode fill, so the bow reads on silver. */
  ribbonEdge: "#b07a00",
} as const;

/** Outline width in drawing units. Thicker below 20px so the silver cat keeps its edge on light rows. */
export const outlineFor = (size: number) => (size < 20 ? 3.2 : 2.6);

function Eyes({ mood, look }: { mood: Mood; look: { x: number; y: number } }) {
  if (mood === "sleep") {
    return (
      <g stroke={CAT.ink} strokeWidth="3" strokeLinecap="round" fill="none">
        <path d="M37 58 q6 5 12 0" />
        <path d="M71 58 q6 5 12 0" />
      </g>
    );
  }
  if (mood === "happy") {
    return (
      <g stroke={CAT.ink} strokeWidth="3.2" strokeLinecap="round" fill="none">
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
        <g stroke={CAT.ink} strokeWidth="2.6" strokeLinecap="round">
          <path d="M36 44 L48 47" />
          <path d="M84 44 L72 47" />
        </g>
      )}
      <ellipse cx={43 + dx} cy={57 + dy} rx="6.5" ry={mood === "worried" ? 6.5 : 8} fill={CAT.ink} />
      <ellipse cx={77 + dx} cy={57 + dy} rx="6.5" ry={mood === "worried" ? 6.5 : 8} fill={CAT.ink} />
      <circle cx={45.5 + dx} cy={53.5 + dy} r="2.4" fill={CAT.glint} />
      <circle cx={79.5 + dx} cy={53.5 + dy} r="2.4" fill={CAT.glint} />
    </g>
  );
}

/** The update sheet's cat holds a parcel: the box sits over the tucked paws, paws resting on top. */
function Parcel({ ol }: { ol: number }) {
  return (
    <>
      <g className="parcel">
        <rect x="40" y="88" width="40" height="27" rx="3" fill={CAT.paw} stroke={CAT.stripe} strokeWidth={ol * 0.8} />
        <rect x="37" y="82" width="46" height="9" rx="2.5" fill={CAT.fur} stroke={CAT.stripe} strokeWidth={ol * 0.8} />
        <rect x="56.5" y="82" width="7" height="33" fill={CAT.tailStar} />
        <path d="M60 82 C 52 72, 44 78, 52 82 Z" fill={CAT.tailStar} stroke={CAT.ribbonEdge} strokeWidth="1.5" strokeLinejoin="round" />
        <path d="M60 82 C 68 72, 76 78, 68 82 Z" fill={CAT.tailStar} stroke={CAT.ribbonEdge} strokeWidth="1.5" strokeLinejoin="round" />
        <circle cx="60" cy="82" r="2.6" fill={CAT.ribbonEdge} />
      </g>
      <ellipse cx="38" cy="98" rx="8" ry="6.5" fill={CAT.paw} stroke={CAT.stripe} strokeWidth="1.6" />
      <ellipse cx="82" cy="98" rx="8" ry="6.5" fill={CAT.paw} stroke={CAT.stripe} strokeWidth="1.6" />
    </>
  );
}

export function Mascot({
  size = 48,
  mood = "idle",
  title = "NekoTrail cat",
  follow = false,
  purring = false,
  parcel = false,
  onClick,
}: {
  size?: number;
  mood?: Mood;
  title?: string;
  /** Eyes follow the mouse pointer. */
  follow?: boolean;
  purring?: boolean;
  /** Holding a parcel (the update sheet's art). */
  parcel?: boolean;
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

  const ol = outlineFor(size);
  return (
    <svg
      ref={ref}
      width={size}
      height={size}
      viewBox="0 0 140 120"
      role="img"
      aria-label={title}
      className={`mascot mascot-${parcel ? "parcel" : mood} ${purring ? "purring" : ""} ${onClick ? "pettable" : ""}`}
      onClick={onClick}
    >
      {/* merge tail: two branches join at a commit; the commits are lantern stars */}
      <g className="mascot-tail">
        <g stroke={CAT.stripe} strokeWidth="7" strokeLinecap="round" fill="none">
          <path d="M100 100 C 122 100, 128 84, 124 66" />
          <path d="M124 66 C 121 54, 112 50, 112 40" />
          <path d="M124 66 C 128 56, 134 52, 134 42" />
        </g>
        <circle cx="124" cy="66" r="6" fill={CAT.tailStar} stroke={CAT.stripe} strokeWidth="3" />
        <circle cx="112" cy="38" r="5" fill={CAT.tailStar} stroke={CAT.stripe} strokeWidth="3" />
        <circle cx="134" cy="40" r="5" fill={CAT.tailStar} stroke={CAT.stripe} strokeWidth="3" />
      </g>
      {/* ears */}
      <path d="M22 44 L30 12 Q32 8 36 11 L55 30 Z" fill={CAT.fur} stroke={CAT.stripe} strokeWidth={ol} strokeLinejoin="round" />
      <path d="M98 44 L90 12 Q88 8 84 11 L65 30 Z" fill={CAT.fur} stroke={CAT.stripe} strokeWidth={ol} strokeLinejoin="round" />
      <path d="M30 36 L34 19 L45 30 Z" fill={CAT.blush} opacity="0.6" />
      <path d="M90 36 L86 19 L75 30 Z" fill={CAT.blush} opacity="0.6" />
      {/* loaf body */}
      <path d="M14 70 C 14 36, 36 26, 60 26 C 84 26, 106 36, 106 70 L106 92 C 106 104, 98 110, 86 110 L34 110 C 22 110, 14 104, 14 92 Z" fill={CAT.fur} stroke={CAT.stripe} strokeWidth={ol} strokeLinejoin="round" />
      {/* tabby stripes: forehead and flanks */}
      <g stroke={CAT.stripe} strokeWidth="3.5" strokeLinecap="round">
        <path d="M60 31 L60 41" />
        <path d="M51 33 L53 41" />
        <path d="M69 33 L67 41" />
        <path d="M15 80 L24 82" />
        <path d="M15 88 L23 89" />
        <path d="M105 80 L96 82" />
        <path d="M105 88 L97 89" />
      </g>
      {/* tucked paws */}
      {!parcel && (
        <>
          <ellipse cx="44" cy="108" rx="11" ry="6" fill={CAT.paw} stroke={CAT.stripe} strokeWidth="1.6" />
          <ellipse cx="76" cy="108" rx="11" ry="6" fill={CAT.paw} stroke={CAT.stripe} strokeWidth="1.6" />
        </>
      )}
      <Eyes mood={mood} look={look} />
      <ellipse cx="33" cy="71" rx="6" ry="3.5" fill={CAT.blush} opacity="0.6" />
      <ellipse cx="87" cy="71" rx="6" ry="3.5" fill={CAT.blush} opacity="0.6" />
      {/* nose and mouth */}
      <path d="M56 67 L64 67 L60 72 Z" fill={CAT.nose} stroke={CAT.nose} strokeWidth="1.5" strokeLinejoin="round" />
      <path d="M60 72 q-3 5 -7 3 M60 72 q3 5 7 3" stroke={CAT.ink} strokeWidth="2.2" strokeLinecap="round" fill="none" />
      {/* whiskers */}
      <g stroke={CAT.stripe} strokeWidth="2" strokeLinecap="round" opacity="0.9">
        <path d="M24 66 L6 62" />
        <path d="M24 72 L6 74" />
        <path d="M96 66 L114 62" />
        <path d="M96 72 L114 74" />
      </g>
      {parcel && <Parcel ol={ol} />}
    </svg>
  );
}
