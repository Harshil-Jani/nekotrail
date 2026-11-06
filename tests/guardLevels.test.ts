// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { AGENTS, AGENT_INFO, guardGroups } from "../src/agents";

describe("how protected files hold, per agent", () => {
  test("every agent says how, matching Agent::guard_level in the backend", () => {
    const level = Object.fromEntries(AGENTS.map((a) => [a, AGENT_INFO[a].guard.level]));
    expect(level.claude).toBe("blocked");
    expect(level.opencode).toBe("blocked");
    expect(level.codex).toBe("after");
    expect(level.gemini).toBe("after");
    // Every other agent speaks ACP: refused when it asks, checked after the turn otherwise.
    for (const a of AGENTS.filter((x) => !["claude", "opencode", "codex", "gemini"].includes(x))) expect(level[a]).toBe("asks");
    for (const a of AGENTS) expect(AGENT_INFO[a].guard.how.length).toBeGreaterThan(10);
  });

  test("Repo settings groups agents that hold the same way, strongest first, each listed once", () => {
    const groups = guardGroups();
    expect(groups.map((g) => g.level)).toEqual(["blocked", "blocked", "asks", "asks", "after", "after"]);
    expect(groups.flatMap((g) => g.agents).sort()).toEqual([...AGENTS].sort());
    const shared = groups.find((g) => g.agents.includes("cursor"))!;
    expect(shared.agents).toEqual(["cursor", "goose", "qwen", "kimi", "auggie", "grok", "kiro", "droid", "vibe", "cline", "kilo"]);
    expect(shared.how).not.toMatch(/Cursor/);
  });
});
