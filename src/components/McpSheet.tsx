import { useEffect, useState } from "react";
import { CircleAlert, CircleCheck, Plug, RotateCw, SquareTerminal, X } from "lucide-react";
import { api, errText, type Agent } from "../api";

type Server = { name: string; target: string; ok: boolean; status: string };

/** `claude mcp list` prints "name: target - ✔ Connected"; Codex prints a table. Parse what we can, show the rest raw. */
export function parseMcp(text: string): Server[] {
  const out: Server[] = [];
  for (const l of text.split("\n")) {
    const m = /^(.+?):\s+(.+?)\s+-\s+([✔✓✗✘!⚠])\s*(.+)$/.exec(l.trim());
    if (m) out.push({ name: m[1], target: m[2], ok: m[3] === "✔" || m[3] === "✓", status: m[4] });
  }
  return out;
}

/** /mcp: the MCP servers this agent can use in this workspace, and where to change them. */
export function McpSheet({ workspaceId, agent, onClose }: { workspaceId: string; agent: Agent; onClose: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => { setText(null); setError(null); api.mcpList(workspaceId, agent).then(setText).catch((e) => setError(errText(e))); };
  useEffect(load, [workspaceId, agent]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const servers = text ? parseMcp(text) : [];
  const bin = agent === "codex" ? "codex" : "claude";
  const term = (cmd: string) => { window.dispatchEvent(new CustomEvent("nekotrail-terminal", { detail: cmd })); onClose(); };
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal mcp" role="dialog" aria-modal="true" aria-label="MCP servers" onClick={(e) => e.stopPropagation()}>
        <div className="row">
          <Plug size={16} />
          <h2>MCP servers for {agent === "codex" ? "Codex" : "Claude"}</h2>
          <span className="spacer" />
          <button className="icon" onClick={load} aria-label="Check again" title="Check again"><RotateCw size={14} /></button>
          <button className="icon" onClick={onClose} aria-label="Close"><X size={15} /></button>
        </div>
        {text === null && !error && <p className="muted"><span className="spinner" /> Asking {bin} which servers it can reach…</p>}
        {error && <div className="banner error">{error}</div>}
        {servers.length > 0 && (
          <ul className="mcp-list">
            {servers.map((s) => (
              <li key={s.name}>
                {s.ok ? <CircleCheck size={15} className="ok" /> : <CircleAlert size={15} className="bad" />}
                <div><b>{s.name}</b><code className="muted">{s.target}</code></div>
                <span className={s.ok ? "muted" : "bad"}>{s.status}</span>
              </li>
            ))}
          </ul>
        )}
        {text !== null && servers.length === 0 && <pre className="mcp-raw">{text || "No MCP servers configured."}</pre>}
        <div className="row">
          <span className="muted">Servers come from your {bin} settings, so every workspace and your terminal see the same ones.</span>
          <span className="spacer" />
          <button onClick={() => term(`${bin} mcp`)}><SquareTerminal size={13} />Manage in terminal</button>
        </div>
      </div>
    </div>
  );
}
