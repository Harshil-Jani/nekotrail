// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { commandsFrom, commandsText } from "../src/commandsLog";
import type { ChatEvent } from "../src/api";

describe("commands the agent ran", () => {
  test("pairs each shell command with its result, in order", () => {
    const events: ChatEvent[] = [
      { kind: "user", text: "fix it" },
      { kind: "tool", id: "1", name: "Bash", input: "bun test" },
      { kind: "tool", id: "2", name: "Read", input: "src/a.ts" },
      { kind: "tool_result", id: "2", output: "...", is_error: false },
      { kind: "tool_result", id: "1", output: "1 fail", is_error: true },
      { kind: "tool", id: "3", name: "Bash", input: "git status" },
      { kind: "tool_result", id: "3", output: "clean", is_error: false },
      { kind: "tool", id: "4", name: "Bash", input: "sleep 100" },
    ];
    expect(commandsFrom(events)).toEqual([
      { command: "bun test", output: "1 fail", failed: true, pending: false },
      { command: "git status", output: "clean", failed: false, pending: false },
      { command: "sleep 100", output: "", failed: false, pending: true },
    ]);
  });

  test("ignores other tools, empty commands and stray results", () => {
    const events: ChatEvent[] = [
      { kind: "tool", id: "1", name: "Edit", input: "a.ts" },
      { kind: "tool", id: "2", name: "Bash", input: "  " },
      { kind: "tool_result", id: "9", output: "x", is_error: false },
    ];
    expect(commandsFrom(events)).toEqual([]);
  });

  test("copies as plain text grouped by chat", () => {
    const text = commandsText([
      { chat: "Fix login", commands: [{ command: "bun test", output: "", failed: true, pending: false }, { command: "ls", output: "", failed: false, pending: false }] },
      { chat: "Empty", commands: [] },
    ]);
    expect(text).toBe("# Fix login\n(failed) bun test\nls");
  });
});
