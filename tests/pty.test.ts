// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { startFailedText } from "../src/pty";

describe("terminal that couldn't start", () => {
  test("says so and gives the reason, in terminal line ends", () => {
    const text = startFailedText("This workspace's folder is gone, so a terminal can't start there: /code/gone");
    expect(text).toContain("Couldn't start a terminal here.");
    expect(text).toContain("folder is gone");
    expect(text).toContain("/code/gone");
    expect(text).not.toMatch(/[^\r]\n/);
  });

  test("an empty reason still explains", () => {
    expect(startFailedText("  ")).toContain("The shell didn't start.");
  });

  test("multi-line reasons keep their lines", () => {
    expect(startFailedText("one\ntwo")).toContain("one\r\ntwo");
  });
});
