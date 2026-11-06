// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { EVENTS, SERVICES, secretReady, statusLine, type Destination } from "../src/alerts";

const NOW = Date.parse("2026-11-07T09:00:00Z");
const sec = (ms: number) => Math.floor(ms / 1000);
const dest = (over: Partial<Destination> = {}): Destination => ({ id: "d1", service: "ntfy", name: "My phone", on: true, events: ["needs_you"], last_line: false, saved: true, hint: "ntfy.sh", ...over });

describe("away alerts in Settings", () => {
  test("every event and service the backend knows has a label", () => {
    expect(EVENTS.map((e) => e.id)).toEqual(["needs_you", "turn_finished", "checks_failed", "checks_green", "pr_merged", "queue_drained", "cap_reached", "runaway", "morning_summary"]);
    expect(SERVICES.map((s) => s.id)).toEqual(["ntfy", "slack", "discord", "webhook", "pushover"]);
    for (const e of EVENTS) expect(e.label).not.toContain("—");
  });

  test("optional fields don't block saving, required ones do", () => {
    expect(secretReady("ntfy", { url: "https://ntfy.sh/t", token: "", user: "" })).toBe(true);
    expect(secretReady("ntfy", { url: " ", token: "x", user: "" })).toBe(false);
    expect(secretReady("pushover", { url: "", token: "a", user: "" })).toBe(false);
    expect(secretReady("pushover", { url: "", token: "a", user: "b" })).toBe(true);
  });

  test("the status line puts an error first and says why something waits", () => {
    expect(statusLine(dest({ saved: false }), undefined, 0, false, NOW).text).toContain("Not set up yet");
    const err = statusLine(dest(), { last_ok: sec(NOW) - 60, last_error: "No answer in 10 seconds.", last_error_at: sec(NOW) - 300 }, 2, false, NOW);
    expect(err).toEqual({ text: "Last try failed 5 min ago: No answer in 10 seconds.", bad: true });
    expect(statusLine(dest(), {}, 3, true, NOW).text).toBe("3 waiting for quiet hours to end");
    expect(statusLine(dest(), {}, 3, false, NOW).text).toBe("3 waiting, sent together in a moment");
    expect(statusLine(dest(), { last_ok: sec(NOW) - 10 }, 0, false, NOW).text).toBe("Last sent just now");
    expect(statusLine(dest({ on: false }), {}, 0, false, NOW).text).toBe("Off. Nothing is sent.");
    expect(statusLine(dest({ events: [] }), {}, 0, false, NOW).text).toBe("Pick at least one event.");
  });
});
