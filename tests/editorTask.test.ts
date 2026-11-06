import { expect, test } from "bun:test";
import { taskFacts } from "../src/editorTask";

test("names the file and lines a selection came from", () => {
  expect(taskFacts("Fix it.\n\nFrom src/auth/session.ts:41-44:\n\n```ts\nx\n```")).toEqual([["Code", "src/auth/session.ts:41-44"]]);
  expect(taskFacts("From a.go:7:\nx")).toEqual([["Code", "a.go:7"]]);
});

test("a short task without a selection adds nothing", () => {
  expect(taskFacts("Fix the flaky login test on CI")).toEqual([]);
});

test("a long task says how long it is", () => {
  const long = Array.from({ length: 300 }, (_, i) => `line ${i} of the selection`).join("\n");
  expect(taskFacts(long)).toEqual([["Length", `300 lines, ${long.length.toLocaleString("en-US")} characters`]]);
});
