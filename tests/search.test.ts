// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { ago, flatten, occurrences, plainSnippet, sinceFor, snippetParts } from "../src/search";
import type { SearchGroup } from "../src/api";

describe("global search helpers", () => {
  test("snippets split into plain and matched runs, never HTML", () => {
    expect(snippetParts("why \u0002502\u0003 after <b>deploy</b>\n…")).toEqual([
      { text: "why ", hit: false },
      { text: "502", hit: true },
      { text: " after <b>deploy</b> …", hit: false },
    ]);
    expect(snippetParts("")).toEqual([]);
    expect(snippetParts("\u0002a\u0003\u0002b\u0003")).toEqual([{ text: "a", hit: true }, { text: "b", hit: true }]);
  });

  test("date filters start at local midnight, 7 or 30 days back", () => {
    const now = new Date(2026, 10, 6, 15, 30);
    expect(sinceFor("any", now)).toBeNull();
    expect(new Date(sinceFor("today", now)!).getHours()).toBe(0);
    expect(new Date(sinceFor("today", now)!).getDate()).toBe(6);
    expect(Date.parse(sinceFor("week", now)!)).toBe(now.getTime() - 7 * 86_400_000);
    expect(Date.parse(sinceFor("month", now)!)).toBe(now.getTime() - 30 * 86_400_000);
  });

  test("results flatten into keyboard stops, with a 'more' stop per group that has more", () => {
    const hit = (id: number) => ({ message_id: id, session_id: "s", kind: "user" as const, snippet: "", created_at: "", agent: "claude" as const, chat: "" });
    const g = (id: string, n: number, more: number): SearchGroup => ({ workspace_id: id, repo_id: "r", repo: "api", name: id, title: "", branch: "b", state: "active", hits: Array.from({ length: n }, (_, i) => hit(i)), more });
    const items = flatten([g("a", 2, 3), g("b", 1, 0)]);
    expect(items.map((i) => `${i.group.workspace_id}:${i.type}`)).toEqual(["a:hit", "a:hit", "a:more", "b:hit"]);
  });

  test("relative dates", () => {
    const now = Date.parse("2026-11-06T12:00:00Z");
    expect(ago("2026-11-06T11:59:30Z", now)).toBe("now");
    expect(ago("2026-11-06T11:15:00Z", now)).toBe("45 min ago");
    expect(ago("2026-11-06T07:00:00Z", now)).toBe("5 h ago");
    expect(ago("2026-11-05T10:00:00Z", now)).toBe("yesterday");
    expect(ago("2026-11-02T12:00:00Z", now)).toBe("4 days ago");
    expect(ago("not a date", now)).toBe("");
  });

  test("find-in-chat occurrences are case-insensitive and don't overlap", () => {
    expect(occurrences("Retry, retry and RETRY", "retry")).toEqual([[0, 5], [7, 12], [17, 22]]);
    expect(occurrences("aaaa", "aa")).toEqual([[0, 2], [2, 4]]);
    expect(occurrences("abc", "")).toEqual([]);
  });
});

describe("plainSnippet", () => {
  test("drops markdown marks around words and matches", () => {
    expect(plainSnippet("after **30s**, I raised `\u0002IdleTimeout\u0003` to 75s")).toBe("after 30s, I raised \u0002IdleTimeout\u0003 to 75s");
  });
  test("links keep their text, list and heading markers go", () => {
    expect(plainSnippet("## Plan\n- see [the doc](https://x.dev/a)")).toBe("Plan\nsee the doc");
  });
  test("leaves maths and lone asterisks alone", () => {
    expect(plainSnippet("2 * 3 = 6")).toBe("2 * 3 = 6");
  });
});
