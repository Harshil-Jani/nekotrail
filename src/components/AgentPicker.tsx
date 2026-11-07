import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import type { Agent } from "../api";
import { AGENT_INFO, type AgentSplit } from "../agents";
import { AgentLogo } from "./AgentLogo";
import { AgentMenuItems } from "./AgentMenuItems";
import { menuArrows, useMenuPlace } from "../menuPlace";

export type AgentChoice = Agent | "both";

const nameOf = (a: AgentChoice) => (a === "both" ? "Claude + Codex" : AGENT_INFO[a].name);

/** The chosen agent's mark; "both" shows Claude's and Codex's side by side. */
function Mark({ agent }: { agent: AgentChoice }) {
  if (agent !== "both") return <AgentLogo agent={agent} />;
  return <span className="agent-pair" aria-hidden><AgentLogo agent="claude" /><AgentLogo agent="codex" /></span>;
}

/**
 * One button for the agent ("Claude ▾"), and a menu with the agents up front, the Claude + Codex
 * bake-off, then every other agent, installed first. `only` limits the list (a task across repos
 * offers just the agents that can open several folders).
 */
export function AgentPicker({ agent, onChange, split, known, only }: {
  agent: AgentChoice;
  onChange: (a: AgentChoice) => void;
  split: AgentSplit;
  known: boolean;
  only?: Agent[];
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLSpanElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const btn = useRef<HTMLButtonElement>(null);
  useMenuPlace(menu, open);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => { if (!root.current?.contains(e.target as Node)) setOpen(false); };
    window.addEventListener("mousedown", away);
    // Focus the ticked row, so Up and Down start from where you are.
    (menu.current?.querySelector<HTMLElement>("[aria-checked=true]") ?? menu.current?.querySelector<HTMLElement>("[role^=menuitem]"))?.focus();
    return () => window.removeEventListener("mousedown", away);
  }, [open]);

  const pick = (a: AgentChoice) => { setOpen(false); onChange(a); btn.current?.focus(); };
  const row = (a: AgentChoice, why?: string) => (
    <button key={a} role="menuitemradio" aria-checked={agent === a} onClick={() => pick(a)}>
      {agent === a ? <Check size={14} /> : <span className="check-space" />}
      <Mark agent={a} />
      <span>{nameOf(a)}{why && <small className="why">{why}</small>}</span>
    </button>
  );

  return (
    <span className="nw-agent-picker" ref={root}>
      <button ref={btn} className="nw-agent-btn" aria-haspopup="menu" aria-expanded={open} aria-label={`Agent: ${nameOf(agent)}`}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => { if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !open) { e.preventDefault(); setOpen(true); } }}
        title={agent === "both" ? "Runs Claude and Codex side by side, then you keep the better one" : AGENT_INFO[agent].full}>
        <Mark agent={agent} />
        <span className="nw-agent-name">{nameOf(agent)}</span>
        <ChevronDown size={14} aria-hidden />
      </button>
      {open && (
        <div ref={menu} className="menu nw-menu nw-agent-menu" role="menu" aria-label="Agent"
          onKeyDown={(e) => {
            if (menuArrows(e)) return;
            if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setOpen(false); btn.current?.focus(); }
            if (e.key === "Tab") setOpen(false);
          }}>
          {only ? (
            <>
              <div className="menu-label">Can work across repos</div>
              {only.map((a) => row(a))}
            </>
          ) : (
            <>
              {split.shown.map((a) => row(a))}
              {row("both", "Same task in two workspaces, keep the better one")}
              {(split.installed.length > 0 || split.missing.length > 0) && !known && <div className="menu-sep" />}
              {(split.installed.length > 0 || split.missing.length > 0) && (
                <AgentMenuItems split={split} known={known} current={agent} pick={pick} label={(a) => AGENT_INFO[a].name} />
              )}
            </>
          )}
        </div>
      )}
    </span>
  );
}
