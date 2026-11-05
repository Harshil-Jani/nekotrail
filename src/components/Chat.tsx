import { createContext, memo, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Brain, Check, History, Paperclip, Plus, FilePen, FileText, Globe, ListTodo, Search, Square, SquareTerminal, Users, Wrench, X } from "lucide-react";
import { api, errText, type Agent, type ChatEvent, type Effort, type PermissionMode, type Session } from "../api";
import { Mascot } from "./Mascot";
import { getSettings } from "../settings";

/** Rank files for an @-mention: name matches beat path matches, shorter paths win ties. */
export function matchFiles(files: string[], q: string, max = 8): string[] {
  const query = q.toLowerCase();
  const scored: [number, string][] = [];
  for (const f of files) {
    const lower = f.toLowerCase();
    const base = lower.slice(lower.lastIndexOf("/") + 1);
    let score: number;
    if (!query) score = 3;
    else if (base.startsWith(query)) score = 0;
    else if (base.includes(query)) score = 1;
    else if (lower.includes(query)) score = 2;
    else {
      // Subsequence match ("srvhttp" finds server/http.go).
      let i = 0;
      for (const ch of lower) if (ch === query[i]) i++;
      if (i < query.length) continue;
      score = 3;
    }
    scored.push([score * 1000 + f.length, f]);
  }
  return scored.sort((a, b) => a[0] - b[0]).slice(0, max).map(([, f]) => f);
}

const TOOL_ICON: Record<string, typeof Wrench> = {
  Bash: SquareTerminal, Read: FileText, Write: FilePen, Edit: FilePen, MultiEdit: FilePen, NotebookEdit: FilePen,
  Grep: Search, Glob: Search, WebFetch: Globe, WebSearch: Globe, Task: Users, Agent: Users, TodoWrite: ListTodo,
};

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "plan", label: "Plan (read only)" },
  { value: "acceptEdits", label: "Auto-accept edits" },
  { value: "bypassPermissions", label: "Full auto" },
];

const EFFORTS: Record<Agent, { value: Effort; label: string }[]> = {
  claude: [
    { value: "", label: "Effort: default" }, { value: "low", label: "Low effort" }, { value: "medium", label: "Medium effort" },
    { value: "high", label: "High effort" }, { value: "xhigh", label: "Extra high effort" }, { value: "max", label: "Max effort" },
  ],
  codex: [
    { value: "", label: "Reasoning: default" }, { value: "low", label: "Low reasoning" }, { value: "medium", label: "Medium reasoning" },
    { value: "high", label: "High reasoning" }, { value: "xhigh", label: "Extra high reasoning" },
  ],
};

/** Ring that fills as the conversation uses up the model's context window. */
function ContextMeter({ tokens, window }: { tokens: number; window: number }) {
  const pct = Math.min(1, tokens / window);
  const r = 7, c = 2 * Math.PI * r;
  const tone = pct > 0.85 ? "var(--bad)" : pct > 0.7 ? "var(--wait)" : "var(--muted)";
  const k = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}k` : String(n));
  return (
    <span className="ctx" title={`Context: ${k(tokens)} of ${k(window)} tokens (${Math.round(pct * 100)}%). Start a new chat or ask the agent to summarise when it fills up.`}>
      <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden>
        <circle cx="9" cy="9" r={r} fill="none" stroke="var(--line)" strokeWidth="2.5" />
        <circle cx="9" cy="9" r={r} fill="none" stroke={tone} strokeWidth="2.5" strokeDasharray={`${pct * c} ${c}`} transform="rotate(-90 9 9)" />
      </svg>
      {pct >= 0.5 && <span className="num" style={{ color: tone }}>{Math.round(pct * 100)}%</span>}
    </span>
  );
}

// Suggestions only: any model id the CLI accepts can be typed in.
const MODELS: Record<Agent, string[]> = {
  claude: ["opus", "sonnet", "haiku"],
  codex: [],
};

type Row =
  | { type: "user"; text: string; checkpoint?: string | null }
  | { type: "text" | "thinking" | "error" | "note"; text: string }
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
      case "user":
        rows.push({ type: "user", text: ev.text, checkpoint: ev.checkpoint });
        break;
      default:
        rows.push({ type: ev.kind, text: ev.text });
    }
  }
  return rows;
}

/** Rewind handler shared through context, so memoized rows don't re-render when it changes. */
const RewindCtx = createContext<{ rewind: (checkpoint: string) => Promise<void>; disabled: boolean }>({
  rewind: async () => {},
  disabled: true,
});

/** Two-step button: the first click arms it, the second (within 4s) rewinds. No modal needed. */
function RewindButton({ checkpoint }: { checkpoint: string }) {
  const { rewind, disabled } = useContext(RewindCtx);
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(t);
  }, [armed]);
  return (
    <button
      className={`rewind ${armed ? "armed" : ""}`}
      disabled={disabled || busy}
      title={disabled ? "Stop the agent to rewind" : "Undo every file change made from this message on"}
      onClick={async () => {
        if (!armed) return setArmed(true);
        setBusy(true);
        try { await rewind(checkpoint); } finally { setBusy(false); setArmed(false); }
      }}
    >
      <History size={13} />
      {armed ? "Click again to undo the changes" : "Rewind files to here"}
    </button>
  );
}

/** One chat row. Memoized: a new message renders only the new row, not the whole history. */
const RowView = memo(function RowView({ r, pendingSpin }: { r: Row; pendingSpin: boolean }) {
  switch (r.type) {
    case "user":
      return (
        <div className="msg-user-wrap">
          <div className="msg user">{r.text}</div>
          {r.checkpoint && <RewindButton checkpoint={r.checkpoint} />}
        </div>
      );
    case "note":
      return <div className="msg note"><History size={14} />{r.text}</div>;
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
  const input = useRef<HTMLTextAreaElement>(null);
  const session = sessions.find((s) => s.id === active) ?? null;
  const busy = !!active && running.has(active);
  const anyBusy = sessions.some((s) => running.has(s.id));

  // @-mentions: the file list loads the first time "@" is typed in this workspace.
  const files = useRef<string[] | null>(null);
  useEffect(() => { files.current = null; }, [workspaceId]);
  const [mention, setMention] = useState<{ q: string; start: number; items: string[]; sel: number } | null>(null);
  const updateMention = async (text: string, caret: number) => {
    const m = /(^|\s)@([^\s@]*)$/.exec(text.slice(0, caret));
    if (!m) return setMention(null);
    if (!files.current) files.current = await api.workspaceFiles(workspaceId).catch(() => []);
    const items = matchFiles(files.current, m[2]);
    setMention(items.length ? { q: m[2], start: caret - m[2].length - 1, items, sel: 0 } : null);
  };
  const pickMention = (path: string) => {
    if (!mention) return;
    const el = input.current;
    const end = mention.start + 1 + mention.q.length;
    const next = `${draft.slice(0, mention.start)}@${path} ${draft.slice(end)}`;
    setDraft(next);
    setMention(null);
    const caret = mention.start + path.length + 2;
    requestAnimationFrame(() => { el?.focus(); el?.setSelectionRange(caret, caret); });
  };

  const insertAtCaret = (text: string) => {
    const el = input.current;
    const at = el?.selectionStart ?? draft.length;
    const pad = at > 0 && !/\s$/.test(draft.slice(0, at)) ? " " : "";
    setDraft(draft.slice(0, at) + pad + text + " " + draft.slice(at));
  };
  const attach = async (list: FileList | File[]) => {
    for (const f of Array.from(list)) {
      try {
        const rel = await api.saveAttachment(workspaceId, f);
        insertAtCaret(`@${rel}`);
      } catch (e) {
        setError(errText(e));
      }
    }
  };

  const rewindCtx = useMemo(() => ({
    disabled: anyBusy,
    rewind: async (checkpoint: string) => {
      if (!active) return;
      setError(null);
      try { await api.restoreCheckpoint(active, checkpoint); } catch (e) { setError(errText(e)); }
    },
  }), [anyBusy, active]);

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
    const un4 = listen<{ session_id: string; context_tokens: number }>("usage", ({ payload }) => {
      if (payload.session_id === active) setLiveTokens(payload.context_tokens);
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      un3.then((f) => f());
      un4.then((f) => f());
    };
  }, [active]);

  // A git action (Create PR, Review...) messaged one of this workspace's chats: show it.
  useEffect(() => {
    const un = listen<{ workspace_id: string; session_id: string }>("focus-session", ({ payload }) => {
      if (payload.workspace_id === workspaceId) loadSessions(payload.session_id);
    });
    return () => { un.then((f) => f()); };
  }, [workspaceId]);

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
  const [liveTokens, setLiveTokens] = useState<number | null>(null);
  useEffect(() => setLiveTokens(null), [active]);
  // Latest context size: live while a turn runs, else from the last finished turn.
  const context = useMemo(() => {
    for (let i = events.length - 1; i >= 0; i--) {
      const ev = events[i];
      if (ev.kind === "done" && ev.context_window) return { tokens: ev.context_tokens ?? 0, window: ev.context_window };
    }
    return null;
  }, [events]);
  const ctxWindow = context?.window ?? (session?.agent === "claude" ? 200_000 : 0);
  const ctxTokens = liveTokens ?? context?.tokens ?? 0;
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

  const update = async (patch: Partial<Pick<Session, "model" | "permission_mode" | "title" | "effort">>) => {
    if (!session) return;
    const next = { ...session, ...patch };
    if ("effort" in patch) await api.setSessionEffort(next.id, next.effort || null);
    else await api.updateSession(next.id, next.title, next.model, next.permission_mode);
    setSessions((ss) => ss.map((s) => (s.id === next.id ? next : s)));
  };

  const closeChat = async (id: string) => {
    try {
      await api.hideSession(id);
      await loadSessions(active === id ? undefined : active ?? undefined);
    } catch (e) {
      setError(errText(e));
    }
  };

  const newChat = async (agent: Agent) => {
    const s = await api.newSession(workspaceId, agent);
    await loadSessions(s.id);
  };

  return (
    <div className="chat">
      <div className="chat-tabs" role="tablist" aria-label="Chats">
        {sessions.map((s) => (
          <span key={s.id} className={`chat-tab ${s.id === active ? "active" : ""}`}>
            <button role="tab" aria-selected={s.id === active} onClick={() => setActive(s.id)} title={s.title}>
              {running.has(s.id) && <span className="spinner" />}
              {s.agent === "codex" ? "Codex · " : ""}{s.title}
            </button>
            {sessions.length > 1 && !running.has(s.id) && (
              <button className="tab-close" aria-label={`Close ${s.title}`} title="Close chat (history is kept)" onClick={() => closeChat(s.id)}><X size={12} /></button>
            )}
          </span>
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
        <RewindCtx.Provider value={rewindCtx}>
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
        </RewindCtx.Provider>
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
          {mention && (
            <div className="mention" role="listbox" aria-label="Files">
              {mention.items.map((f, i) => {
                const cut = f.lastIndexOf("/") + 1;
                return (
                  <button
                    key={f}
                    role="option"
                    aria-selected={i === mention.sel}
                    className={i === mention.sel ? "on" : ""}
                    onMouseDown={(e) => { e.preventDefault(); pickMention(f); }}
                  >
                    <FileText size={13} />
                    <b>{f.slice(cut)}</b>
                    <span className="muted">{f.slice(0, cut)}</span>
                  </button>
                );
              })}
            </div>
          )}
          <textarea
            ref={input}
            value={draft}
            aria-label="Message"
            placeholder={busy ? "Working… anything you send now runs next." : "Ask for a change, a fix or a plan. @ to mention a file, paste a screenshot."}
            onChange={(e) => { setDraft(e.target.value); updateMention(e.target.value, e.target.selectionStart); }}
            onBlur={() => setMention(null)}
            onPaste={(e) => {
              if (e.clipboardData.files.length) {
                e.preventDefault();
                attach(e.clipboardData.files);
              }
            }}
            onKeyDown={(e) => {
              if (mention) {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  const d = e.key === "ArrowDown" ? 1 : -1;
                  setMention({ ...mention, sel: (mention.sel + d + mention.items.length) % mention.items.length });
                  return;
                }
                if (e.key === "Enter" || e.key === "Tab") {
                  e.preventDefault();
                  pickMention(mention.items[mention.sel]);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setMention(null);
                  return;
                }
              }
              const enterSends = getSettings().sendKey === "enter";
              if (e.key === "Enter" && !e.nativeEvent.isComposing && (enterSends ? !e.shiftKey && !e.metaKey : e.metaKey || e.ctrlKey)) {
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
                <select
                  value={session.effort ?? ""}
                  onChange={(e) => update({ effort: (e.target.value || null) as Effort | null })}
                  aria-label="Effort"
                  title="How hard the model thinks. Higher is slower and costs more."
                >
                  {EFFORTS[session.agent].map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
                </select>
                {ctxWindow > 0 && ctxTokens > 0 && <ContextMeter tokens={ctxTokens} window={ctxWindow} />}
              </>
            )}
            <label className="icon attach" title="Attach files (or paste a screenshot)">
              <Paperclip size={15} />
              <input type="file" multiple hidden onChange={(e) => { if (e.target.files) attach(e.target.files); e.target.value = ""; }} />
            </label>
            <span className="spacer" />
            <span className="muted" style={{ fontSize: 13 }}>{getSettings().sendKey === "enter" ? "↵" : "⌘↵"}</span>
            {busy && <button className="danger" onClick={() => active && api.stop(active)}><Square size={14} />Stop</button>}
            <button className="accent" onClick={submit} disabled={!draft.trim()}>{busy ? "Queue" : "Send"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
