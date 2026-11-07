import { invoke } from "@tauri-apps/api/core";

/**
 * System banners (notice.rs sends them). Title = the task, subtitle = "repo · branch",
 * body = what happened, the mascot on the right. Plain text: no emoji, no one-word leftovers.
 */
export type Banner = { title: string; subtitle: string; body: string };

/** Why a banner did not go out: an e2e run ("test"), a macOS build outside a .app ("unbundled"), or no Tauri at all ("preview"). */
export type BannerResult = "send" | "test" | "unbundled" | "preview";

// Emoji, pictographs and the joiners and variation selectors that glue them together.
const PICTO = /[\u{231A}-\u{23FF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}\u{FE00}-\u{FE0F}\u{1F000}-\u{1FAFF}\u{200D}\u{20E3}]/gu;

/** One plain line: no emoji, no line breaks, no wrapping quotes, collapsed spaces. */
export function plainLine(s: string | undefined | null): string {
  return (s ?? "").replace(PICTO, "").replace(/\s+/g, " ").trim().replace(/^[“"']+|[”"']+$/g, "").trim();
}

/** A detail worth a line of its own: a few real words, not a leftover like "back" or "ok". */
export function saysSomething(s: string): boolean {
  const words = s.split(" ").filter((w) => /\p{L}{2,}/u.test(w));
  return words.length >= 3 || (words.length >= 2 && s.length >= 12);
}

const sentence = (s: string) => (/[.!?…]$/.test(s) ? s : `${s}.`);

/**
 * `task` is the workspace's task title (or its branch or star when it has none), `what` a full
 * sentence about what happened, `detail` an optional extra line (the chat's name, a check's name).
 */
export function bannerText(i: { task: string; repo?: string; branch?: string; what: string; detail?: string | null }): Banner {
  const title = plainLine(i.task) || "NekoTrail";
  const repo = plainLine(i.repo), branch = plainLine(i.branch);
  const subtitle = repo && branch ? `${repo} · ${branch}` : repo || branch;
  const what = sentence(plainLine(i.what) || "Open NekoTrail to see what happened");
  const detail = plainLine(i.detail);
  const useful = detail && saysSomething(detail) && detail.toLowerCase() !== title.toLowerCase();
  const body = useful ? `${what} ${sentence(detail)}` : what;
  return { title, subtitle, body };
}

let mode = { system: true, quiet: false };

/** An end-to-end run: no banners, no sounds, no Dock bounces. */
export const isQuiet = () => mode.quiet;
export const setNoticeMode = (m: { system: boolean; quiet: boolean }) => { mode = { system: m.system, quiet: m.quiet }; };

/** Asks the app once at startup whether this process may alert at all. */
export async function loadNoticeMode(): Promise<void> {
  try {
    setNoticeMode(await invoke<{ system: boolean; quiet: boolean }>("notice_mode"));
  } catch {
    /* not in Tauri (preview) */
  }
}

/** `mood`: "happy" (done) or "idle" (needs you) picks the mascot on the right. */
export async function showBanner(b: Banner, mood: "happy" | "idle" = "idle"): Promise<BannerResult> {
  if (mode.quiet) return "test";
  try {
    return await invoke<BannerResult>("system_notify", { ...b, mood });
  } catch {
    return "preview";
  }
}
