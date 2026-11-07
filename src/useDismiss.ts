import { useEffect, useRef } from "react";

/** The one pop-up that is open right now, so opening another closes it first. */
let current: { close: () => void } | null = null;

/** Closes whatever pop-up menu is open (for code that opens a sheet or switches view). */
export function closeOpenMenu() {
  current?.close();
}

/**
 * How every pop-up menu goes away: a press anywhere outside it, Esc (focus goes back to the button
 * that opened it), or another menu opening. Put the returned ref on the element that holds both the
 * button and the menu, so a press on the button itself still toggles.
 *
 * Menus used to close only when the mouse left them, so one opened from the keyboard, or left
 * behind by a quick move, stayed up over the tabs and ate the next click.
 */
export function useDismiss<T extends HTMLElement = HTMLElement>(open: boolean, close: () => void) {
  const root = useRef<T>(null);
  const closeRef = useRef(close);
  closeRef.current = close;

  useEffect(() => {
    if (!open) return;
    const me = { close: () => closeRef.current() };
    if (current && current !== me) current.close();
    current = me;
    const trigger = () => root.current?.querySelector<HTMLElement>("[aria-expanded]") ?? root.current?.querySelector<HTMLElement>("button");
    const away = (e: PointerEvent) => {
      const t = e.target as Node | null;
      if (t && root.current?.contains(t)) return;
      me.close();
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      // A confirm box opened from inside the menu takes Esc for itself.
      const modal = (document.activeElement as HTMLElement | null)?.closest("[aria-modal=true]");
      if (modal && !modal.contains(root.current)) return;
      e.preventDefault();
      e.stopPropagation();
      me.close();
      trigger()?.focus();
    };
    document.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", esc, true);
    return () => {
      document.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", esc, true);
      if (current === me) current = null;
    };
  }, [open]);

  return root;
}

/** A sheet keeps Tab inside it: from the last control back to the first, and the other way round. */
export function keepTabInside(e: React.KeyboardEvent<HTMLElement>) {
  if (e.key !== "Tab") return;
  const all = [...e.currentTarget.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), textarea, select, a[href], [tabindex]:not([tabindex='-1'])")]
    .filter((el) => el.tabIndex >= 0 && el.offsetParent !== null);
  if (!all.length) { e.preventDefault(); return; }
  const first = all[0], last = all[all.length - 1];
  const at = document.activeElement as HTMLElement | null;
  if (!e.shiftKey && (at === last || !e.currentTarget.contains(at))) { e.preventDefault(); first.focus(); }
  else if (e.shiftKey && (at === first || !e.currentTarget.contains(at))) { e.preventDefault(); last.focus(); }
}
