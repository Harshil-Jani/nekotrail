// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { DEFAULT_FILTER, accountLine, createLabel, emptyText, issueMeta, issueRef, linearFrom, looksLikeKey } from "../src/linear";

// Made up, in the shape of a real key. Not live.
const KEY = "lin_api_" + "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56Qr78St90";

describe("Linear helpers", () => {
  test("who the key belongs to", () => {
    expect(accountLine({ name: "Ada Lovelace", org: "Acme" })).toBe("Ada Lovelace, Acme");
    expect(accountLine({ name: " ", org: "Acme" })).toBe("Acme");
    expect(accountLine(null)).toBe("");
  });

  test("an issue's muted line keeps only what helps pick it", () => {
    expect(issueMeta({ state: "Todo", priority: 2, labels: ["Bug", "Auth", "Web"] })).toBe("Todo · High · Bug, Auth");
    expect(issueMeta({ state: "Backlog", priority: 3, labels: [] })).toBe("Backlog");
    expect(issueMeta({ state: "In Progress", priority: 1, labels: ["Incident"] })).toBe("In Progress · Urgent · Incident");
  });

  test("keys and identifiers", () => {
    expect(looksLikeKey(KEY)).toBe(true);
    expect(looksLikeKey(`  Bearer ${KEY} `)).toBe(true);
    expect(looksLikeKey("ghp_abcdefghijklmnopqrstuvwxyz0123456789")).toBe(false);
    expect(looksLikeKey("lin_api_short")).toBe(false);
    expect(issueRef("eng-123")).toBe("ENG-123");
    expect(issueRef(" OPS2-7 ")).toBe("OPS2-7");
    expect(issueRef("https://linear.app/acme/issue/ENG-42/fix-the-thing")).toBe("ENG-42");
    for (const bad of ["", "ENG", "ENG-012", "1NG-2", "fix the login", "ENG-12a"]) expect(issueRef(bad)).toBeNull();
  });

  test("the empty list says which filter is on", () => {
    expect(emptyText(DEFAULT_FILTER)).toBe("No open issues assigned to you.");
    expect(emptyText({ ...DEFAULT_FILTER, anyone: true, state: "started" }, "Engineering")).toBe("No in progress issues in Engineering.");
    expect(emptyText({ ...DEFAULT_FILTER, state: "all", query: " login " })).toBe('No issues assigned to you matching "login".');
    expect(emptyText({ ...DEFAULT_FILTER, query: "eng-9" })).toBe("No issue ENG-9, or your key can't see it.");
  });

  test("the create button and the request", () => {
    expect(createLabel(null)).toBe("Pick an issue");
    expect(createLabel("ENG-123")).toBe("Create from ENG-123");
    expect(linearFrom("ENG-123")).toBe("linear:ENG-123");
  });
});
