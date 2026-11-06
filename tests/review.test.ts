// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { hasSuggestion, planLine, requestMeta, reviewAgent, reviewWorkspaceFor, submitBlocker } from "../src/review";
import type { ReviewRequest, Workspace } from "../src/api";

const ws = (id: string, review_pr: number, state: Workspace["state"] = "active", repo_id = "r"): Workspace => ({
  id, repo_id, name: id, branch: "b", path: "/p", target_branch: "main", state, pinned: false, unread: false,
  notes: "", imported_from: null, created_at: "", title: "", title_source: "", status: "", pair_id: "", section_id: "", parent_id: "", review_pr,
});

const req: ReviewRequest = {
  repo_id: "r", number: 12, url: "u", title: "Add retries", isDraft: false, headRefName: "retries", headRefOid: "abc", baseRefName: "main",
  isCrossRepository: false, additions: 1, deletions: 0, changedFiles: 1, updatedAt: "", author: { login: "sam", avatarUrl: "" },
};

describe("review requests", () => {
  test("a review starts with the agent you last picked, else Claude", () => {
    expect(reviewAgent({ agent: "gemini" })).toBe("gemini");
    expect(reviewAgent({ agent: "both" })).toBe("claude");
    expect(reviewAgent(null)).toBe("claude");
    expect(reviewAgent({ agent: "rm -rf" })).toBe("claude");
  });
  test("an open review workspace is found again, archived ones and other repos are not", () => {
    expect(reviewWorkspaceFor(req, [ws("a", 0), ws("b", 12)])?.id).toBe("b");
    expect(reviewWorkspaceFor(req, [ws("b", 12, "archived")])).toBeUndefined();
    expect(reviewWorkspaceFor(req, [ws("b", 12, "active", "other")])).toBeUndefined();
  });
  test("the meta line", () => {
    expect(requestMeta(req, "app")).toBe("#12 · @sam · app");
    expect(requestMeta({ ...req, isDraft: true })).toBe("#12 · @sam · draft");
  });
});

describe("submitting a review", () => {
  const plan = (inline: number, folded = 0, suggestions = 0) => ({ inline, folded: Array.from({ length: folded }, (_, i) => `f${i}`), suggestions });
  test("request changes needs a summary, folded comments count as one", () => {
    expect(submitBlocker("request_changes", " ", plan(3))).not.toBeNull();
    expect(submitBlocker("request_changes", "", plan(0, 1))).toBeNull();
    expect(submitBlocker("request_changes", "Fix the race.", plan(0))).toBeNull();
  });
  test("a comment review needs something in it; approve never does", () => {
    expect(submitBlocker("comment", "", plan(0))).not.toBeNull();
    expect(submitBlocker("comment", "", plan(1))).toBeNull();
    expect(submitBlocker("approve", "", plan(0))).toBeNull();
  });
  test("plain words on where the drafts go", () => {
    expect(planLine(plan(3, 0, 1))).toBe("3 comments go on lines, 1 with a suggested change.");
    expect(planLine(plan(1, 2))).toBe("1 comment goes on lines. 2 comments are on lines outside this PR's changes, so they go in the summary.");
    expect(planLine(plan(0))).toBe("No line comments. Only the summary goes up.");
    expect(planLine(plan(0, 1, 0))).toContain("so it goes in the summary");
  });
  test("suggestion blocks", () => {
    expect(hasSuggestion("Fix.\n\n```suggestion\nx\n```")).toBe(true);
    expect(hasSuggestion("Fix.\n\n````suggestion\nx\n````")).toBe(true);
    expect(hasSuggestion("```ts\nx\n```")).toBe(false);
  });
});
