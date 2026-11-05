import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Brain, Check, Plus, FilePen, FileText, Globe, ListTodo, Search, Square, SquareTerminal, Users, Wrench, X } from "lucide-react";
import { api, errText, type Agent, type ChatEvent, type PermissionMode, type Session } from "../api";
import { Mascot } from "./Mascot";

const TOOL_ICON: Record<string, typeof Wrench> = {
  Bash: SquareTerminal, Read: FileText, Write: FilePen, Edit: FilePen, MultiEdit: FilePen, NotebookEdit: FilePen,
  Grep: Search, Glob: Search, WebFetch: Globe, WebSearch: Globe, Task: Users, Agent: Users, TodoWrite: ListTodo,
};

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "plan", label: "Plan (read only)" },
  { value: "acceptEdits", label: "Auto-accept edits" },
  { value: "bypassPermissions", label: "Full auto" },
];

// Suggestions only: any model id the CLI accepts can be typed in.
const MODELS: Record<Agent, string[]> = {
  claude: ["opus", "sonnet", "haiku"],
  codex: [],
};

type Row =
  | { type: "user" | "text" | "thinking" | "error"; text: string }
  | { type: "tool"; name: string; input: string; output?: string; is_error?: boolean; pending: boolean }
  | { type: "done"; ev: Extract<ChatEvent, { kind: "done" }> };

/** Pair each tool call with its result so one row shows both. */
function toRows(events: ChatEvent[]): Row[] {
  const rows: Row[] = [];
  const toolIdx = new Map<string, number>();
  for (const ev of events) {
    switch (ev.kind) {
      case "tool":
        toolIdx.set(ev.id, rows.length);
        rows.push({ type: "tool", name: ev.name, input: ev.input, pending: true });
        break;
      case "tool_result": {
        const i = toolIdx.get(ev.id);
        if (i !== undefined) {
          const r = rows[i] as Extract<Row, { type: "tool" }>;
          rows[i] = { ...r, output: ev.output, is_error: ev.is_error, pending: false };
        }
        break;
      }
      case "done":
        rows.push({ type: "done", ev });
        break;
      default:
        rows.push({ type: ev.kind, text: ev.text });
    }
  }
  return rows;
}

/** One chat row. Memoized: a new message renders only the new row, not the whole history. */
const RowView = memo(function RowView({ r, pendingSpin }: { r: Row; pendingSpin: boolean }) {
  switch (r.type) {
    case "user":
      return <div className="msg user">{r.text}</div>;
    case "text":
      return (
        <div className="msg assistant">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{r.text}</ReactMarkdown>
        </div>
      );
    case "thinking":
      return (
        <details className="step">
          <summary><Brain size={16} /><b>Thinking</b></summary>
          <div className="body">{r.text}</div>
        </details>
      );
    case "tool": {
      const Icon = r.is_error ? AlertCircle : TOOL_ICON[r.name] ?? Wrench;
      return (
        <details className={`step ${r.is_error ? "failed" : ""}`}>
          <summary>
            {r.pending && pendingSpin ? <span className="spinner" /> : <Icon size={16} />}
            <b>{r.name}</b> <code>{r.input}</code>
          </summary>
          {r.output ? <pre>{r.output}</pre> : null}
        </details>
      );
    }
    case "error":
      return <div className="msg error">{r.text}</div>;
    case "done":
      return (
        <div className="msg done num">
          {r.ev.stopped ? <Square size={14} /> : r.ev.is_error ? <X size={14} /> : <Check size={14} />}
          {r.ev.stopped ? "Stopped" : r.ev.is_error ? "Ended with an error" : "Done"}
          {r.ev.duration_ms ? ` · ${(r.ev.duration_ms / 1000).toFixed(1)}s` : ""}
          {r.ev.cost_usd ? ` · $${r.ev.cost_usd.toFixed(2)}` : ""}
        </div>
      );
  }
}, (a, b) => {
  // Rows are rebuilt on every new event; compare their fields so unchanged rows skip rendering.
  if (a.pendingSpin !== b.pendingSpin) return false;
  const x = a.r as Record<string, unknown>, y = b.r as Record<string, unknown>;
  const keys = Object.keys(x);
  return keys.length === Object.keys(y).length && keys.every((k) => x[k] === y[k]);
});

const PAGE = 150; // rows rendered at a time; older rows load on demand

export function Chat({
  workspaceId,
  running,
  onRunningChange,
  inject,
}: {
  workspaceId: string;
  running: Set<string>;
  onRunningChange: () => void;
  /** Text to drop into the composer (from PR buttons), for the user to review and send. */
  inject?: { text: string; nonce: number };
}) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [events, setEvents] = useState<ChatEvent[]>([]);
  const [queue, setQueue] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [menu, setMenu] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const session = sessions.find((s) => s.id === active) ?? null;
  const busy = !!active && running.has(active);

  const loadSessions = async (pick?: string) => {
    let list = await api.sessions(workspaceId);
    if (list.length === 0) list = [await api.newSession(workspaceId, "claude")];
    setSessions(list);
    setActive((cur) => pick ?? (cur && list.some((s) => s.id === cur) ? cur : list[list.length - 1].id));
  };

  useEffect(() => {
    setActive(null);
    loadSessions().catch((e) => setError(errText(e)));
  }, [workspaceId]);

  useEffect(() => {
    if (!active) return;
    setEvents([]);
    api.sessionEvents(active).then(setEvents);
    api.queued(active).then(setQueue);
    const un1 = listen<{ session_id: string; event: ChatEvent }>("agent", ({ payload }) => {
      if (payload.session_id === active) setEvents((e) => [...e, payload.event]);
    });
    const un2 = listen<{ session_id: string; queued: string[] }>("queue", ({ payload }) => {
      if (payload.session_id === active) setQueue(payload.queued);
    });
    const un3 = listen("running", () => {
      onRunningChange();
      api.sessions(workspaceId).then(setSessions);
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      un3.then((f) => f());
    };
  }, [active]);

  const stick = useRef(true); // follow new output only while the reader is at the bottom
  useEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [events.length]);
  useEffect(() => { stick.current = true; }, [active]);

  useEffect(() => {
    if (inject?.text) setDraft(inject.text);
  }, [inject?.nonce]);

  const rows = useMemo(() => toRows(events), [events]);
  const [limit, setLimit] = useState(PAGE);
  useEffect(() => setLimit(PAGE), [active]);
  const hidden = Math.max(0, rows.length - limit);
  const visible = useMemo(() => rows.slice(hidden), [rows, hidden]);
  // Keep the reader's place when older rows are added above.
  const anchor = useRef<number | null>(null);
  const showEarlier = () => {
    anchor.current = scroller.current ? scroller.current.scrollHeight - scroller.current.scrollTop : null;
    setLimit((l) => l + PAGE);
  };
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && anchor.current !== null) {
      el.scrollTop = el.scrollHeight - anchor.current;
      anchor.current = null;
    }
  }, [limit]);

  const submit = async () => {
    const text = draft.trim();
    if (!text || !active) return;
    setDraft("");
    setError(null);
    try {
      await api.send(active, text);
      onRunningChange();
    } catch (e) {
      setError(errText(e));
    }
  };

  const update = async (patch: Partial<Pick<Session, "model" | "permission_mode" | "title">>) => {
    if (!session) return;
    const next = { ...session, ...patch };
    await api.updateSession(next.id, next.title, next.model, next.permission_mode);
    setSessions((ss) => ss.map((s) => (s.id === next.id ? next : s)));
  };

  const newChat = async (agent: Agent) => {
    const s = await api.newSession(workspaceId, agent);
    await loadSessions(s.id);
  };

  return (
    <div className="chat">
      <div className="chat-tabs" role="tablist" aria-label="Chats">
        {sessions.map((s) => (
          <button key={s.id} role="tab" aria-selected={s.id === active} className={`chat-tab ${s.id === active ? "active" : ""}`} onClick={() => setActive(s.id)} title={s.title}>
            {running.has(s.id) && <span className="spinner" />}
            {s.agent === "codex" ? "Codex · " : ""}{s.title}
          </button>
        ))}
        <span className="newchat">
          <button className="icon" title="New chat" aria-label="New chat" aria-expanded={menu} onClick={() => setMenu((m) => !m)}><Plus size={15} /></button>
          {menu && (
            <span className="newchat-menu" role="menu" onMouseLeave={() => setMenu(false)}>
              <button role="menuitem" onClick={() => { setMenu(false); newChat("claude"); }}>New Claude chat</button>
              <button role="menuitem" onClick={() => { setMenu(false); newChat("codex"); }}>New Codex chat</button>
            </span>
          )}
        </span>
      </div>

      <div
        className="messages"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        {rows.length === 0 && (
          <div className="empty">
            <Mascot size={64} mood="sleep" />
            <h2>{session?.agent_session_id ? "Pick up where you left off" : "What should we build?"}</h2>
            <p>
              {session?.agent_session_id
                ? "This chat came over from Conductor. New messages continue the same agent session."
                : `Describe a change, a bug or a plan. ${session?.agent === "codex" ? "Codex" : "Claude"} works in this workspace's own branch, so nothing touches your main checkout.`}
            </p>
          </div>
        )}
        <div className="thread">
          {hidden > 0 && (
            <button className="quiet earlier" onClick={showEarlier}>
              Show {Math.min(PAGE, hidden)} earlier ({hidden} hidden)
            </button>
          )}
          {visible.map((r, k) => (
            <RowView key={hidden + k} r={r} pendingSpin={busy && hidden + k === rows.length - 1} />
          ))}
        </div>
      </div>

      {queue.length > 0 && (
        <div className="queue">
          <span className="spinner" />
          <span>{queue.length} queued, runs next: {queue[0].slice(0, 80)}</span>
          <button className="quiet" onClick={() => active && api.clearQueue(active)}>Clear queue</button>
        </div>
      )}
      {error && <div className="banner error">{error}</div>}

      <div className="composer">
        <div className="composer-box">
          <textarea
            value={draft}
            aria-label="Message"
            placeholder={busy ? "Working… anything you send now runs next." : "Ask for a change, a fix or a plan"}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                submit();
              }
            }}
            rows={3}
          />
          <div className="composer-bar">
            {session && (
              <>
                <span className="agent-badge">{session.agent === "codex" ? "Codex" : "Claude"}</span>
                <input
                  className="model"
                  list={`models-${session.agent}`}
                  placeholder="Default model"
                  defaultValue={session.model ?? ""}
                  key={session.id}
                  onBlur={(e) => update({ model: e.target.value.trim() || null })}
                  aria-label="Model"
                  title="Model (blank uses the CLI's default)"
                />
                <datalist id={`models-${session.agent}`}>
                  {MODELS[session.agent].map((m) => <option key={m} value={m} />)}
                </datalist>
                <select
                  value={session.permission_mode}
                  onChange={(e) => update({ permission_mode: e.target.value as PermissionMode })}
                  aria-label="Permissions"
                  title="What the agent may do without asking"
                >
                  {MODES.map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
                </select>
              </>
            )}
            <span className="spacer" />
            <span className="muted" style={{ fontSize: 13 }}>⌘↵</span>
            {busy && <button className="danger" onClick={() => active && api.stop(active)}><Square size={14} />Stop</button>}
            <button className="accent" onClick={submit} disabled={!draft.trim()}>{busy ? "Queue" : "Send"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
