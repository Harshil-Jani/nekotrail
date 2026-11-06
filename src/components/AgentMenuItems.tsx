import type { Agent } from "../api";
import type { AgentSplit } from "../agents";

/**
 * The "More agents" list shared by the new-workspace picker and the chat + menu: other installed
 * agents first, then the ones not on this Mac (picking one still works; its first message shows
 * how to install it). `known` false: we don't know yet what's installed, so no headings.
 */
export function AgentMenuItems({ split, known, pick, label }: { split: AgentSplit; known: boolean; pick: (a: Agent) => void; label: (a: Agent) => string }) {
  const item = (a: Agent) => (
    <button key={a} role="menuitem" onClick={() => pick(a)}><span className="menu-icon-space" aria-hidden />{label(a)}</button>
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
