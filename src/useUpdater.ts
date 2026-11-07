/**
 * In-app updates. Release builds check GitHub's latest.json 10 seconds after launch and every
 * 6 hours (dev builds and the browser preview never check on their own). "Not now" snoozes that
 * version for a day. "Update" downloads in the background; installing waits until nothing would
 * be lost: running agent turns and busy terminals are counted first, and the user picks
 * "Restart now" or "Restart when idle".
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { highlights, isSnoozed, snooze } from "./updateNotes";

export type UpdatePhase =
  | { kind: "checking" }
  | { kind: "latest"; version: string }
  | { kind: "dev" }
  | { kind: "available"; version: string; current: string; items: string[]; more: number }
  | { kind: "downloading"; version: string; got: number; total: number | null }
  | { kind: "busy"; version: string; agents: number; terminals: number }
  | { kind: "waiting"; version: string; agents: number; terminals: number }
  | { kind: "restarting"; version: string }
  | { kind: "error"; title: string; message: string; detail?: string; version?: string; retry: boolean };

const FIRST_CHECK_MS = 10_000;
const EVERY_MS = 6 * 60 * 60_000;
const IDLE_POLL_MS = 3_000;
const QUIET_TYPING_MS = 15_000;

/** Release builds inside the app check on their own. */
const autoChecks = () => !import.meta.env.DEV && "__TAURI_INTERNALS__" in window;

type Busy = { agents: number; terminals: number; unknown?: boolean };
/** What a restart would stop. If the app can't tell, it says so, and the caller asks instead of restarting. */
const busyWork = (): Promise<Busy> =>
  invoke<Busy | null>("busy_work").then(
    (b) => b ?? { agents: 0, terminals: 0, unknown: true },
    () => ({ agents: 0, terminals: 0, unknown: true }),
  );

/** An unsent message in a text box also counts as work: drafts are not saved across a restart. */
const hasDraft = () =>
  Array.from(document.querySelectorAll<HTMLTextAreaElement>("textarea")).some((t) => t.value.trim() !== "");

const detailOf = (e: unknown) => String((e as { message?: string })?.message ?? e ?? "").slice(0, 200);

declare global {
  interface Window {
    /** Set by src/preview.tsx (?update=...) to screenshot one state of the card. */
    __nekotrailUpdatePreview?: UpdatePhase;
  }
}

export function useUpdater() {
  const preview = window.__nekotrailUpdatePreview;
  const [phase, setPhase] = useState<UpdatePhase | null>(preview ?? null);
  const [open, setOpen] = useState(!!preview);
  const update = useRef<Update | null>(null);
  const downloaded = useRef(false);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const lastKey = useRef(0);

  useEffect(() => {
    const onKey = () => { lastKey.current = Date.now(); };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  /** Swap in a newly found update, freeing the old one unless it is already downloaded. */
  const adopt = (u: Update) => {
    if (update.current && update.current !== u) update.current.close().catch(() => {});
    update.current = u;
    downloaded.current = false;
  };

  const offer = (u: Update) => {
    const { items, more } = highlights(u.body);
    setPhase({ kind: "available", version: u.version, current: u.currentVersion, items, more });
    setOpen(true);
  };

  /** Busy with an update already (downloading, waiting to restart): checks leave it alone. */
  const inFlight = () => {
    const k = phaseRef.current?.kind;
    return k === "downloading" || k === "busy" || k === "waiting" || k === "restarting";
  };

  const autoCheck = useCallback(async () => {
    // Downloaded but put off ("Not now" while agents ran): remind gently, at most every 6 hours.
    if (downloaded.current && update.current && phaseRef.current?.kind === "busy") { await afterDownload(update.current.version, true); return; }
    if (inFlight() || downloaded.current) return;
    try {
      const u = await check({ timeout: 30_000 });
      if (!u) return;
      if (isSnoozed(u.version)) { u.close().catch(() => {}); return; }
      adopt(u);
      offer(u);
    } catch (e) {
      // Offline or GitHub hiccup: a background check never interrupts anyone.
      console.warn("update check failed", e);
    }
  }, []);

  useEffect(() => {
    if (!autoChecks()) return;
    const first = setTimeout(autoCheck, FIRST_CHECK_MS);
    const every = setInterval(autoCheck, EVERY_MS);
    return () => { clearTimeout(first); clearInterval(every); };
  }, [autoCheck]);

  /** Settings and ⌘K. `showLatest` opens the card for "You're on the latest" too. */
  const checkNow = useCallback(async (showLatest: boolean) => {
    const k = phaseRef.current?.kind;
    if (k === "downloading" || k === "waiting" || k === "restarting") { setOpen(true); return; }
    if (downloaded.current && update.current) { await afterDownload(update.current.version, true); return; }
    if (import.meta.env.DEV) { setPhase({ kind: "dev" }); setOpen(showLatest); return; }
    setPhase({ kind: "checking" });
    if (showLatest) setOpen(true);
    try {
      const u = await check({ timeout: 30_000 });
      if (!u) {
        setPhase({ kind: "latest", version: await getVersion().catch(() => "") });
        setOpen(showLatest);
        return;
      }
      adopt(u);
      offer(u);
    } catch (e) {
      setPhase({ kind: "error", title: "Couldn't check for updates", message: "NekoTrail couldn't reach GitHub. Check your connection and try again.", detail: detailOf(e), retry: false });
      setOpen(true);
    }
  }, []);

  const restart = async (version: string) => {
    setPhase({ kind: "restarting", version });
    setOpen(true);
    try {
      await update.current!.install();
    } catch (e) {
      setPhase({ kind: "error", title: "The update didn't install", message: "Nothing was changed. You can try again, or download it from GitHub.", detail: detailOf(e), version, retry: true });
      return;
    }
    // Installed. Stop agents and terminals cleanly, then come back on the new version.
    await invoke("stop_all_work").catch(() => {});
    await relaunch();
  };

  /**
   * Downloaded: restart straight away if nothing is running, otherwise ask. Reminders (`ask`)
   * always ask, since the user may be in the middle of something the app can't see.
   */
  const afterDownload = async (version: string, ask = false) => {
    const b = await busyWork();
    if (!ask && !b.unknown && b.agents + b.terminals === 0) return restart(version);
    setPhase({ kind: "busy", version, agents: b.agents, terminals: b.terminals });
    setOpen(true);
  };

  const startUpdate = useCallback(async () => {
    const u = update.current;
    const p = phaseRef.current;
    if (p?.kind === "error" && !p.retry) { checkNow(true); return; }
    if (!u) return;
    if (downloaded.current) { await afterDownload(u.version); return; }
    let got = 0;
    let total: number | null = null;
    let painted = 0;
    setPhase({ kind: "downloading", version: u.version, got: 0, total: null });
    try {
      await u.download((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? null;
        else if (ev.event === "Progress") got += ev.data.chunkLength;
        const now = performance.now();
        if (ev.event !== "Progress" || now - painted > 80) {
          painted = now;
          setPhase({ kind: "downloading", version: u.version, got, total });
        }
      }, { timeout: 10 * 60_000 });
    } catch (e) {
      setPhase({ kind: "error", title: "The update didn't download", message: "Check your connection and try again, or download it from GitHub.", detail: detailOf(e), version: u.version, retry: true });
      setOpen(true);
      return;
    }
    downloaded.current = true;
    await afterDownload(u.version);
  }, [checkNow]);

  const restartNow = useCallback(() => {
    const p = phaseRef.current;
    if (p && "version" in p && p.version && update.current) restart(p.version);
  }, []);

  const restartWhenIdle = useCallback(() => {
    const p = phaseRef.current;
    if (p?.kind === "busy") setPhase({ ...p, kind: "waiting" });
  }, []);

  // Waiting: poll what is running; restart once agents, terminals and typing have all been quiet twice in a row.
  const waiting = phase?.kind === "waiting";
  useEffect(() => {
    if (!waiting) return;
    let quiet = 0;
    const t = setInterval(async () => {
      const p = phaseRef.current;
      if (p?.kind !== "waiting") return;
      const b = await busyWork();
      const typing = Date.now() - lastKey.current < QUIET_TYPING_MS || hasDraft();
      if (b.agents !== p.agents || b.terminals !== p.terminals) setPhase({ ...p, agents: b.agents, terminals: b.terminals });
      quiet = !b.unknown && b.agents + b.terminals === 0 && !typing ? quiet + 1 : 0;
      if (quiet >= 2) { clearInterval(t); restart(p.version); }
    }, IDLE_POLL_MS);
    return () => clearInterval(t);
  }, [waiting]);

  /**
   * Esc / "Not now". On an offer it snoozes that version for a day. While downloading or
   * waiting it only hides the card: the work carries on and the card comes back when needed.
   */
  const notNow = useCallback(() => {
    const p = phaseRef.current;
    setOpen(false);
    if (p?.kind === "available") {
      snooze(p.version);
      update.current?.close().catch(() => {});
      update.current = null;
      setPhase(null);
    } else if (p?.kind === "error" || p?.kind === "checking") {
      setPhase(null);
    }
  }, []);

  return { phase, open, checkNow, startUpdate, notNow, restartNow, restartWhenIdle };
}
