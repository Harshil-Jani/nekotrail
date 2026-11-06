import { useEffect, useState } from "react";
import { ArrowRightLeft, RotateCcw, ShieldCheck, Square, X } from "lucide-react";
import { api, errText, type Agent, type HandoffBrief, type Session } from "../api";
import { AGENT_INFO, agentName } from "../agents";
import { briefSize, defaultTarget, handoffTargets, modeNote, reasonLine } from "../handoff";
import { useInstalledAgents } from "../useInstalledAgents";
import { ModelPicker } from "./ModelPicker";

/**
 * "Hand off to another agent": pick who carries on, read and edit the brief nekotrail wrote from
 * this chat, then start a new chat with it in the same workspace. Nothing is sent until you press
 * Hand off; the brief is built on this Mac, with no model asked to summarise.
 */
export function HandoffSheet({ session, title, multiRepo, busy, initialAgent, initialModel, onClose, onStarted }: {
  session: Session;
  /** The workspace, for the header. */
  title: string;
  /** A multi-repo task: only agents that can open every repo are offered. */
  multiRepo: boolean;
  /** This chat's agent is working right now. */
  busy: boolean;
  initialAgent?: Agent | null;
  initialModel?: string | null;
  onClose: () => void;
  onStarted: (s: Session) => void;
}) {
  const installed = useInstalledAgents();
  const targets = handoffTargets(installed, session.agent, multiRepo);
  const [agent, setAgent] = useState<Agent | null>(null);
  const [model, setModel] = useState<{ model: string | null; effort: string | null }>({ model: initialModel ?? null, effort: null });
  const [brief, setBrief] = useState<HandoffBrief | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const from = agentName(session.agent);

  useEffect(() => {
    if (installed && !agent) setAgent(defaultTarget(targets, session.agent, initialAgent));
  }, [installed]);
  useEffect(() => {
    let live = true;
    api.handoffBrief(session.id)
      .then((b) => { if (live) { setBrief(b); setText(b.text); } })
      .catch((e) => live && setError(errText(e)));
    return () => { live = false; };
  }, [session.id]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const pick = (a: Agent) => {
    setAgent(a);
    // Models belong to an agent: a new agent starts on your default for it.
    if (a !== agent) setModel({ model: null, effort: null });
  };
  const size = brief ? briefSize(text, brief.max) : null;
  const edited = !!brief && text !== brief.text;
  const why = brief ? reasonLine(brief.reason, session.agent) : null;
  const blocked = !agent ? "Pick an agent to hand off to." : busy ? `${from} is still working. Stop it first.` : !text.trim() ? "The brief is empty." : null;

  const go = async () => {
    if (!agent || blocked) return;
    setSending(true);
    setError(null);
    try {
      const s = await api.handoffStart(session.id, agent, model.model, model.effort, text);
      onStarted(s);
    } catch (e) {
      setError(errText(e));
      setSending(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal handoff" role="dialog" aria-modal="true" aria-labelledby="handoff-title" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          <ArrowRightLeft size={16} />
          <h2 id="handoff-title">Hand off to another agent</h2>
          <span className="muted export-where">{title}</span>
          <span className="spacer" />
          <button className="icon" aria-label="Close" onClick={onClose}><X size={15} /></button>
        </div>
        {why && <div className="mcp-note handoff-why" role="note">{why}</div>}
        {error && <div className="mcp-note bad" role="alert">{error}</div>}

        <div className="handoff-pick">
          <span className="mcp-h">Continue with</span>
          {installed === null ? (
            <p className="muted export-small"><span className="spinner" /> Looking for agents…</p>
          ) : targets.length === 0 ? (
            <p className="muted export-small">
              {multiRepo
                ? "No other agent that can open every repo of this task is installed. Claude, Codex, Gemini and Copilot can."
                : `No other agent is installed. Install Codex (${AGENT_INFO.codex.install}) or another agent, then come back.`}
            </p>
          ) : (
            <div className="handoff-agents" role="radiogroup" aria-label="Agent">
              {targets.map((a) => (
                <button key={a} role="radio" aria-checked={a === agent} className={a === agent ? "on" : ""} onClick={() => pick(a)}>{AGENT_INFO[a].name}</button>
              ))}
            </div>
          )}
          {agent && (
            <div className="handoff-model row">
              <ModelPicker agent={agent} model={model.model} effort={model.effort} onChange={setModel} compact />
              <span className="muted export-small">{modeNote(session.permission_mode, agent)}</span>
            </div>
          )}
        </div>

        <div className="handoff-brief">
          <div className="row">
            <span className="mcp-h">Brief, sent as {agent ? `${agentName(agent)}'s` : "the"} first message</span>
            <span className="spacer" />
            {edited && <button className="quiet handoff-reset" onClick={() => setText(brief!.text)}><RotateCcw size={12} />Start over</button>}
          </div>
          {!brief && !error ? (
            <p className="muted export-loading"><span className="spinner" /> Reading the chat, the changes and the last checks…</p>
          ) : (
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              aria-label="Brief"
              spellCheck={false}
              aria-describedby="handoff-size"
            />
          )}
          {brief && size && (
            <p id="handoff-size" className="muted export-small handoff-meta">
              <span className={size.over ? "bad-text" : undefined}>{size.label}{size.over ? ". Long briefs leave the next agent less room for the work." : "."}</span>
              {brief.cut && !edited && <span> Older steps and long messages were shortened to fit.</span>}
              {brief.hidden > 0 && <span><ShieldCheck size={13} className="ok" /> {brief.hidden === 1 ? "1 thing that looks like a secret is" : `${brief.hidden} things that look like secrets are`} hidden.</span>}
            </p>
          )}
        </div>

        {busy && (
          <div className="mcp-note bad row handoff-busy" role="alert">
            <span>{from} is still working. Stop it first, so two agents don't change the same files at once.</span>
            <span className="spacer" />
            <button onClick={() => api.stop(session.id).catch((e) => setError(errText(e)))}><Square size={13} />Stop {from}</button>
          </div>
        )}

        <div className="row export-foot">
          <span className="muted export-small">A new chat opens in this workspace. Spend caps and protected files apply to it too, and this chat stays as it is.</span>
          <span className="spacer" />
          <button onClick={onClose}>Cancel</button>
          <button className="primary" disabled={!!blocked || !brief || sending} title={blocked ?? undefined} onClick={go}>
            {sending ? <span className="spinner" /> : <ArrowRightLeft size={14} />}
            {agent ? `Hand off to ${agentName(agent)}` : "Hand off"}
          </button>
        </div>
      </div>
    </div>
  );
}
