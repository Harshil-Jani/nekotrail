import type { Agent, GuardLevel, McpAgent } from "./api";

/**
 * The agent CLIs nekotrail drives, in the order the UI lists them. The backend twin is `ALL` in
 * src-tauri/src/agent.rs; adding an agent means one entry here, one there (an `AcpAgent` entry in
 * acp.rs for agents that speak ACP), and its models in models.json.
 */
export const AGENTS: Agent[] = ["claude", "codex", "gemini", "opencode", "cursor", "copilot", "goose", "qwen", "kimi", "auggie", "grok", "kiro", "droid", "vibe", "cline", "kilo"];

/** The agents MCP server and skill sharing covers (src-tauri/src/mcpshare.rs). */
export const MCP_AGENTS: McpAgent[] = ["claude", "codex", "gemini", "opencode"];

/** Always offered in the pickers, installed or not; the rest show up once installed or under "More agents". */
export const CORE_AGENTS: Agent[] = ["claude", "codex"];

type AgentInfo = {
  /** Short name for badges and buttons. */
  name: string;
  /** The product name, for setup and errors. */
  full: string;
  /** The command it runs as in a terminal. */
  cli: string;
  install: string;
  /** Run in the workspace terminal to sign in. */
  login: string;
  /** What setup tells you to run once after installing. */
  setup: string;
  /** Has "Ask before changes" (approvals mid-turn). */
  ask: boolean;
  /** Has an effort (thinking) level nekotrail can set. */
  effort: boolean;
  /** Can run with an untrusted repo config switched off. False: "Not now" means it doesn't run in that repo. */
  configOff: boolean;
  /** How protected paths hold (twin of `Agent::guard_level`), in plain words for Repo settings. */
  guard: { level: GuardLevel; how: string };
};

export const AGENT_INFO: Record<Agent, AgentInfo> = {
  claude: { name: "Claude", full: "Claude Code", cli: "claude", install: "npm install -g @anthropic-ai/claude-code", login: "claude /login", setup: "claude", ask: true, effort: true, configOff: true,
    guard: { level: "blocked", how: "Can't edit them with its file tools or simple shell writes, in every mode. A script could still get around that, so each turn is checked too." } },
  codex: { name: "Codex", full: "Codex", cli: "codex", install: "npm install -g @openai/codex", login: "codex login", setup: "codex", ask: false, effort: true, configOff: true,
    guard: { level: "after", how: "Codex has no way to block single files yet, so each turn is checked when it ends." } },
  gemini: { name: "Gemini", full: "Gemini CLI", cli: "gemini", install: "npm install -g @google/gemini-cli", login: "gemini", setup: "gemini", ask: false, effort: false, configOff: false,
    guard: { level: "after", how: "Checked when each turn ends." } },
  opencode: { name: "OpenCode", full: "OpenCode", cli: "opencode", install: "curl -fsSL https://opencode.ai/install | bash", login: "opencode auth login", setup: "opencode auth login", ask: true, effort: false, configOff: true,
    guard: { level: "blocked", how: "Its edits to them are refused in every mode, with the reason in the chat. Shell commands aren't, so each turn is checked too." } },
  cursor: { name: "Cursor", full: "Cursor CLI", cli: "cursor-agent", install: "curl https://cursor.com/install -fsS | bash", login: "cursor-agent login", setup: "cursor-agent login", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  copilot: { name: "Copilot", full: "GitHub Copilot CLI", cli: "copilot", install: "npm install -g @github/copilot", login: "copilot login", setup: "copilot login", ask: true, effort: false, configOff: false,
    guard: { level: "asks", how: "In Ask before changes it asks before every write, and protected ones are refused. In other modes it may write without asking, so each turn is checked when it ends too." } },
  goose: { name: "Goose", full: "Goose", cli: "goose", install: "brew install block-goose-cli", login: "goose configure", setup: "goose configure", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  qwen: { name: "Qwen", full: "Qwen Code", cli: "qwen", install: "npm install -g @qwen-code/qwen-code", login: "qwen", setup: "qwen", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  kimi: { name: "Kimi", full: "Kimi Code CLI", cli: "kimi", install: "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash", login: "kimi", setup: "kimi", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  auggie: { name: "Auggie", full: "Auggie CLI", cli: "auggie", install: "npm install -g @augmentcode/auggie", login: "auggie login", setup: "auggie login", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  grok: { name: "Grok", full: "Grok Build", cli: "grok", install: "npm install -g @xai-official/grok", login: "grok login", setup: "grok login", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  kiro: { name: "Kiro", full: "Kiro CLI", cli: "kiro-cli", install: "curl -fsSL https://cli.kiro.dev/install | bash", login: "kiro-cli login", setup: "kiro-cli login", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  droid: { name: "Droid", full: "Factory Droid", cli: "droid", install: "curl -fsSL https://app.factory.ai/cli | sh", login: "droid", setup: "droid", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  vibe: { name: "Vibe", full: "Mistral Vibe", cli: "vibe", install: "curl -LsSf https://mistral.ai/vibe/install.sh | bash", login: "vibe --setup", setup: "vibe --setup", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  cline: { name: "Cline", full: "Cline CLI", cli: "cline", install: "npm install -g cline", login: "cline auth", setup: "cline auth", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
  kilo: { name: "Kilo", full: "Kilo CLI", cli: "kilo", install: "npm install -g @kilocode/cli", login: "kilo auth login", setup: "kilo auth login", ask: false, effort: false, configOff: false,
    guard: { level: "asks", how: "If the agent asks before changing a protected file, nekotrail says no. It often edits without asking, so each turn is checked when it ends too." } },
};

/** Display name for a stored agent id. Unknown ids are Claude, as in the backend. */
export const agentName = (a?: string | null) => AGENT_INFO[a as Agent]?.name ?? "Claude";

/** The agent to hand over to (limits, second opinions): Claude and Codex swap, the others go to Claude. */
export const otherAgent = (a?: string | null): Agent => (a === "claude" || !AGENT_INFO[a as Agent] ? "codex" : "claude");

export type AgentSplit = { shown: Agent[]; installed: Agent[]; missing: Agent[] };

/**
 * How a picker lays out sixteen agents. Up front (`shown`, at most `max`, in `AGENTS` order):
 * Claude and Codex, then installed agents, and always `keep` (the one already chosen). Under
 * "More agents": the other installed ones, then the ones not installed. `installed` null (not
 * known yet) counts nothing as installed.
 */
export function splitAgents(installed: Agent[] | null, keep?: Agent | null, max = 4): AgentSplit {
  const inst = new Set(installed ?? []);
  let front = AGENTS.filter((a) => CORE_AGENTS.includes(a) || inst.has(a)).slice(0, max);
  if (keep && AGENT_INFO[keep] && !front.includes(keep)) front = [...front.slice(0, max - 1), keep];
  const shown = AGENTS.filter((a) => front.includes(a));
  const rest = AGENTS.filter((a) => !front.includes(a));
  return { shown, installed: rest.filter((a) => inst.has(a)), missing: rest.filter((a) => !inst.has(a)) };
}

/** Short label for how protected paths hold for an agent. */
export const GUARD_TAG: Record<GuardLevel, string> = { blocked: "Blocked", asks: "When it asks", after: "Checked after" };

/** Agents that hold protected paths the same way, together, strongest first: one row each in Repo settings. */
export function guardGroups(): { agents: Agent[]; level: GuardLevel; how: string }[] {
  const rank: Record<GuardLevel, number> = { blocked: 0, asks: 1, after: 2 };
  const groups: { agents: Agent[]; level: GuardLevel; how: string }[] = [];
  for (const a of AGENTS) {
    const { level, how } = AGENT_INFO[a].guard;
    const g = groups.find((x) => x.level === level && x.how === how);
    if (g) g.agents.push(a);
    else groups.push({ agents: [a], level, how });
  }
  return groups.sort((x, y) => rank[x.level] - rank[y.level]);
}
