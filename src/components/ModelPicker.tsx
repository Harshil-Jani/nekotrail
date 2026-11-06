import { useEffect, useMemo, useRef, useState } from "react";
import { Brain, Check, ChevronDown, ChevronRight, Sparkles } from "lucide-react";
import { api, type Agent, type ModelCatalog } from "../api";
import { keyHint, mod } from "../platform";
import { AGENT_INFO } from "../agents";

let cached: Promise<ModelCatalog> | null = null;
/** The model catalog, fetched once per launch. */
export const loadCatalog = () => (cached ??= api.modelCatalog().catch(() => ({}) as ModelCatalog));

export function useCatalog() {
  const [cat, setCat] = useState<ModelCatalog | null>(null);
  useEffect(() => { loadCatalog().then(setCat); }, []);
  return cat;
}

const NEW_FOR_MS = 30 * 24 * 3600_000;
/**
 * "New" only for 30 days. A catalog `released` date (YYYY-MM-DD) wins; without one, the 30 days count
 * from the first launch that saw the model flagged new, remembered in localStorage.
 */
function isNew(m: { id: string; new?: boolean; released?: string }) {
  const now = Date.now();
  if (m.released) return now - Date.parse(m.released) < NEW_FOR_MS;
  if (!m.new) return false;
  let seen: Record<string, number> = {};
  try { seen = JSON.parse(localStorage.getItem("nekotrail.modelsSeen") || "{}"); } catch { /* reset below */ }
  if (!seen[m.id]) { seen[m.id] = now; localStorage.setItem("nekotrail.modelsSeen", JSON.stringify(seen)); }
  return now - seen[m.id] < NEW_FOR_MS;
}

/** Newest model of each family first; everything else folds under "Older versions". */
function split(models: ModelCatalog[Agent]["models"]) {
  const seen = new Set<string>();
  const top: typeof models = [], older: typeof models = [];
  for (const m of models) {
    const family = m.label.split(" ")[0];
    if (!seen.has(family)) { seen.add(family); top.push(m); } else older.push(m);
  }
  return { top, older };
}

/**
 * One control for the model and its intelligence (effort) level, like the picker in Conductor:
 * "Opus 5.5 · High". Only levels the chosen model supports are offered.
 */
export function ModelPicker({
  agent, model, effort, onChange, compact = false,
}: {
  agent: Agent;
  model: string | null;
  effort: string | null;
  onChange: (next: { model: string | null; effort: string | null }) => void;
  compact?: boolean;
}) {
  const cat = useCatalog();
  const [open, setOpen] = useState(() => !compact && new URLSearchParams(location.search).has("picker"));
  const [showOlder, setShowOlder] = useState(false);
  const [custom, setCustom] = useState("");
  const root = useRef<HTMLSpanElement>(null);
  const a = cat?.[agent];
  // Aliases like "opus" or "sonnet" mean that family's newest model.
  const alias = model && a?.models.find((m) => m.label.toLowerCase().startsWith(model.toLowerCase()));
  const current = a?.models.find((m) => m.id === model)
    ?? (model ? { id: model, label: alias ? `${alias.label.split(" ")[0]} (latest)` : model, efforts: alias ? alias.efforts : a?.models[0]?.efforts ?? [] } : null);
  const defaultModel = a?.models.find((m) => m.id === a.default_model);
  const efforts = (current ?? defaultModel)?.efforts ?? [];
  const effortName = (e: string) => a?.effort_names[e] ?? e.charAt(0).toUpperCase() + e.slice(1);
  const { top, older } = useMemo(() => split(a?.models ?? []), [a]);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => { if (!root.current?.contains(e.target as Node)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("keydown", esc); };
  }, [open]);
  // ⌘/ opens the picker (Conductor's shortcut).
  useEffect(() => {
    if (compact) return;
    const onKey = (e: KeyboardEvent) => {
      if (mod(e) && e.key === "/" && !root.current?.closest(".pane")?.hasAttribute("hidden")) { e.preventDefault(); setOpen((o) => !o); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [compact]);

  const pick = (id: string | null) => {
    const m = a?.models.find((x) => x.id === id);
    // Keep the chosen level when the new model supports it.
    const keep = effort && (m ? m.efforts.includes(effort) : true) ? effort : null;
    onChange({ model: id, effort: keep });
  };

  // OpenCode's catalog is one "Default" entry (its own config picks the model): say it once, not "Default (default)".
  const plainDefault = (m?: { id: string; label: string } | null) => !!m && m.id === a?.default_model && m.label.toLowerCase() === "default";
  const label = `${plainDefault(current) || (!current && plainDefault(defaultModel)) ? "Default model" : current?.label ?? (defaultModel ? `${defaultModel.label} (default)` : "Default model")}${effort ? ` · ${effortName(effort)}` : ""}`;
  const row = (m: (typeof top)[number]) => (
    <button key={m.id} role="menuitemradio" aria-checked={m.id === model} onClick={() => pick(m.id)}>
      {m.id === model ? <Check size={14} /> : <span className="check-space" />}
      <span className="pm-label">{m.label}</span>
      {isNew(m) && <span className="new-tag">New</span>}
      {m.id === a?.default_model && <span className="muted">default</span>}
    </button>
  );

  return (
    <span className="model-picker" ref={root}>
      <button className="picker-btn" onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-haspopup="menu" title={keyHint("Pick model and intelligence (⌘/)")}>
        <Sparkles size={13} />
        <span className="picker-label">{label}</span>
        <ChevronDown size={12} />
      </button>
      {open && a && (
        <span className="menu picker-menu" role="menu">
          <div className="menu-label">{AGENT_INFO[agent]?.name ?? "Claude"} model</div>
          <button role="menuitemradio" aria-checked={!model} onClick={() => pick(null)}>
            {!model ? <Check size={14} /> : <span className="check-space" />}<span className="pm-label">Default</span>{!plainDefault(defaultModel) && <span className="muted">{defaultModel?.label}</span>}
          </button>
          {/* "Auto" as the default means the CLI picks, which is what Default already says: one row, not two. */}
          {top.filter((m) => !(m.id === "auto" && a.default_model === "auto" && model !== "auto") && !(plainDefault(m) && model !== m.id)).map(row)}
          {older.length > 0 && (
            <button className="older-toggle" onClick={() => setShowOlder((v) => !v)} aria-expanded={showOlder}>
              <ChevronRight size={13} style={{ transform: showOlder ? "rotate(90deg)" : undefined }} /><span className="pm-label">Older versions</span><span className="muted">{older.length}</span>
            </button>
          )}
          {showOlder && older.map(row)}
          <form className="custom-model" onSubmit={(e) => { e.preventDefault(); if (custom.trim()) { onChange({ model: custom.trim(), effort }); setCustom(""); } }}>
            <input value={custom} onChange={(e) => setCustom(e.target.value)} placeholder={keyHint("Other model ID, then ↵")} aria-label="Other model ID" />
          </form>
          {plainDefault(defaultModel) && (
            <p className="muted menu-hint">Default uses the model in {AGENT_INFO[agent]?.full ?? "the agent"}'s own config. To pick another, type its ID above.</p>
          )}
          {efforts.length > 0 && (
            <>
              <div className="menu-label"><Brain size={12} /> Intelligence</div>
              <div className="effort-seg" role="radiogroup" aria-label="Intelligence">
                <button role="radio" aria-checked={!effort} className={!effort ? "on" : ""} onClick={() => onChange({ model, effort: null })} title="The model's default">Auto</button>
                {efforts.map((e) => (
                  <button key={e} role="radio" aria-checked={effort === e} className={effort === e ? "on" : ""} onClick={() => onChange({ model, effort: e })}>{effortName(e)}</button>
                ))}
              </div>
              <p className="muted menu-hint">Higher thinks longer and costs more. Use it for hard bugs and big refactors.</p>
            </>
          )}
        </span>
      )}
    </span>
  );
}
