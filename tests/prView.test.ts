// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { prFromView, prStateOf, type Pr, type PrView } from "../src/api";

const pr = (extra: Partial<NonNullable<Pr>> = {}): NonNullable<Pr> => ({
  number: 1, url: "u", state: "OPEN", title: "t", isDraft: false, reviewDecision: "", mergeable: "MERGEABLE",
  statusCheckRollup: [], author: { login: "me" }, latestReviews: [], reviewRequests: [], additions: 1, deletions: 0,
  commentCount: 0, ...extra,
});
const view = (v: Partial<PrView>): PrView => ({ workspace_id: "w1", ...v });

describe("PR bar from the poller", () => {
  test("nothing known yet means still looking", () => {
    expect(prFromView(undefined)).toEqual({ pr: undefined, error: null });
  });

  test("a PR, or null for none, passes straight through", () => {
    expect(prFromView(view({ pr: pr() })).pr?.number).toBe(1);
    expect(prFromView(view({ pr: null }))).toEqual({ pr: null, error: null });
  });

  test("not logged in settles to no PR with the gh auth login prompt", () => {
    const r = prFromView(view({ problem: { code: "not_logged_in", message: "gh isn't logged in" } }));
    expect(r.pr).toBeNull();
    // PrBar turns errors matching "log in" into the gh auth login button.
    expect(r.error).toMatch(/log ?in|logged in/i);
  });

  test("rate limits keep the last answer and show nothing", () => {
    expect(prFromView(view({ pr: pr(), problem: { code: "rate_limited", message: "slow down" } }))).toEqual({ pr: pr(), error: null });
    expect(prFromView(view({ problem: { code: "rate_limited", message: "slow down" } }))).toEqual({ pr: undefined, error: null });
  });

  test("a repo without GitHub is just no PR, not an error", () => {
    expect(prFromView(view({ problem: { code: "not_github", message: "no remote" } }))).toEqual({ pr: null, error: null });
  });

  test("a passing hiccup only shows when there is nothing older to show", () => {
    expect(prFromView(view({ pr: pr(), problem: { code: "error", message: "timeout" } })).error).toBeNull();
    expect(prFromView(view({ problem: { code: "error", message: "timeout" } })).error).toBe("timeout");
  });
});

describe("PR state for the sidebar", () => {
  test("drafts are their own state, only while open", () => {
    expect(prStateOf(pr({ isDraft: true }))).toBe("DRAFT");
    expect(prStateOf(pr({ isDraft: true, state: "MERGED" }))).toBe("MERGED");
    expect(prStateOf(pr())).toBe("OPEN");
    expect(prStateOf(undefined)).toBeUndefined();
  });
});
