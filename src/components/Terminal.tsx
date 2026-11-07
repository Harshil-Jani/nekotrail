import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Play, Plus, RotateCw, Square, SquareTerminal, Wrench, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";
import { api, errText } from "../api";
import { afterScrollback, startFailedText } from "../pty";
import { ptyBus } from "../ptyBus";

const SLOW_START_MS = 8000; // a shell that hasn't answered by now gets a Try again
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
  // "slow": the shell hasn't answered after a while; "exited" and "failed" offer a fresh shell.
  const [state, setState] = useState<"starting" | "live" | "slow" | "exited" | "failed">("starting");
  const [restart, setRestart] = useState(0);

  useEffect(() => {
    const look = getSettings();
    const t = new XTerm({
      fontFamily: fontStack(look.termFont, TERM_STACK),
      fontSize: look.termSize || TERM_SIZE,
      cursorBlink: mode === "shell",
      scrollback: 5000,
      theme: {
        background: "#0b0b1e", foreground: "#e6e6f5", cursor: "#ccccff", cursorAccent: "#0b0b1e", selectionBackground: "#ccccff40",
        // Moonlight ANSI colours: each passes 4.5:1 on the terminal background (xterm's own red and blue don't).
        black: "#2a2a52", red: "#ff7a8e", green: "#5ee0a0", yellow: "#ffd27a", blue: "#9c9cff", magenta: "#e0a6ff", cyan: "#7adce8", white: "#d8d8ee",
        brightBlack: "#8a8ab8", brightRed: "#ff9eae", brightGreen: "#8aeebc", brightYellow: "#ffe0a3", brightBlue: "#c0c0ff", brightMagenta: "#ecc6ff", brightCyan: "#a6eaf2", brightWhite: "#f5f5ff",
      },
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
    setState("starting");

    const onData = t.onData((d) => { api.ptyWrite(ptyId, d).catch(() => {}); });
    // Output that arrives before the scrollback does waits here, so none is lost or shown twice.
    let early: string[] | null = [];
    let exitedEarly = false;
    let off: (() => void) | null = null;
    const ended = () => {
      t.write(mode === "shell" ? "\r\n\x1b[2m[shell exited]\x1b[0m\r\n" : "\r\n\x1b[2m[finished]\x1b[0m\r\n");
      if (mode === "shell") setState("exited");
    };
    const slow = setTimeout(() => { if (live) setState((s) => (s === "starting" ? "slow" : s)); }, SLOW_START_MS);
    (async () => {
      // Listen first, then start: a new shell's first prompt can come before `pty_open` returns.
      const unsub = await ptyBus.subscribe(ptyId, (data) => {
        if (early) { early.push(data); return; }
        setIdle(false);
        t.write(data);
      }, () => {
        if (!live) return;
        if (early) { exitedEarly = true; return; } // said after the scrollback, below
        ended();
      });
      if (!live) { unsub(); return; }
      off = unsub;
      // This shell ended earlier (seen even with the tab closed): clear it, so a fresh one starts.
      if (mode === "shell" && ptyBus.takeEnded(ptyId)) await api.ptyClose(ptyId).catch(() => {});
      const sb = await (mode === "shell" ? api.ptyOpen(ptyId, cwd, t.cols, t.rows) : api.ptyAttach(ptyId));
      if (!live) return;
      clearTimeout(slow);
      if (sb) t.write(sb);
      for (const d of afterScrollback(sb ?? "", early ?? [])) t.write(d);
      early = null;
      if (mode === "attach" && sb === null) setIdle(true);
      setState("live");
      if (exitedEarly) ended();
      api.ptyResize(ptyId, t.cols, t.rows).catch(() => {});
    })().catch((err) => {
      if (!live) return;
      clearTimeout(slow);
      early = null;
      // Say why there is no shell (a folder that is gone, say) instead of a blank terminal that ignores typing.
      t.write(startFailedText(errText(err)));
      setState("failed");
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
      clearTimeout(slow);
      window.removeEventListener("nekotrail-settings", onLook);
      ro.disconnect();
      onData.dispose();
      off?.();
      t.dispose();
    };
  }, [ptyId, cwd, mode, nonce, restart]);

  // Start a fresh shell in this tab: close what is left of the old one, then build the view again.
  const startAgain = () => {
    ptyBus.takeEnded(ptyId);
    api.ptyClose(ptyId).catch(() => {}).finally(() => setRestart((n) => n + 1));
  };
  const stuck = mode === "shell" && (state === "exited" || state === "failed" || state === "slow");

  return (
    <div className="terminal-wrap">
      {idle && <p className="terminal-idle muted">Not running. Start it with the button above.</p>}
      {stuck && (
        <div className="terminal-state" role="status">
          <span>{state === "exited" ? "The shell exited." : state === "slow" ? "The shell is taking a while to start." : "The shell didn't start."}</span>
          <button className="quiet" onClick={startAgain}><RotateCw size={12} />{state === "slow" ? "Try again" : "Start a new shell"}</button>
        </div>
      )}
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
