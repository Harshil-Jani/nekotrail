// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { liveFor, opencodeSkillsNote, parseMcp, scopeText, secretLabel, secretsAnswered } from "../src/mcpShare";
import type { McpRow, McpSecret } from "../src/api";

const row = (name: string): McpRow => ({
  name, transport: "stdio", summary: "", env: [], has: [], from: { agent: "claude", scope: "user", file: "~/.claude.json" }, secrets: [], targets: [], problem: null,
});

describe("parseMcp", () => {
  test("reads claude mcp list lines and skips the rest", () => {
    const text = "Checking MCP server health…\n\ngrafana: uvx mcp-grafana - ✔ Connected\ncloudflare: https://mcp.cloudflare.com/mcp (HTTP) - ✗ Failed to connect\n";
    const got = parseMcp(text);
    expect(got.map((s) => [s.name, s.ok, s.status])).toEqual([["grafana", true, "Connected"], ["cloudflare", false, "Failed to connect"]]);
    expect(liveFor(row("grafana"), got)?.ok).toBe(true);
    expect(liveFor(row("graf"), got)).toBeUndefined();
  });
});

describe("secrets", () => {
  const env: McpSecret = { id: "env:TOKEN", kind: "env", key: "TOKEN", can_ref: true };
  const arg: McpSecret = { id: "arg:2", kind: "arg", key: "--api-key", can_ref: false };
  test("every secret needs an answer the backend takes", () => {
    expect(secretsAnswered([], {})).toBe(true);
    expect(secretsAnswered([env], {})).toBe(false);
    expect(secretsAnswered([env], { "env:TOKEN": "ref" })).toBe(true);
    // Arguments can only be copied.
    expect(secretsAnswered([env, arg], { "env:TOKEN": "ref", "arg:2": "ref" })).toBe(false);
    expect(secretsAnswered([env, arg], { "env:TOKEN": "copy", "arg:2": "copy" })).toBe(true);
  });
  test("labels say what the secret is", () => {
    expect(secretLabel(env)).toBe("TOKEN (environment variable)");
    expect(secretLabel(arg)).toBe("Command line: --api-key");
    expect(scopeText("local")).toBe("this folder only");
  });
});

describe("opencodeSkillsNote", () => {
  test("says which shared folders OpenCode already reads", () => {
    expect(opencodeSkillsNote(["~/.config/opencode/skills", "~/.claude/skills", "~/.agents/skills"])).toBe(
      "OpenCode already reads skills in ~/.claude/skills and ~/.agents/skills.",
    );
    expect(opencodeSkillsNote(["~/oc/skills", "~/.agents/skills"])).toBe("OpenCode already reads skills in ~/.agents/skills.");
    expect(opencodeSkillsNote(["~/oc/skills"])).toBe("OpenCode reads skills only from ~/oc/skills, because it's set to skip the shared folders.");
    expect(opencodeSkillsNote([])).toBe("");
  });
});
