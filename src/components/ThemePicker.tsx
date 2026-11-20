import { useEffect, useRef, useState } from "react";
import { ChevronRight, ClipboardCopy, ClipboardPaste, Palette as PaletteIcon, RotateCcw } from "lucide-react";
import { type AppTheme, type ThemeColors, isHex, luminance, parseTheme, PRESET_ORDER, PRESETS, resolvePalette, themeJson } from "../themes";
import type { Settings as S } from "../settings";

/** Moonlight's base colours, so "Copy theme" and "Customize" work from the default too. */
const MOONLIGHT: Record<"dark" | "light", AppTheme> = {
  dark: { name: "Moonlight", mode: "dark", colors: { background: "#101026", sidebar: "#161634", surface: "#161634", text: "#f0f0ff", muted: "#9c9ccc", border: "#29295a", accent: "#ccccff", green: "#5ee0a0", red: "#ff7a8e", yellow: "#ffd27a", orange: "#ff9f5a", purple: "#d3a6ff" } },
  light: { name: "Moonlight", mode: "light", colors: { background: "#fdfdff", sidebar: "#f3f3fa", surface: "#f3f3fa", text: "#14143a", muted: "#545485", border: "#dcdcee", accent: "#3d3d85", green: "#0b6a3b", red: "#b51e3b", yellow: "#b07a00", orange: "#9a4a00", purple: "#7a3fa8" } },
};

/** The colours you can change by hand, in the order they show. */
const EDITABLE: [keyof ThemeColors, string][] = [
  ["background", "Background"], ["sidebar", "Sidebar"], ["text", "Text"], ["muted", "Secondary text"],
  ["accent", "Accent"], ["border", "Lines"], ["green", "Success"], ["red", "Error"], ["yellow", "Needs you"],
];

/** A tiny picture of the app in the theme's colours: sidebar, two rows, a button. */
function Swatch({ t }: { t: AppTheme }) {
  const c = t.colors;
  const side = c.sidebar ?? c.background;
  return (
    <span className="theme-swatch" aria-hidden="true" style={{ background: c.background }}>
      <span className="theme-swatch-side" style={{ background: side }}>
        <i style={{ background: c.accent }} />
        <i style={{ background: c.muted ?? c.text, opacity: 0.5 }} />
        <i style={{ background: c.muted ?? c.text, opacity: 0.5 }} />
      </span>
      <span className="theme-swatch-main">
        <i style={{ background: c.text, width: "70%" }} />
        <i style={{ background: c.muted ?? c.text, width: "50%", opacity: 0.7 }} />
        <b style={{ background: c.accent }} />
      </span>
    </span>
  );
}

/**
 * Settings, Appearance, Theme: Moonlight or a preset, your own theme, and sharing by copy and
 * paste. Agents can set one too with `nekotrail theme`.
 */
export function ThemePicker({ s, set, dark }: { s: S; set: <K extends keyof S>(k: K, v: S[K]) => void; dark: boolean }) {
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const active = resolvePalette(s.palette, s.customTheme);
  const current = active ?? MOONLIGHT[dark ? "dark" : "light"];
  const say = (ok: boolean, text: string) => { setMsg({ ok, text }); if (ok) setTimeout(() => setMsg((m) => (m?.text === text ? null : m)), 2500); };

  const pick = (id: string) => { set("palette", id); setMsg(null); };
  const copy = async () => {
    try { await navigator.clipboard.writeText(themeJson(current)); say(true, `Copied ${current.name}. Paste it anywhere to share it.`); }
    catch { say(false, "Couldn't reach the clipboard."); }
  };
  const paste = async () => {
    let text = "";
    try { text = await navigator.clipboard.readText(); } catch { return say(false, "Couldn't read the clipboard."); }
    const r = parseTheme(text);
    if ("error" in r) return say(false, r.error);
    set("customTheme", r.theme);
    set("palette", "custom");
    say(true, `${r.theme.name} is on.`);
  };
  /** Start your own from what's showing now, so the first edit changes one colour, not everything. */
  const custom = s.palette === "custom" && s.customTheme ? s.customTheme : null;
  const editColor = (k: keyof ThemeColors, v: string) => {
    const base = custom ?? { ...current, name: current.name === "Moonlight" ? "My theme" : `My ${current.name}`, colors: { ...current.colors } };
    const next: AppTheme = { ...base, colors: { ...base.colors, [k]: v } };
    if (k === "background") next.mode = luminance(v) > 0.4 ? "light" : "dark";
    set("customTheme", next);
    if (s.palette !== "custom") set("palette", "custom");
  };

  const choices: [string, AppTheme][] = [
    ["", MOONLIGHT[dark ? "dark" : "light"]],
    ...PRESET_ORDER.map((id) => [id, PRESETS[id]] as [string, AppTheme]),
    ...(s.customTheme ? [["custom", s.customTheme] as [string, AppTheme]] : []),
  ];
  const selected = Math.max(0, choices.findIndex(([id]) => id === s.palette));
  const grid = useRef<HTMLDivElement>(null);
  /** Radio group keys: arrows move and pick, like the system's own radio buttons. One Tab stop. */
  const onKey = (e: React.KeyboardEvent) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const next = (selected + step + choices.length) % choices.length;
    pick(choices[next][0]);
    requestAnimationFrame(() => grid.current?.querySelectorAll<HTMLButtonElement>(".theme-card")[next]?.focus());
  };

  return (
    <section>
      <div className="theme-head">
        <h3>Theme</h3>
        {!s.palette && (
          <div className="seg" role="radiogroup" aria-label="Moonlight light or dark">
            {([["system", "Follow system"], ["light", "Light"], ["dark", "Dark"]] as const).map(([id, name]) => (
              <button key={id} role="radio" aria-checked={s.theme === id} className={s.theme === id ? "on" : ""} onClick={() => set("theme", id)}>{name}</button>
            ))}
          </div>
        )}
      </div>
      <div className="theme-grid" role="radiogroup" aria-label="Theme" ref={grid} onKeyDown={onKey}>
        {choices.map(([id, t], i) => {
          const on = i === selected;
          return (
            <button key={id || "moonlight"} role="radio" aria-checked={on} tabIndex={on ? 0 : -1} className={"theme-card" + (on ? " on" : "")} onClick={() => pick(id)} title={t.name}>
              <Swatch t={t} />
              <span className="theme-name">{t.name || "Custom"}</span>
            </button>
          );
        })}
      </div>

      <div className="theme-actions">
        <button className="quiet" onClick={copy}><ClipboardCopy size={13} /> Copy theme</button>
        <button className="quiet" onClick={paste}><ClipboardPaste size={13} /> Paste theme</button>
        {s.palette && <button className="quiet" onClick={() => pick("")}><RotateCcw size={13} /> Back to Moonlight</button>}
        {msg && <span className={"theme-msg " + (msg.ok ? "ok" : "bad")} role="status">{msg.text}</span>}
      </div>

      <details className="theme-edit">
        <summary><ChevronRight size={12} className="theme-chevron" /><PaletteIcon size={13} /> Make your own</summary>
        <p className="muted setting-note">Change any colour. NekoTrail works out the rest and keeps text readable.</p>
        <div className="theme-colors">
          {EDITABLE.map(([k, label]) => {
            const v = (custom ?? current).colors[k] ?? "";
            return (
              <div key={k} className="theme-color">
                <input type="color" value={v || "#888888"} onChange={(e) => editColor(k, e.target.value)} aria-label={`${label} colour`} />
                <span>{label}</span>
                <HexInput value={v} label={label} onChange={(x) => editColor(k, x)} />
              </div>
            );
          })}
        </div>
        {custom && (
          <div className="setting">
            <span>Name</span>
            <input className="theme-name-input" value={custom.name} maxLength={40} onChange={(e) => set("customTheme", { ...custom, name: e.target.value })} />
          </div>
        )}
      </details>
      <p className="muted setting-note">Or ask any agent for one. It can run <code>nekotrail theme</code> to see the format and set it.</p>
    </section>
  );
}

/** A hex field you can type or paste into. A bad value goes back to the last good one when you leave it. */
function HexInput({ value, label, onChange }: { value: string; label: string; onChange: (v: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  const commit = () => {
    const t = text.trim().startsWith("#") ? text.trim() : `#${text.trim()}`;
    if (isHex(t)) { if (t.toLowerCase() !== value) onChange(t.toLowerCase()); }
    else setText(value);
  };
  return (
    <input className="theme-hex" value={text} placeholder="auto" spellCheck={false} aria-label={`${label} hex`}
      onChange={(e) => setText(e.target.value)} onBlur={commit} onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />
  );
}
