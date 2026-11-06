import { useLayoutEffect, useRef, useState } from "react";

/**
 * How far a bar has to fold up to fit its width: 0 shows everything, each step up hides a bit more
 * (labels become icons, items move into a menu). Steps are tried before paint, so nothing clips or
 * flickers. `tight` says whether the current layout still doesn't fit; `deps` are what changes the
 * bar's content (buttons that come and go), since that changes how much room it needs.
 */
export function useFit(max: number, tight: (el: HTMLElement) => boolean, deps: unknown[]) {
  const [el, setEl] = useState<HTMLElement | null>(null);
  const [width, setWidth] = useState(0);
  const [level, setLevel] = useState(0);
  const seen = useRef<{ width: number; key: string }>({ width: -1, key: "" });

  useLayoutEffect(() => {
    if (!el) return;
    setWidth(el.clientWidth);
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);

  const key = JSON.stringify(deps);
  useLayoutEffect(() => {
    if (!el || !width) return;
    // A new width or new content: start from the full bar and fold again.
    if (seen.current.width !== width || seen.current.key !== key) {
      seen.current = { width, key };
      if (level !== 0) { setLevel(0); return; }
    }
    if (level < max && tight(el)) setLevel(level + 1);
  });

  return [setEl, level] as const;
}

/** True when the element's content is wider than the element (it would clip or scroll). */
export const overflows = (el: HTMLElement) => el.scrollWidth > el.clientWidth + 1;
