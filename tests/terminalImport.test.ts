// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { bytes, keyOf, newKeys, picks, range, resumeCommand, totals } from "../src/terminalImport";
import type { TerminalPreview, TerminalSession } from "../src/api";

const s = (id: string, extra: Partial<TerminalSession> = {}): TerminalSession => ({
  agent: "claude", id, title: id, messages: 10, first: "2026-10-01T09:00:00+00:00", last: "2026-10-01T10:00:00+00:00", bytes: 2000, imported: false, resumable: true, ...extra,
});

const preview: TerminalPreview = {
  repos: [{
    root: "/code/app", name: "app", repo_id: null,
    folders: [
      { path: "/code/app", kind: "history", branch: "main", workspace_id: null, sessions: [s("a"), s("b", { imported: true })] },
      { path: "/code/app-fix", kind: "worktree", branch: "me/fix", workspace_id: null, sessions: [s("c", { agent: "codex", last: "2026-11-02T00:00:00+00:00" })] },
    ],
  }],
  skipped: [], found: { claude: 2, codex: 1 }, sources: [], took_ms: 1,
};

describe("terminal import", () => {
  test("ticks only what is not in nekotrail yet", () => {
    expect([...newKeys(preview)].sort()).toEqual(["claude:a", "codex:c"]);
  });

  test("never sends an imported session, even if ticked", () => {
    expect(picks(preview, new Set(["claude:a", "claude:b"]))).toEqual([{ agent: "claude", id: "a" }]);
    expect(keyOf({ agent: "codex", id: "c" })).toBe("codex:c");
  });

  test("totals add up and span the dates", () => {
    const t = totals(preview.repos[0].folders.flatMap((f) => f.sessions));
    expect(t).toEqual({ sessions: 3, messages: 30, bytes: 6000, first: "2026-10-01T09:00:00+00:00", last: "2026-11-02T00:00:00+00:00" });
    expect(range(t.first, t.first)).not.toContain(" to ");
    expect(range(t.first, t.last)).toContain(" to ");
    expect(range("", "")).toBe("");
  });

  test("sizes read plainly", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(4_200)).toBe("4 KB");
    expect(bytes(3_400_000)).toBe("3.4 MB");
    expect(bytes(2_193_000_000)).toBe("2.2 GB");
  });

  test("resume commands quote the folder", () => {
    expect(resumeCommand("claude", "x", "/a b")).toBe(`cd "/a b" && claude --resume x`);
    expect(resumeCommand("codex", "t", "/r")).toBe(`cd "/r" && codex resume t`);
    expect(resumeCommand("gemini", "g", "/r")).toBe(`cd "/r" && gemini --resume g`);
  });
});
