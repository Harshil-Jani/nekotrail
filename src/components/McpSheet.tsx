import { useEffect, useState } from "react";
import { ChevronLeft, CircleAlert, CircleCheck, Plug, RotateCw, SquareTerminal, X } from "lucide-react";
import { api, errText, type Agent, type McpOverview, type McpPreview, type McpRow, type SecretChoices, type SkillRow, type SkillShare } from "../api";
import { AGENT_INFO, AGENTS, agentName } from "../agents";
import { liveFor, parseMcp, scopeText, secretLabel, secretsAnswered } from "../mcpShare";

export { parseMcp };

/**
 * /mcp: the MCP servers and skills Claude, Codex and Gemini each have, and one-click sharing between
 * them. Sharing writes into the other agent's own config (never a proxy), after showing exactly what
 * goes in and backing the file up.
 */
export function McpSheet({ workspaceId, agent, onClose }: { workspaceId: string; agent: Agent; onClose: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const [liveError, setLiveError] = useState<string | null>(null);
  const [all, setAll] = useState<McpOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [share, setShare] = useState<{ row: McpRow; target: Agent } | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const load = () => {
    setText(null); setLiveError(null); setError(null);
    api.mcpList(workspaceId, agent).then(setText).catch((e) => setLiveError(errText(e)));
    api.mcpShareOverview(workspaceId).then(setAll).catch((e) => setError(errText(e)));
  };
  useEffect(load, [workspaceId, agent]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { if (share) setShare(null); else onClose(); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, share]);
  const live = text ? parseMcp(text) : [];
  // Every agent id is also its CLI name.
  const bin: Agent = AGENT_INFO[agent] ? agent : "claude";
  const term = (cmd: string) => { window.dispatchEvent(new CustomEvent("nekotrail-terminal", { detail: cmd })); onClose(); };
  const finished = (msg: string) => { setShare(null); setDone(msg); load(); };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal mcp" role="dialog" aria-modal="true" aria-label="MCP servers and skills" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          {share ? <button className="icon" onClick={() => setShare(null)} aria-label="Back" title="Back"><ChevronLeft size={15} /></button> : <Plug size={16} />}
          <h2>{share ? `Add ${share.row.name} to ${agentName(share.target)}` : "MCP servers and skills"}</h2>
          <span className="spacer" />
          {!share && <button className="icon" onClick={load} aria-label="Check again" title="Check again"><RotateCw size={14} /></button>}
          <button className="icon" onClick={onClose} aria-label="Close"><X size={15} /></button>
        </div>
        {share ? (
          <SharePanel workspaceId={workspaceId} row={share.row} target={share.target} onBack={() => setShare(null)} onDone={finished} />
        ) : (
          <div className="mcp-body">
            {done && <div className="mcp-done"><CircleCheck size={14} /> <span>{done}</span></div>}
            {error && <div className="mcp-note bad">{error}</div>}
            {all?.problems.map((p) => <div key={p} className="mcp-note bad">{p}</div>)}
            {!all && !error && <p className="muted"><span className="spinner" /> Reading your Claude, Codex and Gemini settings…</p>}
            {all && (
              <>
                <h3 className="mcp-h">Servers</h3>
                {all.servers.length === 0 && <p className="muted">None of your agents has an MCP server yet.</p>}
                {all.servers.length > 0 && (
                  <ul className="mcp-list">
                    {all.servers.map((row) => {
                      const l = liveFor(row, live);
                      return (
                        <li key={row.name}>
                          {l ? (l.ok ? <CircleCheck size={15} className="ok" aria-label={`${agentName(agent)}: ${l.status}`} /> : <CircleAlert size={15} className="bad" aria-label={`${agentName(agent)}: ${l.status}`} />) : <span className="mcp-dot" aria-hidden />}
                          <div>
                            <b>{row.name}</b>
                            <code className="muted" title={row.summary}>{row.summary || row.problem}</code>
                            {l && !l.ok && <span className="bad mcp-status">{agentName(agent)}: {l.status}</span>}
                          </div>
                          <div className="mcp-agents">
                            {AGENTS.map((a) => {
                              const has = row.has.filter((h) => h.agent === a);
                              if (has.length) {
                                return <span key={a} className="agent-chip has" title={has.map((h) => `${agentName(a)} has it for ${scopeText(h.scope)} (${h.file})`).join("\n")}>{agentName(a)}</span>;
                              }
                              const t = row.targets.find((x) => x.agent === a);
                              return (
                                <button key={a} className={t?.ok ? "add-chip" : "add-chip off"} title={t?.why ?? `Copy it into ${all.files[a]}`} onClick={() => { setDone(null); setShare({ row, target: a }); }}>
                                  Add to {agentName(a)}
                                </button>
                              );
                            })}
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
                {text !== null && live.length === 0 && text.trim() !== "" && (
                  <details className="mcp-raw-wrap"><summary className="muted">What {bin} says</summary><pre className="mcp-raw">{text}</pre></details>
                )}
                {liveError && <p className="muted">Couldn't ask {bin} which servers it reaches: {liveError}</p>}
                <h3 className="mcp-h">Skills</h3>
                <Skills skills={all.skills} onDone={(m) => { setDone(m); load(); }} />
              </>
            )}
          </div>
        )}
        {!share && (
          <div className="row">
            <span className="muted">Sharing copies a server into the other agent's own settings, so it works in your terminal too.</span>
            <span className="spacer" />
            <button onClick={() => term(`${bin} mcp`)}><SquareTerminal size={13} />Manage in terminal</button>
          </div>
        )}
      </div>
    </div>
  );
}

/** Shows exactly what goes into the other agent's file, asks about secrets, then writes it. */
function SharePanel({ workspaceId, row, target, onBack, onDone }: { workspaceId: string; row: McpRow; target: Agent; onBack: () => void; onDone: (msg: string) => void }) {
  const [choices, setChoices] = useState<SecretChoices>({});
  const [preview, setPreview] = useState<McpPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    api.mcpSharePreview(workspaceId, row.name, target, choices).then((p) => { if (live) { setPreview(p); setError(null); } }).catch((e) => live && setError(errText(e)));
    return () => { live = false; };
  }, [workspaceId, row.name, target, choices]);
  const secrets = preview?.secrets ?? row.secrets;
  const answered = secretsAnswered(secrets, choices);
  const blockers = preview?.blockers ?? [];
  const add = async () => {
    if (!preview) return;
    setBusy(true); setError(null);
    try {
      const r = await api.mcpShareApply(workspaceId, row.name, target, choices, preview.token);
      onDone(`Added ${row.name} to ${agentName(target)} in ${r.file}.${r.backup ? ` Your old file is at ${r.backup}.` : ""}`);
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };
  const from = `${agentName(row.from.agent)} (${scopeText(row.from.scope)}, ${row.from.file})`;
  return (
    <div className="mcp-body">
      {blockers.length > 0 ? (
        <p className="muted mcp-lead">{agentName(target)} can't take {row.name} from {from}:</p>
      ) : (
        <p className="muted mcp-lead">
          Copies it from {from} into {agentName(target)}'s own settings{preview ? `, ${preview.file}` : ""}, where it's on for all your folders.
          Nothing else in that file changes.
        </p>
      )}
      {secrets.length > 0 && blockers.length === 0 && (
        <div className="mcp-secrets">
          <b>Looks like a secret. How should it come across?</b>
          {secrets.map((s) => (
            <div key={s.id} className="mcp-secret">
              <span>{secretLabel(s)}</span>
              <div className="seg" role="radiogroup" aria-label={secretLabel(s)}>
                {s.can_ref && (
                  <button role="radio" aria-checked={choices[s.id] === "ref"} className={choices[s.id] === "ref" ? "on" : ""} onClick={() => setChoices({ ...choices, [s.id]: "ref" })} title="The value stays out of the file. You set it in your shell profile.">As a variable</button>
                )}
                <button role="radio" aria-checked={choices[s.id] === "copy"} className={choices[s.id] === "copy" ? "on" : ""} onClick={() => setChoices({ ...choices, [s.id]: "copy" })} title="The value is written into the other agent's file.">Copy the value</button>
              </div>
            </div>
          ))}
        </div>
      )}
      {blockers.map((b) => <div key={b} className="mcp-note bad">{b}</div>)}
      {error && <div className="mcp-note bad">{error}</div>}
      {preview && blockers.length === 0 && (
        <>
          <div className="mcp-snippet-head muted">
            {preview.creates ? `${preview.file} doesn't exist yet, so nekotrail creates it with this:` : `This goes into ${preview.file}:`}
          </div>
          <pre className="mcp-snippet">{preview.snippet}</pre>
          {secrets.some((s) => choices[s.id] === "copy") && <p className="muted mcp-small">Copied values are hidden here and written in full.</p>}
          {preview.notes.length > 0 && <ul className="mcp-notes">{preview.notes.map((n) => <li key={n}>{n}</li>)}</ul>}
          {!preview.creates && <p className="muted mcp-small">Your current file is backed up next to it first, as {preview.file.split("/").pop()}.bak-nekotrail-&lt;time&gt;.</p>}
        </>
      )}
      {!preview && !error && <p className="muted"><span className="spinner" /> Working out what to add…</p>}
      <div className="row">
        <span className="spacer" />
        <button onClick={onBack}>Back</button>
        {blockers.length === 0 && <button className="primary" disabled={!preview || busy || !answered || preview.blockers.length > 0 || preview.asks.length > 0} onClick={add}>
          {busy ? "Adding…" : `Add to ${agentName(target)}`}
        </button>}
      </div>
    </div>
  );
}

/** Skills by name, which agents see each, and a link to share one. */
function Skills({ skills, onDone }: { skills: SkillRow[]; onDone: (msg: string) => void }) {
  const [ask, setAsk] = useState<{ name: string; s: SkillShare } | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (skills.length === 0) return <p className="muted">No skills yet. Claude keeps them in ~/.claude/skills; Codex and Gemini read ~/.agents/skills.</p>;
  const link = async () => {
    if (!ask) return;
    try {
      const where = await api.skillShare(ask.name, ask.s.target);
      setAsk(null);
      onDone(`Linked ${ask.name} at ${where}.`);
    } catch (e) {
      setError(errText(e));
    }
  };
  return (
    <>
      <ul className="mcp-list">
        {skills.map((k) => (
          <li key={k.name} className="mcp-skill">
            <div className="mcp-skill-main">
              <span className="mcp-dot" aria-hidden />
              <div>
                <b>{k.name}</b>
                {k.description && <span className="muted mcp-desc" title={k.description}>{k.description}</span>}
              </div>
              <div className="mcp-agents">
                {AGENTS.filter((a) => k.has.includes(a)).map((a) => <span key={a} className="agent-chip has">{agentName(a)}</span>)}
                {k.share.map((s) => (
                  <button key={s.target} className={s.ok ? "add-chip" : "add-chip off"} title={s.why ?? `Link ${s.link} to ${s.to}`} onClick={() => { setError(null); setAsk({ name: k.name, s }); }}>{s.label}</button>
                ))}
              </div>
            </div>
            {ask?.name === k.name && (
              <div className="mcp-ask">
                {ask.s.ok ? (
                  <>
                    <span>Makes a link at <code>{ask.s.link}</code> to <code>{ask.s.to}</code>. It's one skill in one place, so an edit shows up for every agent.</span>
                    <div className="row">
                      <span className="spacer" />
                      <button onClick={() => setAsk(null)}>Cancel</button>
                      <button className="primary" onClick={link}>Link it</button>
                    </div>
                  </>
                ) : (
                  <div className="row"><span className="bad">{ask.s.why}</span><span className="spacer" /><button onClick={() => setAsk(null)}>OK</button></div>
                )}
                {error && <div className="bad">{error}</div>}
              </div>
            )}
          </li>
        ))}
      </ul>
      <p className="muted mcp-small">Codex and Gemini both read skills from ~/.agents/skills, so one link shares a Claude skill with both. Claude-only settings in a skill (like allowed-tools) are ignored there.</p>
    </>
  );
}
