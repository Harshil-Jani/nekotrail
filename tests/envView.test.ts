// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { envMatches, envSourceLabel, MASK } from "../src/envView";
import type { EnvEntry } from "../src/api";

const e = (over: Partial<EnvEntry>): EnvEntry => ({ key: "API_URL", value: "http://localhost:4101", source: "Repo settings", masked: false, overridden_by: null, ...over });

describe("Environment sheet filter", () => {
  test("matches the name, the source and a shown value", () => {
    expect(envMatches(e({}), "api")).toBe(true);
    expect(envMatches(e({}), "repo set")).toBe(true);
    expect(envMatches(e({}), "4101")).toBe(true);
    expect(envMatches(e({}), "  ")).toBe(true);
    expect(envMatches(e({}), "nope")).toBe(false);
  });
  test("never searches a hidden value, so typing can't guess it", () => {
    const secret = e({ key: "STRIPE_SECRET_KEY", value: "sk_live_abc123", source: ".env.local", masked: true });
    expect(envMatches(secret, "sk_live")).toBe(false);
    expect(envMatches(secret, ".env.local")).toBe(true);
  });
  test("finds what is overridden", () => {
    expect(envMatches(e({ overridden_by: "Repo settings", source: "Login shell" }), "overridden")).toBe(true);
  });
  test("labels and the mask", () => {
    expect(envSourceLabel("Port block")).toBe("port block");
    expect(envSourceLabel("apps/web/.env")).toBe("apps/web/.env");
    expect(MASK).not.toMatch(/\w/);
  });
});
