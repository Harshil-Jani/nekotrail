import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown } from "lucide-react";
import { menuArrows, useMenuPlace } from "../menuPlace";
import { useDismiss } from "../useDismiss";

export type MenuOption<T extends string> = { value: T; label: string; icon?: ReactNode; group?: string };

/**
 * A select in the app's own menu style (the same pop-up as the agent and permissions pickers), in
 * place of the system one. Up and Down open it and move through it, Enter picks, Esc closes it and
 * leaves the sheet around it open. `group` starts a labelled block (e.g. "Not installed").
 */
export function MenuSelect<T extends string>({ value, options, onChange, label, className, icon, placeholder }: {
  value: T;
  options: MenuOption<T>[];
  onChange: (v: T) => void;
  /** Read out with the current choice, e.g. "Agent: Claude". */
  label: string;
  className?: string;
  icon?: ReactNode;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const btn = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const root = useDismiss<HTMLSpanElement>(open, () => setOpen(false));
  useMenuPlace(menu, open);
  useEffect(() => {
    if (!open) return;
    (menu.current?.querySelector<HTMLElement>("[aria-checked=true]") ?? menu.current?.querySelector<HTMLElement>("[role^=menuitem]"))?.focus();
  }, [open]);
  const cur = options.find((o) => o.value === value);
  const pick = (v: T) => { setOpen(false); btn.current?.focus(); if (v !== value) onChange(v); };
  let group: string | undefined;
  return (
    <span className={`mselect${className ? ` ${className}` : ""}`} ref={root}>
      <button ref={btn} type="button" className="picker-btn mselect-btn" aria-haspopup="menu" aria-expanded={open} aria-label={`${label}: ${cur?.label ?? placeholder ?? ""}`}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => { if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !open) { e.preventDefault(); setOpen(true); } }}>
        {cur?.icon ?? icon}
        <span className="picker-label">{cur?.label ?? placeholder}</span>
        <ChevronDown size={12} aria-hidden />
      </button>
      {open && (
        <div ref={menu} className="menu nw-menu mselect-menu" role="menu" aria-label={label} onClick={(e) => e.preventDefault()}
          onKeyDown={(e) => {
            if (menuArrows(e)) return;
            if (e.key === "Tab") setOpen(false);
          }}>
          {options.map((o) => {
            const head = o.group !== group ? o.group : undefined;
            group = o.group;
            return (
              <span key={o.value} className="mselect-row">
                {head && <div className="menu-label">{head}</div>}
                <button type="button" role="menuitemradio" aria-checked={o.value === value} onClick={() => pick(o.value)}>
                  {o.value === value ? <Check size={14} /> : <span className="check-space" />}
                  {o.icon}
                  <span className="pm-label">{o.label}</span>
                </button>
              </span>
            );
          })}
        </div>
      )}
    </span>
  );
}
