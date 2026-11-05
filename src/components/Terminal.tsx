import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { Play, Plus, RotateCw, Square, SquareTerminal, Wrench, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";
import { api } from "../api";

/**
 * One terminal view. `shell` starts a login shell (or reattaches to the running one);
 * `attach` only shows a process started elsewhere (run and setup scripts).
 * Output printed while the view was closed is replayed from the backend's scrollback.
 */
export function Terminal({ ptyId, cwd, mode = "shell", nonce = 0 }: { ptyId: string; cwd: string; mode?: "shell" | "attach"; nonce?: number }) {
  const host = useRef<HTMLDivElement>(null);
  const [idle, setIdle] = useState(false);

  useEffect(() => {
    const t = new XTerm({
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      cursorBlink: mode === "shell",
      scrollback: 5000,
      theme: { background: "#000000", foreground: "#d8dee9", cursor: "#ff9a4d", selectionBackground: "#ff9a4d55" },
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(host.current!);
    fit.fit();
    let live = true;

    const onData = t.onData((d) => api.ptyWrite(ptyId, d));
    const un = listen<{ id: string; data: string }>("pty", ({ payload }) => {
      if (payload.id === ptyId) { setIdle(false); t.write(payload.data); }
    });
    const unExit = listen<{ id: string }>("pty-exit", ({ payload }) => {
      if (payload.id !== ptyId) return;
      t.write(mode === "shell" ? "\r\n\x1b[2m[shell exited; open a new tab to start another]\x1b[0m\r\n" : "\r\n\x1b[2m[finished]\x1b[0m\r\n");
    });
    const start = mode === "shell" ? api.ptyOpen(ptyId, cwd, t.cols, t.rows) : api.ptyAttach(ptyId);
    start.then((sb) => {
      if (!live) return;
      if (sb) t.write(sb);
      if (mode === "attach" && sb === null) setIdle(true);
      api.ptyResize(ptyId, t.cols, t.rows).catch(() => {});
    });

    const ro = new ResizeObserver(() => {
      if (!host.current?.offsetWidth) return; // hidden tab: keep the last size
      fit.fit();
      api.ptyResize(ptyId, t.cols, t.rows).catch(() => {});
    });
    ro.observe(host.current!);
    return () => {
      live = false;
      ro.disconnect();
      onData.dispose();
      un.then((f) => f());
      unExit.then((f) => f());
      t.dispose();
    };
  }, [ptyId, cwd, mode, nonce]);

  return (
    <div className="terminal-wrap">
      {idle && <p className="terminal-idle muted">Not running. Start it with the button above.</p>}
      <div className="terminal" ref={host} />
    </div>
  );
}

type Tab = { id: string; label: string; kind: "shell" | "run" | "setup" };

/** Terminal tabs for a workspace: its setup log, its run script, and as many shells as you like. */
export function TerminalPanel({
  wsId,
  cwd,
  running,
  focus,
  onRun,
  onSetup,
}: {
  wsId: string;
  cwd: string;
  /** Run and setup processes that are alive, by pty id. */
  running: Set<string>;
  /** Switch to a tab ("run", "setup" or a shell id). */
  focus?: { tab: string; nonce: number };
  onRun: () => void;
  onSetup: () => void;
}) {
  const key = `nekotrail.terms.${wsId}`;
  const [shells, setShells] = useState<string[]>(() => JSON.parse(localStorage.getItem(key) || "null") ?? [`${wsId}:term`]);
  const [active, setActive] = useState<string>(shells[0]);
  const [seen, setSeen] = useState<{ run: boolean; setup: boolean }>({ run: false, setup: false });
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => { localStorage.setItem(key, JSON.stringify(shells)); }, [shells]);
  const runId = `${wsId}:run`, setupId = `${wsId}:setup`;

  // Show the Run / Setup tabs once they have been used in this session (or are running now).
  useEffect(() => {
    api.ptyAttach(runId).then((s) => s !== null && setSeen((x) => ({ ...x, run: true }))).catch(() => {});
    api.ptyAttach(setupId).then((s) => s !== null && setSeen((x) => ({ ...x, setup: true }))).catch(() => {});
  }, [wsId]);
  useEffect(() => {
    if (!focus) return;
    if (focus.tab === "run") { setSeen((x) => ({ ...x, run: true })); setActive(runId); }
    else if (focus.tab === "setup") { setSeen((x) => ({ ...x, setup: true })); setActive(setupId); }
    else setActive(focus.tab);
  }, [focus?.nonce]);

  const addShell = () => {
    const id = `${wsId}:term-${Date.now().toString(36)}`;
    setShells((s) => [...s, id]);
    setActive(id);
  };
  const closeShell = (id: string) => {
    api.ptyClose(id);
    setShells((s) => {
      const next = s.filter((x) => x !== id);
      const left = next.length ? next : [`${wsId}:term-${Date.now().toString(36)}`];
      if (active === id) setActive(left[left.length - 1]);
      return left;
    });
  };

  // ⌘T opens another shell while the terminal is showing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (root.current?.closest(".pane")?.hasAttribute("hidden")) return;
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === "t") { e.preventDefault(); addShell(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [wsId]);

  const tabs: Tab[] = [
    ...(seen.setup ? [{ id: setupId, label: "Setup", kind: "setup" as const }] : []),
    ...(seen.run ? [{ id: runId, label: "Run", kind: "run" as const }] : []),
    ...shells.map((id, i) => ({ id, label: shells.length > 1 ? `Terminal ${i + 1}` : "Terminal", kind: "shell" as const })),
  ];
  const cur = tabs.find((t) => t.id === active) ?? tabs[tabs.length - 1];

  return (
    <div className="term-panel" ref={root}>
      <div className="chat-tabs term-tabs" role="tablist" aria-label="Terminals">
        {tabs.map((t) => (
          <span key={t.id} className={`chat-tab ${t.id === cur.id ? "active" : ""}`}>
            <button role="tab" aria-selected={t.id === cur.id} onClick={() => setActive(t.id)}>
              {t.kind === "run" ? (running.has(t.id) ? <span className="live-dot" aria-label="running" /> : <Play size={12} />) : t.kind === "setup" ? (running.has(t.id) ? <span className="spinner" /> : <Wrench size={12} />) : <SquareTerminal size={12} />}
              {t.label}
            </button>
            {t.kind === "shell" && (
              <button className="tab-close" aria-label={`Close ${t.label}`} onClick={() => closeShell(t.id)}><X size={12} /></button>
            )}
          </span>
        ))}
        <span className="newchat"><button className="icon" title="New terminal (⌘T)" aria-label="New terminal" onClick={addShell}><Plus size={15} /></button></span>
        <span className="spacer" />
        {cur.kind === "run" && (
          <button className={running.has(runId) ? "danger term-action" : "term-action"} onClick={onRun} title="⌘R">
            {running.has(runId) ? <><Square size={12} />Stop</> : <><Play size={12} />Run</>}
          </button>
        )}
        {cur.kind === "setup" && (
          <button className="term-action" onClick={onSetup} disabled={running.has(setupId)}><RotateCw size={12} />Run setup again</button>
        )}
      </div>
      {tabs.map((t) => (
        <div key={t.id} className="term-body" hidden={t.id !== cur.id}>
          <Terminal ptyId={t.id} cwd={cwd} mode={t.kind === "shell" ? "shell" : "attach"} nonce={t.kind === "shell" ? 0 : running.has(t.id) ? 1 : 0} />
        </div>
      ))}
    </div>
  );
}
