import { useEffect, useState } from "react";
import { Minus, Plus, Volume2, X } from "lucide-react";
import { availableMonoFonts, CODE_SIZE, CODE_STACK, fontStack, setZoom, stepZoom, TERM_SIZE, TERM_STACK, useZoom, ZOOM_LEVELS, ZOOM_MAX, ZOOM_MIN } from "../appearance";
import { api, type AppSettings } from "../api";
import { AGENTS, AGENT_INFO } from "../agents";
import { useInstalledAgents } from "../useInstalledAgents";
import { okFullAuto } from "../fullAuto";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import pkg from "../../package.json";
import type { UpdatePhase } from "../useUpdater";
import { chime, getSettings, saveSettings, ting, type Settings as S } from "../settings";

const SHORTCUTS: [string, string][] = [
  ["⌘K", "Jump anywhere, run any command"],
  ["⌘N / ⌘⇧N", "Create workspace / from a PR"],
  ["⌘1 / ⌘2 / ⌘3", "Show chat / changes / terminal"],
  ["⌃1 to ⌃9", "Go to a workspace in the sidebar"],
  ["⌘⌥↑ / ⌘⌥↓", "Previous / next workspace"],
  ["⌘⌥L", "Go to next workspace with news"],
  ["⌘L / ⌘⇧D / ⌘J", "Also: type in chat / changes / toggle terminal"],
  ["⌘T", "Start a chat or open a terminal tab"],
  ["⌘F / ⌘G", "Search this chat / next match"],
  ["↵ or ⌘↵", "Send message (choose which above)"],
  ["⌘⇧↵", "Approve the agent's plan"],
  ["⌘⇧⌫", "Stop the agent"],
  ["⌃O", "Show or hide every tool call's output"],
  ["⌘⇧P / ⌘⇧Y / ⌘⇧M", "Create PR / commit and push / merge"],
  ["⌘⇧L / ⌘⇧R / ⌘⇧X", "Pull latest target branch / review / fix failing checks"],
  ["⌘⇧G", "Open the PR on GitHub"],
  ["⌘P", "Open any file"],
  ["⌘⌥U", "Switch changes: all or uncommitted"],
  ["⌘R / ⌘⇧B", "Run or stop the app / open it in browser"],
  ["⌘O", "Open workspace in your editor"],
  ["⌘⇧A", "Archive workspace"],
  ["⌘B", "Hide or show the sidebar"],
  ["⌘+ / ⌘- / ⌘0", "Zoom in / out / reset"],
  ["⌘,", "Open settings"],
  ["@ and /", "Mention a file, run a command"],
];

type Pane = "general" | "appearance" | "notifications" | "agents" | "shortcuts";
const PANES: [Pane, string][] = [["general", "General"], ["appearance", "Appearance"], ["notifications", "Notifications"], ["agents", "Agents"], ["shortcuts", "Shortcuts"]];

const SIZES = [10, 11, 12, 13, 14, 15, 16, 18];
const CODE_SAMPLE = "const purr = (cat) => cat.nap ?? 0; // {}[]|0O1lI";
const TERM_SAMPLE = "~/nekotrail $ git status  // 0O 1lI";

/** A font pop-up (SF Mono plus the monospace fonts this Mac has) with a size pop-up next to it. */
function FontPicker({ label, fonts, font, size, defaultSize, onFont, onSize }: {
  label: string; fonts: string[]; font: string; size: number; defaultSize: number;
  onFont: (f: string) => void; onSize: (n: number) => void;
}) {
  // A font chosen earlier stays in the list even if it was uninstalled since, so the pop-up shows the truth.
  const list = font && !fonts.includes(font) ? [font, ...fonts] : fonts;
  return (
    <span className="font-control">
      <select value={font} onChange={(e) => onFont(e.target.value)} aria-label={`${label} font`}>
        <option value="">SF Mono (default)</option>
        {list.map((f) => <option key={f} value={f}>{f}</option>)}
      </select>
      <select value={size} onChange={(e) => onSize(Number(e.target.value))} aria-label={`${label} size`}>
        {SIZES.map((n) => <option key={n} value={n}>{n} pt{n === defaultSize ? " (default)" : ""}</option>)}
      </select>
    </span>
  );
}

/** Appearance pane: light or dark, zoom, and the code and terminal fonts. Font detection runs only here. */
function AppearancePane({ s, set }: { s: S; set: <K extends keyof S>(k: K, v: S[K]) => void }) {
  const zoom = useZoom();
  const [fonts] = useState(availableMonoFonts);
  const pct = (z: number) => `${Math.round(z * 100)}%`;
  return <>
    <section>
      <h3>Look</h3>
      <div className="setting">
        <span id="theme-label">Appearance</span>
        <div className="seg" role="radiogroup" aria-labelledby="theme-label">
          {([["system", "Follow system"], ["light", "Light"], ["dark", "Dark"]] as const).map(([id, name]) => (
            <button key={id} role="radio" aria-checked={s.theme === id} className={s.theme === id ? "on" : ""} onClick={() => set("theme", id)}>{name}</button>
          ))}
        </div>
      </div>
      <div className="setting">
        <span id="zoom-label">Zoom</span>
        <span className="zoom-control">
          <button className="icon" aria-label="Zoom out" title="Zoom out (⌘-)" disabled={zoom <= ZOOM_MIN} onClick={() => setZoom(stepZoom(zoom, -1))}><Minus size={14} /></button>
          <select aria-labelledby="zoom-label" value={String(zoom)} onChange={(e) => setZoom(Number(e.target.value))}>
            {!ZOOM_LEVELS.includes(zoom) && <option value={String(zoom)}>{pct(zoom)}</option>}
            {ZOOM_LEVELS.map((z) => <option key={z} value={String(z)}>{pct(z)}{z === 1 ? " (actual size)" : ""}</option>)}
          </select>
          <button className="icon" aria-label="Zoom in" title="Zoom in (⌘+)" disabled={zoom >= ZOOM_MAX} onClick={() => setZoom(stepZoom(zoom, 1))}><Plus size={14} /></button>
        </span>
      </div>
      <p className="muted setting-note">⌘+ and ⌘- zoom anywhere in the app, ⌘0 goes back to 100%.</p>
    </section>

    <section>
      <h3>Code</h3>
      <p className="muted setting-note">Diffs, code blocks in chat and tool output.</p>
      <div className="setting">
        <span>Font</span>
        <FontPicker label="Code" fonts={fonts} font={s.codeFont} size={s.codeSize} defaultSize={CODE_SIZE} onFont={(f) => set("codeFont", f)} onSize={(n) => set("codeSize", n)} />
      </div>
      <div className="font-sample" style={{ fontFamily: fontStack(s.codeFont, CODE_STACK), fontSize: s.codeSize }} aria-hidden="true">{CODE_SAMPLE}</div>
    </section>

    <section>
      <h3>Terminal</h3>
      <div className="setting">
        <span>Font</span>
        <FontPicker label="Terminal" fonts={fonts} font={s.termFont} size={s.termSize} defaultSize={TERM_SIZE} onFont={(f) => set("termFont", f)} onSize={(n) => set("termSize", n)} />
      </div>
      <div className="font-sample term" style={{ fontFamily: fontStack(s.termFont, TERM_STACK), fontSize: s.termSize }} aria-hidden="true">{TERM_SAMPLE}</div>
    </section>
  </>;
}

/** ⌘, : preferences in five panes, like a Mac settings window. Changes save as you make them. */
export function Settings({ onClose, home, update, onCheckUpdates }: { onClose: () => void; home: string; update?: UpdatePhase | null; onCheckUpdates?: () => void }) {
  // ?settings=agents opens a pane directly (the preview page uses it for screenshots).
  const [pane, setPane] = useState<Pane>(() => {
    const want = new URLSearchParams(location.search).get("settings");
    return PANES.some(([id]) => id === want) ? (want as Pane) : "general";
  });
  const [s, setS] = useState<S>(getSettings);
  const [backend, setBackend] = useState<AppSettings | null>(null);
  // New-chat defaults: the first four agents, every installed one, and any with a saved default.
  const installed = useInstalledAgents();
  const defaultsFor = AGENTS.filter((a, i) => i < 4 || installed?.includes(a) || [`${a}_model`, `${a}_effort`, `${a}_mode`].some((k) => backend?.[k as keyof AppSettings]));
  const [targets, setTargets] = useState<{ id: string; name: string }[]>([]);
  const [version, setVersion] = useState(pkg.version);
  useEffect(() => { getVersion().then((v) => v && setVersion(v)).catch(() => {}); }, []);
  useEffect(() => {
    api.appSettings().then(setBackend).catch(() => {});
    api.openInTargets().then(setTargets).catch(() => {});
  }, []);
  const setRaw = (key: Exclude<keyof AppSettings, "home">, v: string) => {
    setBackend((b) => (b ? { ...b, [key]: v } : b));
    api.setAppSetting(key, v);
  };
  const setB = (key: "caffeinate" | "auto_branch", on: boolean) => setRaw(key, on ? "on" : "off");
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const set = <K extends keyof S>(k: K, v: S[K]) => {
    const next = { ...s, [k]: v };
    setS(next);
    saveSettings(next);
  };
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal settings" role="dialog" aria-modal="true" aria-label="Settings" onClick={(e) => e.stopPropagation()}>
        <div className="row settings-head">
          <h2>Settings</h2>
          <span className="spacer" />
          <button className="icon" aria-label="Close settings" title="Close (Esc)" onClick={onClose}><X size={16} /></button>
        </div>
        <nav className="seg settings-panes" role="tablist" aria-label="Settings">
          {PANES.map(([id, label]) => (
            <button key={id} role="tab" aria-selected={pane === id} className={pane === id ? "on" : ""} onClick={() => setPane(id)}>{label}</button>
          ))}
        </nav>

        <div className="settings-body" role="tabpanel" aria-label={PANES.find(([id]) => id === pane)?.[1]}>
        {pane === "appearance" && <AppearancePane s={s} set={set} />}

        {pane === "notifications" && <>
        <section>
          <h3>When an agent needs you</h3>
          <p className="muted setting-note">A plan to approve, a question, a sign-in, a usage limit or an error.</p>
          <label className="setting">
            <span>Play a sound</span>
            <span className="row">
              <button className="icon" title="Play the sound" aria-label="Play the needs-you sound" onClick={() => ting()}><Volume2 size={16} /></button>
              <input type="checkbox" className="switch" checked={s.needsSound} onChange={(e) => { set("needsSound", e.target.checked); if (e.target.checked) ting(); }} />
            </span>
          </label>
          <label className="setting">
            <span>Notification</span>
            <input type="checkbox" className="switch" checked={s.needsNotify} onChange={(e) => set("needsNotify", e.target.checked)} />
          </label>
          <label className="setting">
            <span>Bounce the Dock icon</span>
            <input type="checkbox" className="switch" checked={s.needsBounce} onChange={(e) => set("needsBounce", e.target.checked)} />
          </label>
        </section>

        <section>
          <h3>When an agent finishes</h3>
          <label className="setting">
            <span>Notification</span>
            <select value={s.notify} onChange={(e) => set("notify", e.target.value as S["notify"])}>
              <option value="away">Only when I'm looking elsewhere</option>
              <option value="always">Always</option>
              <option value="never">Never</option>
            </select>
          </label>
          <label className="setting">
            <span>Play a sound</span>
            <span className="row">
              <button className="icon" title="Play the chime" aria-label="Play the chime" onClick={() => chime(true)}><Volume2 size={16} /></button>
              <input type="checkbox" className="switch" checked={s.sound} onChange={(e) => { set("sound", e.target.checked); if (e.target.checked) chime(true); }} />
            </span>
          </label>
        </section>
        </>}

        {pane === "general" && <>
        <section>
          <h3>Chat</h3>
          <label className="setting">
            <span>Send with</span>
            <select value={s.sendKey} onChange={(e) => set("sendKey", e.target.value as S["sendKey"])}>
              <option value="enter">↵ sends, ⇧↵ new line</option>
              <option value="cmd-enter">⌘↵ sends, ↵ new line</option>
            </select>
          </label>
          <label className="setting">
            <span>Keep the Mac awake while agents work</span>
            <input type="checkbox" className="switch" checked={backend?.caffeinate !== "off"} onChange={(e) => setB("caffeinate", e.target.checked)} />
          </label>
        </section>

        <section>
          <h3>Workspaces</h3>
          <label className="setting">
            <span>Name the branch after the first message</span>
            <input type="checkbox" className="switch" checked={backend?.auto_branch !== "off"} onChange={(e) => setB("auto_branch", e.target.checked)} />
          </label>
          <label className="setting">
            <span>Archive when the PR merges</span>
            <input type="checkbox" className="switch" checked={s.archiveOnMerge} onChange={(e) => set("archiveOnMerge", e.target.checked)} />
          </label>
          <label className="setting">
            <span>⌘O opens in</span>
            <select value={s.editor} onChange={(e) => set("editor", e.target.value)}>
              {targets.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </select>
          </label>
          <div className="setting">
            <span>Workspaces folder</span>
            <code className="muted">{backend?.home ? `${backend.home}/workspaces` : home}</code>
          </div>
        </section>
        </>}

        {pane === "agents" && <>
        <section>
          <h3>New chats start with</h3>
          {defaultsFor.map((a) => (
            <div className="setting" key={a}>
              <span>{AGENT_INFO[a].name}</span>
              <span className="row">
                <input
                  className="setting-model"
                  placeholder="Default model"
                  defaultValue={backend?.[`${a}_model`] ?? ""}
                  key={`${a}-${backend ? "loaded" : "loading"}`}
                  onBlur={(e) => setRaw(`${a}_model`, e.target.value.trim())}
                  aria-label={`${a} default model`}
                />
                {AGENT_INFO[a].effort && <select value={backend?.[`${a}_effort`] ?? ""} onChange={(e) => setRaw(`${a}_effort`, e.target.value)} aria-label={`${a} default effort`}>
                  <option value="">Default effort</option>
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                  <option value="xhigh">Extra high</option>
                  {a === "claude" && <option value="max">Max</option>}
                </select>}
                <select value={backend?.[`${a}_mode`] || "acceptEdits"} onChange={async (e) => { const m = e.target.value; if (await okFullAuto(m)) setRaw(`${a}_mode`, m); }} aria-label={`${a} default permissions`}>
                  <option value="plan">Plan</option>
                  <option value="acceptEdits">Auto-accept edits</option>
                  <option value="bypassPermissions">Full auto</option>
                </select>
              </span>
            </div>
          ))}
        </section>

        <section>
          <h3>Snippets</h3>
          <p className="muted setting-note">Saved prompts. Type /name in the chat box to insert one, then edit and send.</p>
          {s.snippets.map((sn, i) => (
            <div className="snippet-row" key={i}>
              <input value={sn.name} aria-label="Snippet name" placeholder="name"
                onChange={(e) => set("snippets", s.snippets.map((x, j) => (j === i ? { ...x, name: e.target.value.replace(/[^\w-]/g, "") } : x)))} />
              <textarea rows={2} value={sn.text} aria-label="Snippet text" placeholder="What the agent should do"
                onChange={(e) => set("snippets", s.snippets.map((x, j) => (j === i ? { ...x, text: e.target.value } : x)))} />
              <button className="icon" aria-label={`Delete snippet ${sn.name}`} onClick={() => set("snippets", s.snippets.filter((_, j) => j !== i))}><X size={14} /></button>
            </div>
          ))}
          <button className="quiet add-env" onClick={() => set("snippets", [...s.snippets, { name: "", text: "" }])}><Plus size={14} />Add snippet</button>
        </section>
        </>}

        {pane === "shortcuts" && (
        <section>
          <h3>Keyboard</h3>
          <dl className="shortcuts">
            {SHORTCUTS.map(([k, v]) => (
              <div key={k}><dt><kbd>{k}</kbd></dt><dd>{v}</dd></div>
            ))}
          </dl>
        </section>
        )}
        </div>

        <div className="row end">
          <span className="muted version num">nekotrail v{version}</span>
          <button className="link" onClick={() => openUrl("https://github.com/Harshil-Jani/nekotrail/blob/main/CHANGELOG.md").catch(() => {})}>What's new</button>
          <button className="link" onClick={() => openUrl("https://github.com/sponsors/Harshil-Jani").catch(() => {})} title="nekotrail is free and open source. Sponsoring keeps it that way.">Sponsor</button>
          {update?.kind === "checking" ? (
            <span className="muted update-status" role="status">Checking for updates…</span>
          ) : update?.kind === "latest" ? (
            <span className="muted update-status" role="status">You're on the latest</span>
          ) : update?.kind === "dev" ? (
            <span className="muted update-status" role="status">Update checks run in release builds</span>
          ) : (
            <button className="link" onClick={onCheckUpdates}>Check for updates</button>
          )}
          <span className="spacer" />
          <button className="primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}
