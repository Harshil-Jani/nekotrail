import { Check } from "lucide-react";
import type { Agent } from "../api";
import type { AgentSplit } from "../agents";
import { AgentLogo } from "./AgentLogo";

/**
 * The "More agents" list shared by the new-workspace picker and the chat + menu: other installed
 * agents first, then the ones not on this Mac (picking one still works; its first message shows
 * how to install it). `known` false: we don't know yet what's installed, so no headings.
 * With `current` set, rows get a tick column and the current agent is ticked.
 */
export function AgentMenuItems({ split, known, pick, label, current, size = 14 }: { split: AgentSplit; known: boolean; pick: (a: Agent) => void; label: (a: Agent) => string; current?: Agent | "both" | null; size?: number }) {
  const ticks = current !== undefined;
  const item = (a: Agent) => (
    <button key={a} role={ticks ? "menuitemradio" : "menuitem"} aria-checked={ticks ? current === a : undefined} onClick={() => pick(a)}>
      {ticks && (current === a ? <Check size={14} /> : <span className="check-space" />)}
      <AgentLogo agent={a} size={size} />
      <span>{label(a)}</span>
    </button>
  );
  if (!known) return <>{[...split.installed, ...split.missing].map(item)}</>;
  return (
    <>
      {split.installed.length > 0 && <div className="menu-label">Installed</div>}
      {split.installed.map(item)}
      {split.missing.length > 0 && <div className="menu-label">Not installed</div>}
      {split.missing.map(item)}
    </>
  );
}
