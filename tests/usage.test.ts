// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { costText, duration, keyName, limitText, meters, parseCap, rollup, tokenText, tokens, total, type UsageLine } from "../src/usage";
import { waitText } from "../src/queue";

const line = (day: string, repo: string, agent: string, model: string, extra: Partial<UsageLine> = {}): UsageLine => ({
  day, repo_id: repo, agent, model, turns: 1, priced_turns: 1, cost: 1, token_turns: 1, input: 100, cached: 1000, output: 10, duration_ms: 60_000, ...extra,
});

describe("usage numbers", () => {
  const lines = [
    line("2026-11-07", "r1", "claude", "claude-opus-4-5", { turns: 3, priced_turns: 3, cost: 4.5 }),
    line("2026-11-07", "r1", "codex", "", { turns: 2, priced_turns: 0, cost: 0, input: 500, output: 50 }),
    line("2026-11-06", "r2", "claude", "claude-opus-4-5", { cost: 2 }),
  ];

  test("groups by day, repo, agent and model", () => {
    expect(rollup(lines, "day").map((r) => [r.key, r.turns, r.cost])).toEqual([["2026-11-07", 5, 4.5], ["2026-11-06", 1, 2]]);
    expect(rollup(lines, "agent").map((r) => [r.key, r.turns, r.priced_turns])).toEqual([["claude", 4, 4], ["codex", 2, 0]]);
    expect(rollup(lines, "model").map((r) => r.key)).toEqual(["claude-opus-4-5", ""]);
    expect(rollup(lines, "repo").map((r) => [r.key, r.input + r.output])).toEqual([["r1", 660], ["r2", 110]]);
    const t = total(lines);
    expect([t.turns, t.priced_turns, t.cost, t.duration_ms]).toEqual([6, 4, 6.5, 180_000]);
    expect(total([]).turns).toBe(0);
  });

  test("an unreported cost is never shown as $0", () => {
    const [, codex] = rollup(lines, "agent");
    expect(costText(codex)).toEqual({ text: "Not reported", note: "This agent doesn't report a cost. Its turns and tokens still count." });
    expect(costText({ cost: 4.5, priced_turns: 3, turns: 5 })).toEqual({ text: "$4.50", note: "2 turns without a reported cost aren't in this number." });
    expect(costText({ cost: 4.5, priced_turns: 3, turns: 3 }).note).toBe("");
    expect(tokenText({ token_turns: 0, input: 0, output: 0 })).toBe("Not reported");
    expect(tokenText({ token_turns: 2, input: 1200, output: 300 })).toBe("1.5k");
  });

  test("short numbers and names", () => {
    expect([tokens(950), tokens(12_400), tokens(3_100_000), tokens(25_000_000)]).toEqual(["950", "12k", "3.1M", "25M"]);
    expect([duration(45_000), duration(12 * 60_000), duration(185 * 60_000), duration(120 * 60_000)]).toEqual(["45 s", "12 min", "3 h 5 min", "2 h"]);
    const name = (id: string) => (id === "r1" ? "pawprint-api" : "Removed repo");
    expect(keyName("repo", "r1", name, "", "")).toBe("pawprint-api");
    expect(keyName("agent", "codex", name, "", "")).toBe("Codex");
    expect(keyName("model", "", name, "", "")).toBe("Default model");
    expect(keyName("day", "2026-11-07", name, "2026-11-07", "2026-11-06")).toBe("Today");
    expect(keyName("day", "2026-11-06", name, "2026-11-07", "2026-11-06")).toBe("Yesterday");
  });
});

describe("caps", () => {
  test("what you type in a cap field", () => {
    expect(parseCap("", "usd")).toEqual({ value: null });
    expect(parseCap(" $7.505 ", "usd")).toEqual({ value: 7.51 });
    expect(parseCap("20", "usd")).toEqual({ value: 20 });
    expect(parseCap("2M", "tokens")).toEqual({ value: 2_000_000 });
    expect(parseCap("500k", "tokens")).toEqual({ value: 500_000 });
    expect(parseCap("1,000", "turns")).toEqual({ value: 1000 });
    expect("error" in parseCap("2.5", "turns")).toBe(true);
    expect("error" in parseCap("5k", "usd")).toBe(true);
    expect("error" in parseCap("0", "usd")).toBe(true);
    expect("error" in parseCap("lots", "tokens")).toBe(true);
  });

  test("today against the daily cap", () => {
    const m = meters({ usd: 20, turns: null, tokens: 2_000_000 }, { usd: 21.4, turns: 40, tokens: 500_000 });
    expect(m.map((x) => [x.label, x.text, x.reached, x.share])).toEqual([
      ["API-equivalent", "$21.40 of $20.00", true, 1],
      ["Tokens", "500k of 2.0M", false, 0.25],
    ]);
    expect(meters({ usd: null, turns: null, tokens: null }, { usd: 1, turns: 1, tokens: 1 })).toEqual([]);
  });

  test("a task held back by a cap says so in the queue", () => {
    const view = { max_total: 3, max_per_repo: 3, now: 0 };
    expect(waitText({ kind: "budget", text: "Today's cap is reached: $20.00 of $20.00 (API-equivalent). Nothing new starts until midnight." }, view))
      .toBe("Today's cap is reached: $20.00 of $20.00 (API-equivalent). Nothing new starts until midnight");
    // The banner says the cap on every repo, so its rows don't repeat it.
    const text = "Today's cap is reached: $20.40 of $20.00 (API-equivalent). Nothing new starts until midnight.";
    expect(waitText({ kind: "budget", text }, { ...view, cap: text })).toBe("Waits for today's cap to reset at midnight");
  });
});

describe("usage limits", () => {
  const now = 1_800_000_000;
  test("each agent's limit in one line", () => {
    expect(limitText("claude", undefined, now)).toBe("No usage limit reported");
    expect(limitText("claude", { until: now + 3600, reported: true, text: "" }, now, "en-US")).toMatch(/^Out of usage until \d{1,2}:\d{2}\s?[AP]M \(Claude said so\)$/);
    expect(limitText("codex", { until: now + 1800, reported: false, text: "" }, now, "en-US")).toMatch(/^Out of usage\. Codex didn't say until when, so nekotrail tries again at /);
    expect(limitText("claude", { until: now - 60, reported: true, text: "" }, now, "en-US")).toMatch(/^Its limit was due to reset at .*\. The next turn shows whether it has$/);
  });
});
