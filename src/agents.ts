import type { Agent } from "./api";

/**
 * The agent CLIs nekotrail drives, in the order the UI lists them. The backend twin is `ALL` in
 * src-tauri/src/agent.rs; adding an agent means one entry here, one struct there, and its models
 * in models.json.
 */
export const AGENTS: Agent[] = ["claude", "codex", "gemini"];

type AgentInfo = {
  /** Short name for badges and buttons. */
  name: string;
  /** The product name, for setup and errors. */
  full: string;
  install: string;
  /** Run in the workspace terminal to sign in. */
  login: string;
  /** Has "Ask before changes" (approvals mid-turn). */
  ask: boolean;
  /** Has an effort (thinking) level nekotrail can set. */
  effort: boolean;
};

export const AGENT_INFO: Record<Agent, AgentInfo> = {
  claude: { name: "Claude", full: "Claude Code", install: "npm install -g @anthropic-ai/claude-code", login: "claude /login", ask: true, effort: true },
  codex: { name: "Codex", full: "Codex", install: "npm install -g @openai/codex", login: "codex login", ask: false, effort: true },
  gemini: { name: "Gemini", full: "Gemini CLI", install: "npm install -g @google/gemini-cli", login: "gemini", ask: false, effort: false },
};

/** Display name for a stored agent id. Unknown ids are Claude, as in the backend. */
export const agentName = (a?: string | null) => AGENT_INFO[a as Agent]?.name ?? "Claude";

/** The agent to hand over to (limits, second opinions): Claude and Codex swap, Gemini goes to Claude. */
export const otherAgent = (a?: string | null): Agent => (a === "codex" || a === "gemini" ? "claude" : "codex");
