import { afterEach, describe, expect, test } from "bun:test";
import { bannerText, isQuiet, plainLine, saysSomething, setNoticeMode, showBanner } from "../src/notice";
import { chime, ting } from "../src/settings";

describe("banner text", () => {
  test("title is the task, subtitle repo and branch, body what happened", () => {
    expect(bannerText({ task: "Fix the login redirect", repo: "web", branch: "purr/fix-login", what: "The agent finished its turn" })).toEqual({
      title: "Fix the login redirect",
      subtitle: "web · purr/fix-login",
      body: "The agent finished its turn.",
    });
  });

  test("no emoji and no quotes around the task", () => {
    const b = bannerText({ task: "✅ “Fix login”", repo: "web", what: "⏳ The agent needs you" });
    expect(b.title).toBe("Fix login");
    expect(b.subtitle).toBe("web");
    expect(b.body).toBe("The agent needs you.");
  });

  test("a one-word leftover never becomes the body", () => {
    for (const leftover of ["back", "ok", "Done", "  yes  ", "✅"]) {
      expect(bannerText({ task: "Fix login", what: "The agent finished its turn", detail: leftover }).body).toBe("The agent finished its turn.");
    }
  });

  test("a real detail follows what happened", () => {
    const b = bannerText({ task: "Fix login", repo: "web", branch: "purr/x", what: "A check that passed before fails now", detail: "unit tests on macOS" });
    expect(b.body).toBe("A check that passed before fails now. unit tests on macOS.");
    // The chat's name is left out when it only repeats the task.
    expect(bannerText({ task: "Fix the login redirect", what: "Done", detail: "fix the login redirect" }).body).toBe("Done.");
  });

  test("an empty task still has a title", () => {
    expect(bannerText({ task: "", what: "x" }).title).toBe("NekoTrail");
    expect(plainLine("  a\n b  ")).toBe("a b");
    expect(saysSomething("back")).toBe(false);
    expect(saysSomething("Fix the redirect")).toBe(true);
  });
});

describe("test runs are silent", () => {
  afterEach(() => setNoticeMode({ system: true, quiet: false }));

  test("no banner, no sound in an e2e run", async () => {
    let made = 0;
    const g = globalThis as { AudioContext?: unknown };
    const saved = g.AudioContext;
    g.AudioContext = class { constructor() { made++; throw new Error("no audio here"); } };
    try {
      setNoticeMode({ system: false, quiet: true });
      expect(isQuiet()).toBe(true);
      expect(await showBanner(bannerText({ task: "t", what: "w" }))).toBe("test");
      chime(true);
      ting();
      expect(made).toBe(0);
      // Outside a test run the sounds do try to play.
      setNoticeMode({ system: true, quiet: false });
      ting();
      expect(made).toBe(1);
    } finally {
      g.AudioContext = saved;
    }
  });
});
