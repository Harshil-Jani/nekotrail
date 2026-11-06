import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { Play, Plus, RotateCw, Square, SquareTerminal, Wrench, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";
import { api } from "../api";
import { fontStack, TERM_SIZE, TERM_STACK } from "../appearance";
import { getSettings } from "../settings";
import { IS_MAC, keyHint, mod } from "../platform";

/**
 * One terminal view. `shell` starts a login shell (or reattaches to the running one);
 * `attach` only shows a process started elsewhere (run and setup scripts).
 * Output printed while the view was closed is replayed from the backend's scrollback.
 */
export function Terminal({ ptyId, cwd, mode = "shell", nonce = 0 }: { ptyId: string; cwd: string; mode?: "shell" | "attach"; nonce?: number }) {
  const host = useRef<HTMLDivElement>(null);
  const [idle, setIdle] = useState(false);

  useEffect(() => {
    const look = getSettings();
    const t = new XTerm({
      fontFamily: fontStack(look.termFont, TERM_STACK),
      fontSize: look.termSize || TERM_SIZE,
      cursorBlink: mode === "shell",
      scrollback: 5000,
      theme: { background: "#1e1e1e", foreground: "#d8dee9", cursor: "#ff9a4d", selectionBackground: "#ff9a4d55" },
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(host.current!);
    fit.fit();
    // Linux: Ctrl+letter belongs to the shell, so the app's Ctrl+1..4 tab keys are let through, and
    // copy and paste are Ctrl+Shift+C / Ctrl+Shift+V like in GNOME Terminal. (On a Mac ⌘ never
    // reaches the shell, so nothing changes there.)
    if (!IS_MAC) {
      t.attachCustomKeyEventHandler((e) => {
        if (e.type !== "keydown" || !e.ctrlKey || e.altKey || e.metaKey) return true;
        if (!e.shiftKey && /^[1-4]$/.test(e.key)) return false;
        if (e.shiftKey && e.code === "KeyC") {
          const sel = t.getSelection();
          if (sel) navigator.clipboard.writeText(sel).catch(() => {});
          e.preventDefault();
          return false;
        }
        if (e.shiftKey && e.code === "KeyV") {
          e.preventDefault();
          navigator.clipboard.readText().then((text) => { if (text) t.paste(text); }).catch(() => {});
          return false;
        }
        return true;
      });
    }
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

    // Settings, Appearance: a new terminal font or size applies right away, then the grid refits.
    const onLook = () => {
      const { termFont, termSize } = getSettings();
      const family = fontStack(termFont, TERM_STACK);
      const size = termSize || TERM_SIZE;
      if (t.options.fontFamily === family && t.options.fontSize === size) return;
      t.options.fontFamily = family;
      t.options.fontSize = size;
      if (!host.current?.offsetWidth) return; // hidden tab: the ResizeObserver refits when it shows
      fit.fit();
      api.ptyResize(ptyId, t.cols, t.rows).catch(() => {});
    };
    window.addEventListener("nekotrail-settings", onLook);
    return () => {
      live = false;
      window.removeEventListener("nekotrail-settings", onLook);
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

  // "Run in terminal" from anywhere (sign in, install): show the first shell and type it there.
  useEffect(() => {
    const onRun = (e: Event) => {
      const cmd = (e as CustomEvent<string>).detail;
      const shell = shells[0];
      setActive(shell);
      setTimeout(() => api.ptyWrite(shell, cmd + "\n").catch(() => {}), 400);
    };
    window.addEventListener("nekotrail-terminal", onRun);
    return () => window.removeEventListener("nekotrail-terminal", onRun);
  }, [shells]);

  // ⌘T opens another shell while the terminal is showing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (root.current?.closest(".pane")?.hasAttribute("hidden")) return;
      // Linux: Ctrl+T inside the terminal is the shell's, so Ctrl+Shift+T (like GNOME Terminal) works too.
      if (mod(e) && (IS_MAC ? !e.shiftKey : true) && e.key.toLowerCase() === "t") { e.preventDefault(); addShell(); }
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
        <span className="newchat"><button className="icon" title={keyHint("Open a new terminal (⌘T)")} aria-label="Open a new terminal" onClick={addShell}><Plus size={15} /></button></span>
        <span className="spacer" />
        {cur.kind === "run" && (
          <button className={running.has(runId) ? "danger term-action" : "term-action"} onClick={onRun} title={keyHint("Run or stop (⌘R)")}>
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
