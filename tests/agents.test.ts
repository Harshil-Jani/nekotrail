// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { AGENTS, AGENT_INFO, MCP_AGENTS, otherAgent, splitAgents } from "../src/agents";
import catalog from "../src-tauri/src/models.json";

describe("agent registry", () => {
  test("every agent has its info and a model catalog entry", () => {
    expect(AGENTS.length).toBe(16);
    expect(new Set(AGENTS).size).toBe(AGENTS.length);
    for (const a of AGENTS) {
      const i = AGENT_INFO[a];
      expect(i.name && i.full && i.cli && i.install && i.login && i.setup).toBeTruthy();
      expect((catalog as Record<string, { default_model: string }>)[a]?.default_model).toBeTruthy();
    }
    expect(Object.keys(catalog).sort()).toEqual([...AGENTS].sort());
    // Cursor runs as cursor-agent: `agent` is Grok Build's binary too.
    expect(AGENTS.map((a) => AGENT_INFO[a].cli)).not.toContain("agent");
    expect(MCP_AGENTS).toEqual(["claude", "codex", "gemini", "opencode"]);
  });

  test("ask and config-off match the backend", () => {
    expect(AGENTS.filter((a) => AGENT_INFO[a].ask)).toEqual(["claude", "opencode", "copilot"]);
    expect(AGENTS.filter((a) => AGENT_INFO[a].configOff)).toEqual(["claude", "codex", "opencode"]);
  });

  test("hand-over goes Claude to Codex and everything else to Claude", () => {
    expect(otherAgent("claude")).toBe("codex");
    expect(otherAgent("cursor")).toBe("claude");
    expect(otherAgent("nonsense")).toBe("codex");
  });
});

describe("splitAgents", () => {
  test("Claude and Codex up front, installed ones next, the rest under More", () => {
    const s = splitAgents(["claude", "copilot", "grok"], "claude");
    expect(s.shown).toEqual(["claude", "codex", "copilot", "grok"]);
    expect(s.installed).toEqual([]);
    expect(s.missing).toContain("cursor");
    expect([...s.shown, ...s.installed, ...s.missing].sort()).toEqual([...AGENTS].sort());
  });

  test("no more than `max` up front, the chosen one always among them", () => {
    const s = splitAgents(AGENTS, "kilo");
    expect(s.shown).toEqual(["claude", "codex", "gemini", "kilo"]);
    expect(s.installed[0]).toBe("opencode");
    expect(s.missing).toEqual([]);
    expect(splitAgents(null, "goose").shown).toEqual(["claude", "codex", "goose"]);
    // Unknown stored ids don't sneak in.
    expect(splitAgents(null, "amp" as never).shown).toEqual(["claude", "codex"]);
  });
});
