import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Check, Copy, SquareTerminal, X } from "lucide-react";
import { api, errText } from "../api";
import { agentName } from "../agents";
import { commandsFrom, commandsText, type RanCommand } from "../commandsLog";

type ChatCommands = { id: string; chat: string; agent: string; commands: RanCommand[] };

/** Every shell command the agents ran in one workspace, chat by chat, from the chat history. */
export function CommandsLog({ workspaceId, name, onClose }: { workspaceId: string; name: string; onClose: () => void }) {
  const [chats, setChats] = useState<ChatCommands[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    (async () => {
      const sessions = await api.sessions(workspaceId);
      const all = await Promise.all(sessions.map(async (s) => ({ id: s.id, chat: s.title, agent: s.agent, commands: commandsFrom(await api.sessionEvents(s.id)) })));
      setChats(all);
    })().catch((e) => setError(errText(e)));
  }, [workspaceId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return (chats ?? []).map((c) => ({ ...c, commands: needle ? c.commands.filter((x) => x.command.toLowerCase().includes(needle)) : c.commands })).filter((c) => c.commands.length);
  }, [chats, q]);
  const total = chats?.reduce((n, c) => n + c.commands.length, 0) ?? 0;
  const failed = chats?.reduce((n, c) => n + c.commands.filter((x) => x.failed).length, 0) ?? 0;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal commands-log" role="dialog" aria-modal="true" aria-label="Commands the agent ran" onClick={(e) => e.stopPropagation()}>
        <div className="row"><SquareTerminal size={16} /><h2>Commands the agent ran</h2><span className="spacer" /><button className="icon" aria-label="Close" onClick={onClose}><X size={15} /></button></div>
        <p className="muted">
          {chats === null ? "Reading the chats…" : total === 0 ? `No shell commands in ${name} yet.` : `${total} ${total === 1 ? "command" : "commands"} in ${name}${failed ? `, ${failed} failed` : ""}. Click one to see what it printed.`}
        </p>
        {error && <div className="banner error">{error}</div>}
        {total > 0 && (
          <div className="row">
            <input className="commands-filter" value={q} placeholder="Filter commands" aria-label="Filter commands" onChange={(e) => setQ(e.target.value)} autoFocus />
            <button onClick={() => { navigator.clipboard.writeText(commandsText(shown)); setCopied(true); }}>{copied ? <Check size={13} /> : <Copy size={13} />}{copied ? "Copied" : "Copy all"}</button>
          </div>
        )}
        <div className="commands-list">
          {shown.map((c) => (
            <section key={c.id}>
              <div className="commands-chat"><b>{c.chat}</b><span className="muted">{agentName(c.agent)} · {c.commands.length}</span></div>
              {c.commands.map((x, i) => (
                <details key={i} className={`step ${x.failed ? "failed" : ""}`}>
                  <summary>
                    {x.failed ? <AlertCircle size={14} /> : x.pending ? <span className="muted">…</span> : <Check size={14} />}
                    <code>{x.command}</code>
                  </summary>
                  {x.output ? <pre>{x.output}</pre> : <p className="muted">{x.pending ? "No result: it was still running when the turn ended." : "It printed nothing."}</p>}
                </details>
              ))}
            </section>
          ))}
          {chats && total > 0 && shown.length === 0 && <p className="muted">No command matches "{q}".</p>}
        </div>
      </div>
    </div>
  );
}
