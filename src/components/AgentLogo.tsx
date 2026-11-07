import type { Agent } from "../api";
import { AGENT_INFO } from "../agents";
import { AGENT_PATHS } from "../agentLogos";

/** An agent's mark: its brand icon when one is free to use, else its first letter in a small tile. */
export function AgentLogo({ agent, size = 14, className }: { agent: Agent; size?: number; className?: string }) {
  const d = AGENT_PATHS[agent];
  if (d) return <svg className={`agent-logo${className ? ` ${className}` : ""}`} width={size} height={size} viewBox="0 0 24 24" aria-hidden><path d={d} fill="currentColor" /></svg>;
  const letter = (AGENT_INFO[agent]?.name ?? "?").charAt(0).toUpperCase();
  return <span className={`agent-logo agent-mono${className ? ` ${className}` : ""}`} style={{ width: size, height: size, fontSize: Math.round(size * 0.64) }} aria-hidden>{letter}</span>;
}
