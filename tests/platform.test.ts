// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { detectMac, keyHint } from "../src/platform";

describe("platform", () => {
  test("WKWebView is a Mac, WebKitGTK is not", () => {
    expect(detectMac({ platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)" })).toBe(true);
    expect(detectMac({ platform: "Linux x86_64", userAgent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15" })).toBe(false);
    // navigator.platform wins: a WebKitGTK that ever claims to be a Mac in its user agent stays Linux.
    expect(detectMac({ platform: "Linux x86_64", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)" })).toBe(false);
    expect(detectMac({ platform: "", userAgent: "Mozilla/5.0 (X11; Linux aarch64)" })).toBe(false);
    expect(detectMac(undefined)).toBe(true);
  });
});

describe("shortcut hints", () => {
  const all = ["⌘K", "⌘⇧↵", "⌃1 to ⌃9", "⌘⌥↑ / ⌘⌥↓", "Show sidebar (⌘B)", "Allow (↵)", "⌘+ and ⌘- zoom", "⌘,"];

  test("come back unchanged on a Mac", () => {
    for (const s of all) expect(keyHint(s, true)).toBe(s);
  });

  test("are spelled out on Linux", () => {
    expect(keyHint("⌘K", false)).toBe("Ctrl+K");
    expect(keyHint("⌘⇧↵", false)).toBe("Ctrl+Shift+Enter");
    expect(keyHint("⌘⇧⌫", false)).toBe("Ctrl+Shift+Backspace");
    expect(keyHint("⌘⌥↑ / ⌘⌥↓", false)).toBe("Ctrl+Alt+↑ / Ctrl+Alt+↓");
    expect(keyHint("Show sidebar (⌘B)", false)).toBe("Show sidebar (Ctrl+B)");
    expect(keyHint("Allow (↵)", false)).toBe("Allow (Enter)");
    expect(keyHint("↵ sends, ⇧↵ new line", false)).toBe("Enter sends, Shift+Enter new line");
    expect(keyHint("⌘+ and ⌘- zoom anywhere, ⌘0 resets", false)).toBe("Ctrl++ and Ctrl+- zoom anywhere, Ctrl+0 resets");
    expect(keyHint("⌘, opens settings", false)).toBe("Ctrl+, opens settings");
    expect(keyHint("⌘[", false)).toBe("Ctrl+[");
    expect(keyHint("⌃O", false)).toBe("Ctrl+O");
  });

  test("follow the Linux key moves: workspaces on Alt, Open in on Ctrl+Shift+O", () => {
    expect(keyHint("⌃1 to ⌃9", false)).toBe("Alt+1 to Alt+9");
    expect(keyHint("⌘O", false)).toBe("Ctrl+Shift+O");
    expect(keyHint("⌘1 / ⌘2 / ⌘3", false)).toBe("Ctrl+1 / Ctrl+2 / Ctrl+3");
  });
});
