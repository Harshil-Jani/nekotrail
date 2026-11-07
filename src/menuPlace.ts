import { useLayoutEffect, type RefObject } from "react";

const GAP = 6, EDGE = 8;

/**
 * Keeps an open pop-up menu inside the window. It opens on its usual side when it fits there; if
 * not, on the side with more room, and scrolls instead of running off the edge. Sideways it slides
 * back in. Runs before paint, so the menu never flashes in the wrong place.
 */
export function useMenuPlace(menu: RefObject<HTMLElement | null>, open: boolean, deps: unknown[] = []) {
  useLayoutEffect(() => {
    const el = menu.current;
    const anchor = el?.parentElement;
    if (!open || !el || !anchor) return;
    const place = () => {
      el.style.top = el.style.bottom = el.style.maxHeight = el.style.left = el.style.right = "";
      const a = anchor.getBoundingClientRect();
      const m = el.getBoundingClientRect();
      const below = window.innerHeight - a.bottom - GAP - EDGE;
      const above = a.top - GAP - EDGE;
      const wantsUp = m.bottom <= a.top + 1;
      const up = wantsUp ? m.height <= above || above >= below : m.height > below && above > below;
      if (up) { el.style.top = "auto"; el.style.bottom = `calc(100% + ${GAP}px)`; }
      else { el.style.bottom = "auto"; el.style.top = `calc(100% + ${GAP}px)`; }
      const room = Math.floor(up ? above : below);
      if (m.height > room) el.style.maxHeight = `${room}px`;
      const r = el.getBoundingClientRect();
      if (r.right > window.innerWidth - EDGE) { el.style.left = "auto"; el.style.right = "0"; }
      else if (r.left < EDGE) { el.style.right = "auto"; el.style.left = "0"; }
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, ...deps]);
}

/** Up, Down, Home and End move focus between a menu's items, wrapping at the ends. */
export function menuArrows(e: React.KeyboardEvent<HTMLElement>) {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return false;
  const items = [...e.currentTarget.querySelectorAll<HTMLElement>("[role^=menuitem]:not(:disabled)")];
  if (!items.length) return false;
  e.preventDefault();
  const at = items.indexOf(document.activeElement as HTMLElement);
  const next = e.key === "Home" ? 0 : e.key === "End" ? items.length - 1
    : e.key === "ArrowDown" ? (at + 1) % items.length : (at - 1 + items.length) % items.length;
  items[next].focus();
  return true;
}
