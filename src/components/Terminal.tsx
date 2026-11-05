import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { api } from "../api";

/** A login shell in the workspace folder. `ptyId` keeps the shell alive across tab switches. */
export function Terminal({ ptyId, cwd, run }: { ptyId: string; cwd: string; run?: { cmd: string; nonce: number } }) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<XTerm | null>(null);

  useEffect(() => {
    const t = new XTerm({
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      fontSize: 13,
      cursorBlink: true,
      theme: { background: "#0e1014", foreground: "#d8dee9" },
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(host.current!);
    fit.fit();
    term.current = t;

    const onData = t.onData((d) => api.ptyWrite(ptyId, d));
    const un = listen<{ id: string; data: string }>("pty", ({ payload }) => {
      if (payload.id === ptyId) t.write(payload.data);
    });
    const unExit = listen<{ id: string }>("pty-exit", ({ payload }) => {
      if (payload.id === ptyId) t.write("\r\n[shell exited, select the workspace again to start a new one]\r\n");
    });
    api.ptyOpen(ptyId, cwd, t.cols, t.rows);

    const ro = new ResizeObserver(() => {
      if (!host.current?.offsetWidth) return; // hidden tab: keep the last size
      fit.fit();
      api.ptyResize(ptyId, t.cols, t.rows);
    });
    ro.observe(host.current!);
    return () => {
      ro.disconnect();
      onData.dispose();
      un.then((f) => f());
      unExit.then((f) => f());
      t.dispose();
    };
  }, [ptyId, cwd]);

  useEffect(() => {
    if (run?.cmd) api.ptyWrite(ptyId, run.cmd + "\n");
  }, [run?.nonce]);

  return <div className="terminal" ref={host} />;
}
