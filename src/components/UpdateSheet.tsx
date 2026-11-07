/**
 * The update card: a small notification-style sheet under the toolbar, top right, that never
 * blocks the window. The cat brings the parcel, trots along the download track, and is happy
 * when it's ready. Release notes are shown as plain text only.
 * Keys while the card has focus (or nothing else does): Enter = the primary button, Esc = Not now.
 */
import { useEffect, useRef } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Mascot } from "./Mascot";
import type { UpdatePhase } from "../useUpdater";
import { busyText, busyVerb, releaseUrl } from "../updateNotes";

type Props = {
  phase: UpdatePhase;
  onUpdate: () => void;
  onNotNow: () => void;
  onRestartNow: () => void;
  onRestartWhenIdle: () => void;
};

const mb = (bytes: number) => (bytes / 1_048_576).toFixed(1);
const open = (url: string) => openUrl(url).catch(() => window.open(url, "_blank"));

export function UpdateSheet({ phase, onUpdate, onNotNow, onRestartNow, onRestartWhenIdle }: Props) {
  const card = useRef<HTMLDivElement>(null);
  const primary = useRef<HTMLButtonElement>(null);

  // Take focus only if nothing else has it, so a popup never swallows keys meant for the chat box.
  useEffect(() => {
    const a = document.activeElement;
    if (!a || a === document.body || card.current?.contains(a)) primary.current?.focus({ preventScroll: true });
  }, [phase.kind]);

  // Enter / Esc also work when focus is nowhere in particular.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const a = document.activeElement;
      if (a && a !== document.body) return;
      if (e.key === "Escape" && secondary) { e.preventDefault(); secondary(); }
      else if (e.key === "Enter" && primary.current) { e.preventDefault(); primary.current.click(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  let art: React.ReactNode;
  let title: string;
  let sub: React.ReactNode = null;
  let body: React.ReactNode = null;
  let primaryBtn: { label: string; run: () => void } | null = null;
  let secondaryBtn: { label: string; run: () => void } | null = null;
  let extra: { label: string; run: () => void } | null = null;
  let otherBtn: { label: string; run: () => void } | null = null;

  switch (phase.kind) {
    case "available":
      art = <img className="update-parcel" src="/mascot-parcel.svg" alt="" width={84} height={72} />;
      title = `NekoTrail ${phase.version} is here`;
      sub = <>You have {phase.current}. {phase.items.length ? "What's new:" : ""}</>;
      body = phase.items.length > 0 && (
        <ul className="update-notes">
          {phase.items.map((t, i) => <li key={i}>{t}</li>)}
        </ul>
      );
      if (phase.more > 0) extra = { label: `${phase.more} more in the release notes`, run: () => open(releaseUrl(phase.version)) };
      else extra = { label: "Release notes", run: () => open(releaseUrl(phase.version)) };
      primaryBtn = { label: "Update", run: onUpdate };
      secondaryBtn = { label: "Not now", run: onNotNow };
      break;
    case "downloading": {
      const pct = phase.total ? Math.min(100, Math.round((phase.got / phase.total) * 100)) : null;
      art = <Mascot size={48} mood="working" title="" />;
      title = `Getting NekoTrail ${phase.version}`;
      sub = "You can keep working. It installs when you restart.";
      body = (
        <div className="update-progress">
          <div
            className={`update-track ${pct === null ? "indeterminate" : ""}`}
            style={{ "--p": pct ?? 0 } as React.CSSProperties}
            role="progressbar"
            aria-label="Download progress"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={pct ?? undefined}
          >
            <span className="update-runner"><Mascot size={34} mood="working" title="" /></span>
            <span className="update-rail"><span className="update-fill" /></span>
          </div>
          <p className="update-count num" aria-live="polite">
            {pct === null ? `${mb(phase.got)} MB so far` : `${mb(phase.got)} of ${mb(phase.total!)} MB`}
          </p>
        </div>
      );
      secondaryBtn = { label: "Hide", run: onNotNow };
      break;
    }
    case "busy": {
      const n = phase.agents + phase.terminals;
      art = <Mascot size={64} mood="happy" title="" />;
      title = `NekoTrail ${phase.version} is ready`;
      if (n === 0) {
        sub = "Restart to finish updating. It takes a few seconds.";
        primaryBtn = { label: "Restart now", run: onRestartNow };
        secondaryBtn = { label: "Not now", run: onNotNow };
      } else {
        sub = `${busyText(phase.agents, phase.terminals)} ${busyVerb(phase.agents, phase.terminals)} still running. Restarting now stops ${n === 1 ? "it" : "them"}.`;
        primaryBtn = { label: "Restart when idle", run: onRestartWhenIdle };
        secondaryBtn = { label: "Not now", run: onNotNow };
        otherBtn = { label: "Restart now", run: onRestartNow };
      }
      break;
    }
    case "waiting": {
      const what = busyText(phase.agents, phase.terminals);
      art = <Mascot size={64} mood="idle" title="" />;
      title = "Restarting when everything is idle";
      sub = what ? `Waiting for ${what} to finish.` : "Waiting for a quiet moment.";
      primaryBtn = { label: "Restart now", run: onRestartNow };
      secondaryBtn = { label: "Hide", run: onNotNow };
      break;
    }
    case "restarting":
      art = <Mascot size={64} mood="happy" title="" />;
      title = `Restarting into ${phase.version}`;
      sub = "Back in a few seconds.";
      break;
    case "checking":
      art = <Mascot size={64} mood="working" title="" />;
      title = "Checking for updates";
      sub = "Asking GitHub for the latest release.";
      secondaryBtn = { label: "Hide", run: onNotNow };
      break;
    case "latest":
      art = <Mascot size={64} mood="happy" title="" />;
      title = "You're on the latest";
      sub = phase.version ? `NekoTrail ${phase.version} is the newest version.` : "This is the newest version.";
      primaryBtn = { label: "OK", run: onNotNow };
      break;
    case "dev":
      art = <Mascot size={64} mood="idle" title="" />;
      title = "Update checks are off here";
      sub = "This is a development build. Release builds check GitHub on launch and every 6 hours.";
      primaryBtn = { label: "OK", run: onNotNow };
      break;
    case "error":
      art = <Mascot size={64} mood="worried" title="" />;
      title = phase.title;
      sub = phase.message;
      body = phase.detail ? <details><summary className="muted">Details</summary><p className="update-detail">{phase.detail}</p></details> : null;
      extra = { label: "Download from GitHub", run: () => open(releaseUrl(phase.version)) };
      primaryBtn = { label: "Try again", run: onUpdate };
      secondaryBtn = { label: "Close", run: onNotNow };
      break;
  }
  const secondary = secondaryBtn?.run ?? (primaryBtn?.label === "OK" ? primaryBtn.run : null);

  return (
    <div
      ref={card}
      className={`update-sheet update-${phase.kind}`}
      role="dialog"
      aria-modal="false"
      aria-labelledby="update-title"
      aria-describedby="update-sub"
      onKeyDown={(e) => {
        if (e.key === "Escape" && secondary) { e.preventDefault(); e.stopPropagation(); secondary(); }
        else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement) && primary.current) { e.preventDefault(); primary.current.click(); }
      }}
    >
      <div className="update-head">
        {art && <div className="update-art" aria-hidden="true">{art}</div>}
        <div className="update-titles">
          <h2 id="update-title">{title}</h2>
          {sub && <p id="update-sub" className="update-sub" aria-live={phase.kind === "restarting" ? "polite" : undefined}>{sub}</p>}
        </div>
      </div>
      {body}
      {(primaryBtn || secondaryBtn || extra) && (
        <div className="update-actions">
          {extra && <button className="link update-extra" onClick={extra.run}>{extra.label}</button>}
          <span className="spacer" />
          {secondaryBtn && <button className="quiet" onClick={secondaryBtn.run}>{secondaryBtn.label}</button>}
          {otherBtn && <button onClick={otherBtn.run}>{otherBtn.label}</button>}
          {primaryBtn && <button ref={primary} className="primary" onClick={primaryBtn.run}>{primaryBtn.label}</button>}
        </div>
      )}
    </div>
  );
}
