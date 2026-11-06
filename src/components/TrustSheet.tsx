import { useEffect } from "react";
import { ShieldAlert } from "lucide-react";
import type { ScriptTrust } from "../api";

/** Asks before running commands from a repo's own nekotrail.json, the first time and whenever they change. */
export function TrustSheet({ repo, trust, onAnswer }: { repo: string; trust: ScriptTrust; onAnswer: (run: boolean) => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onAnswer(false); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onAnswer]);
  const rows = (["setup", "run", "archive"] as const).filter((k) => trust[k]);
  return (
    <div className="modal-backdrop" onClick={() => onAnswer(false)}>
      <div className="modal trust" role="alertdialog" aria-modal="true" aria-label="Run this repo's commands?" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          <ShieldAlert size={16} />
          <h2>Run commands from {repo}?</h2>
        </div>
        <p className="muted">
          This repo's <code>nekotrail.json</code> asks nekotrail to run these commands on your Mac. Anyone who can push to the
          repo or open a PR can change them, so run them only if you trust what they do. You'll be asked again if they change.
        </p>
        <dl className="trust-list">
          {rows.map((k) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd><pre>{trust[k]}</pre></dd>
            </div>
          ))}
        </dl>
        <div className="row">
          <span className="spacer" />
          <button onClick={() => onAnswer(false)} autoFocus>Not now</button>
          <button className="primary" onClick={() => onAnswer(true)}>Trust and run</button>
        </div>
      </div>
    </div>
  );
}
