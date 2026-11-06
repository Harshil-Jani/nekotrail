// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { countLine, duration, fixAllPrompt, fixPrompt, FIX_ALL_LIMIT, headline, turnLine, where } from "../src/checks";
import type { CheckFailure, CheckResult } from "../src/api";

const failure = (extra: Partial<CheckFailure> = {}): CheckFailure => ({
  title: "names > slugs keep digits", file: "src/names.test.ts", line: 3, message: "expect(received).toBe(expected)\nExpected: \"fix-502\"", ...extra,
});
const result = (extra: Partial<CheckResult> = {}): CheckResult => ({
  name: "test", command: "bun test", status: "failed", passed: 2, failed: 1, failures: [failure()], formats: ["bun test"],
  exit_code: 1, duration_ms: 420, finished_at: "", regressed: false, ...extra,
});

describe("fix prompts", () => {
  test("one failure: the check, where, the message, the code, and how to confirm", () => {
    const p = fixPrompt({ name: "test", command: "bun test" }, failure({ snippet: "> 3 | expect(slug(\"Fix 502\"))" }));
    expect(p).toContain("The `test` check fails.");
    expect(p).toContain("**names > slugs keep digits** at `src/names.test.ts:3`");
    expect(p).toContain("```\nexpect(received).toBe(expected)\nExpected: \"fix-502\"\n```");
    expect(p).toContain("The code there:\n```\n> 3 | expect(slug(\"Fix 502\"))\n```");
    expect(p.endsWith("Then run `bun test` and make sure it passes.")).toBe(true);
  });

  test("no place, no repeat of a message that is the title, and fences the text can't break", () => {
    const p = fixPrompt({ name: "lint", command: "eslint ." }, failure({ file: null, line: null, title: "Exited with code 2", message: "Exited with code 2" }));
    expect(p).toContain("**Exited with code 2**\n");
    expect(p).not.toContain(" at `");
    expect(p.match(/```/g)).toBeNull();
    const q = fixPrompt({ name: "t", command: "x" }, failure({ message: "see ```js\nx\n```" }));
    expect(q).toContain("~~~~\nsee ```js");
  });

  test("fix all groups failures by check and caps the message", () => {
    const many = Array.from({ length: 30 }, (_, i) => failure({ title: `case ${i}`, line: i + 1 }));
    const p = fixAllPrompt([
      result({ failures: many, failed: 30 }),
      result({ name: "lint", command: "bun run lint", failures: [failure({ title: "no-unused-vars" })] }),
      result({ name: "typecheck", command: "tsc", status: "passed", failures: [] }),
    ]);
    expect(p.startsWith("2 checks fail.")).toBe(true);
    expect(p).toContain("## test: `bun test`");
    expect(p.match(/^- \*\*/gm)?.length).toBe(FIX_ALL_LIMIT);
    // The cap is reached inside the first check, so lint's heading never starts.
    expect(p).not.toContain("## lint");
    expect(p).toContain("There are 6 more failures like these.");
    expect(p).toContain("run `bun test`, `bun run lint` and make sure they all pass.");
    expect(p).not.toContain("tsc");
    const one = fixAllPrompt([result()]);
    expect(one.startsWith("The `test` check fails.")).toBe(true);
    expect(one).toContain("make sure it passes.");
  });
});

describe("words", () => {
  test("counts", () => {
    expect(countLine(result())).toBe("1 failed, 2 passed");
    expect(countLine(result({ passed: null, failed: 3 }))).toBe("3 failed");
    expect(countLine(result({ status: "passed", passed: 40, failed: 0 }))).toBe("40 passed");
    expect(countLine(result({ status: "passed", passed: null, failed: 0 }))).toBe("Passed");
    expect(countLine(result({ status: "error" }))).toBe("Couldn't run");
  });

  test("durations", () => {
    expect(duration(420)).toBe("0.4 s");
    expect(duration(12_400)).toBe("12 s");
    expect(duration(125_000)).toBe("2 min 5 s");
    expect(duration(120_000)).toBe("2 min");
  });

  test("headline", () => {
    const checks = [{ name: "test", command: "bun test", source: "app" as const }, { name: "lint", command: "x", source: "app" as const }];
    expect(headline({ checks, results: [], active: false, current: null })).toBe("2 checks, not run yet");
    expect(headline({ checks, results: [], active: true, current: "lint" })).toBe("Running lint…");
    expect(headline({ checks, results: [result({ status: "passed" })], active: false, current: null })).toBe("1 of 2 passing");
    expect(headline({ checks, results: [result({ status: "passed" }), result({ name: "lint", status: "passed" })], active: false, current: null })).toBe("All 2 passing");
    expect(headline({ checks, results: [result(), result({ name: "lint", status: "passed" })], active: false, current: null })).toBe("1 of 2 failing");
    // A result for a check that no longer exists doesn't count.
    expect(headline({ checks: checks.slice(0, 1), results: [result({ name: "gone" })], active: false, current: null })).toBe("1 check, not run yet");
  });

  test("turn note and places", () => {
    expect(turnLine([{ name: "test", status: "passed", passed: 4, failed: 0 }, { name: "lint", status: "failed", passed: null, failed: 2 }, { name: "tsc", status: "error", passed: null, failed: 1 }]))
      .toBe("test passed · lint 2 failed · tsc couldn't run");
    expect(where({ file: "a.ts", line: 3 })).toBe("a.ts:3");
    expect(where({ file: "a.ts", line: null })).toBe("a.ts");
    expect(where({ file: null, line: 3 })).toBe("");
  });
});
