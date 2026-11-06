import { useEffect } from "react";
import { ShieldAlert } from "lucide-react";
import type { AgentConfigTrust, ScriptTrust } from "../api";
import { agentName } from "../agents";

/**
 * Asks before running commands a repo brings with it, the first time and whenever they change:
 * the setup/run commands in its nekotrail.json, or the agent config (hooks, MCP servers, env) that
 * Claude, Codex or Gemini would run as soon as a chat starts.
 */
export function TrustSheet({ repo, trust, agent, onAnswer }: { repo: string; trust?: ScriptTrust; agent?: AgentConfigTrust; onAnswer: (run: boolean) => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onAnswer(false); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onAnswer]);
  // Gemini has no way to run with a repo's settings off, so "Not now" means no Gemini here.
  const gemini = agent?.items.some((it) => it.agent === "gemini");
  const rows = trust ? (["setup", "run", "archive"] as const).filter((k) => trust[k]) : [];
  return (
    <div className="modal-backdrop" onClick={() => onAnswer(false)}>
      <div className="modal trust" role="alertdialog" aria-modal="true" aria-labelledby="trust-title" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          <ShieldAlert size={16} />
          <h2 id="trust-title">{agent ? `Let ${repo}'s agent settings run?` : `Run commands from ${repo}?`}</h2>
        </div>
        {agent ? (
          <p className="muted">
            This repo commits settings that make the agent run these on your Mac as soon as a chat starts. Anyone who can push
            to the repo or open a PR can change them. {gemini
              ? <><b>Not now</b> means Gemini won't run in this repo. Claude and Codex still can.</>
              : <><b>Not now</b> still lets you chat: the agent runs without this repo's agent settings (yours stay on).</>}
            {" "}You'll be asked again if they change.
          </p>
        ) : (
          <p className="muted">
            This repo's <code>nekotrail.json</code> asks nekotrail to run these commands on your Mac. Anyone who can push to the
            repo or open a PR can change them, so run them only if you trust what they do. You'll be asked again if they change.
          </p>
        )}
        <dl className={agent ? "trust-list stacked" : "trust-list"}>
          {rows.map((k) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd><pre>{trust![k]}</pre></dd>
            </div>
          ))}
          {agent?.items.map((it, i) => (
            <div key={i}>
              <dt>{agentName(it.agent)}: {it.what} <span className="muted">in {it.file}</span></dt>
              <dd><pre>{it.command}</pre></dd>
            </div>
          ))}
        </dl>
        <div className="row">
          <span className="spacer" />
          <button onClick={() => onAnswer(false)} autoFocus>Not now</button>
          <button className="primary" onClick={() => onAnswer(true)}>{agent ? "Trust and send" : "Trust and run"}</button>
        </div>
      </div>
    </div>
  );
}
