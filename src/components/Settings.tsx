import { useEffect, useState } from "react";
import { Coffee, Heart, Minus, Plus, Volume2, X } from "lucide-react";
import { availableMonoFonts, CODE_SIZE, CODE_STACK, fontStack, setZoom, stepZoom, TERM_SIZE, TERM_STACK, useZoom, ZOOM_LEVELS, ZOOM_MAX, ZOOM_MIN } from "../appearance";
import { api, type AppSettings } from "../api";
import { IS_MAC, keyHint } from "../platform";
import { AGENTS, AGENT_INFO } from "../agents";
import { useInstalledAgents } from "../useInstalledAgents";
import { okFullAuto } from "../fullAuto";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import pkg from "../../package.json";
import type { UpdatePhase } from "../useUpdater";
import { PromptsPane } from "./PromptsPane";
import { AwayAlerts } from "./AwayAlerts";
import { LinearSettings } from "./LinearSettings";
import { chime, getSettings, saveSettings, ting, type Settings as S } from "../settings";

const SHORTCUTS: [string, string][] = [
  [keyHint("⌘K"), "Jump anywhere, run any command"],
  [keyHint("⌘N / ⌘⇧N"), "Create workspace / from a PR"],
  [keyHint("⌘1 / ⌘2 / ⌘3"), "Show chat / changes / terminal"],
  [keyHint("⌃1 to ⌃9"), "Go to a workspace in the sidebar"],
  [keyHint("⌘⌥↑ / ⌘⌥↓"), "Previous / next workspace"],
  [keyHint("⌘⌥L"), "Go to next workspace with news"],
  [keyHint("⌘L / ⌘⇧D / ⌘J"), "Also: type in chat / changes / toggle terminal"],
  [keyHint("⌘T"), "Start a chat or open a terminal tab"],
  [keyHint("⌘⇧F"), "Search every chat, archived ones too"],
  [keyHint("⌘F / ⌘G"), "Find in this chat / next match"],
  [keyHint("↵ or ⌘↵"), "Send message (choose which above)"],
  [keyHint("⌘⇧↵"), "Approve the agent's plan"],
  [keyHint("⌘⇧⌫"), "Stop the agent"],
  [keyHint("⌃O"), "Show or hide every tool call's output"],
  [keyHint("⌘⇧P / ⌘⇧Y / ⌘⇧M"), "Create PR / commit and push / merge"],
  [keyHint("⌘⇧L / ⌘⇧R / ⌘⇧X"), "Pull latest target branch / review / fix failing checks"],
  [keyHint("⌘⇧G"), "Open the PR on GitHub"],
  [keyHint("⌘P"), "Open any file"],
  [keyHint("⌘⌥U"), "Switch changes: all or uncommitted"],
  [keyHint("⌘R / ⌘⇧B"), "Run or stop the app / open it in browser"],
  [keyHint("⌘O"), "Open workspace in your editor"],
  [keyHint("⌘⇧A"), "Archive workspace"],
  [keyHint("⌘B"), "Hide or show the sidebar"],
  [keyHint("⌘+ / ⌘- / ⌘0"), "Zoom in / out / reset"],
  [keyHint("⌘,"), "Open settings"],
  ["@ and /", "Mention a file, run a command or insert a prompt"],
  ["Tab / ⇧Tab", "Next / previous blank in a prompt"],
];

export type Pane = "general" | "appearance" | "notifications" | "agents" | "integrations" | "prompts" | "shortcuts";
const PANES: [Pane, string][] = [["general", "General"], ["appearance", "Appearance"], ["notifications", "Notifications"], ["agents", "Agents"], ["integrations", "Integrations"], ["prompts", "Prompts"], ["shortcuts", "Shortcuts"]];

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
        <option value="">{IS_MAC ? "SF Mono (default)" : "System monospace (default)"}</option>
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
          <button className="icon" aria-label="Zoom out" title={keyHint("Zoom out (⌘-)")} disabled={zoom <= ZOOM_MIN} onClick={() => setZoom(stepZoom(zoom, -1))}><Minus size={14} /></button>
          <select aria-labelledby="zoom-label" value={String(zoom)} onChange={(e) => setZoom(Number(e.target.value))}>
            {!ZOOM_LEVELS.includes(zoom) && <option value={String(zoom)}>{pct(zoom)}</option>}
            {ZOOM_LEVELS.map((z) => <option key={z} value={String(z)}>{pct(z)}{z === 1 ? " (actual size)" : ""}</option>)}
          </select>
          <button className="icon" aria-label="Zoom in" title={keyHint("Zoom in (⌘+)")} disabled={zoom >= ZOOM_MAX} onClick={() => setZoom(stepZoom(zoom, 1))}><Plus size={14} /></button>
        </span>
      </div>
      <p className="muted setting-note">{keyHint("⌘+ and ⌘- zoom anywhere in the app, ⌘0 goes back to 100%.")}</p>
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
/** Settings, General: install the `nekotrail` terminal command. */
function CommandLine() {
  const [path, setPath] = useState<string | null>(null);
  const [note, setNote] = useState<{ text: string; error?: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.cliStatus().then((s) => setPath(s.path)).catch(() => {}); }, []);
  const install = async () => {
    setBusy(true);
    try {
      const r = await api.cliInstall();
      setPath(r.path);
      setNote(r.hint ? { text: r.hint } : { text: "Installed. Try nekotrail --help in a new terminal." });
    } catch (e) {
      setNote({ text: String(e), error: true });
    } finally {
      setBusy(false);
    }
  };
  return (
    <section>
      <h3>Command line</h3>
      <p className="muted setting-note">Run <code>nekotrail .</code> to open a repo, or <code>nekotrail new "fix the login bug"</code> to start an agent, from any terminal.</p>
      <div className="setting">
        <span>{path ? <>Installed at <code>{path}</code></> : "Terminal command"}</span>
        <button onClick={install} disabled={busy}>{path ? "Reinstall" : "Install command line tool"}</button>
      </div>
      {note && <p className={`setting-note ${note.error ? "bad" : "muted"}`} role="status">{note.text}</p>}
    </section>
  );
}

export function Settings({ onClose, home, update, onCheckUpdates, workspaceId, repoName, initialPane }: {
  onClose: () => void; home: string; update?: UpdatePhase | null; onCheckUpdates?: () => void;
  /** The workspace in view, so the Prompts pane lists its repo's prompts too. */
  workspaceId?: string; repoName?: string; initialPane?: Pane;
}) {
  // ?settings=agents opens a pane directly (the preview page uses it for screenshots).
  const [pane, setPane] = useState<Pane>(() => {
    const want = new URLSearchParams(location.search).get("settings") ?? initialPane;
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
            <span>{IS_MAC ? "Bounce the Dock icon" : "Flag the window in the taskbar"}</span>
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
        <AwayAlerts />
        </>}

        {pane === "general" && <>
        <section>
          <h3>Chat</h3>
          <label className="setting">
            <span>Send with</span>
            <select value={s.sendKey} onChange={(e) => set("sendKey", e.target.value as S["sendKey"])}>
              <option value="enter">{keyHint("↵ sends, ⇧↵ new line")}</option>
              <option value="cmd-enter">{keyHint("⌘↵ sends, ↵ new line")}</option>
            </select>
          </label>
          <label className="setting">
            <span>{IS_MAC ? "Keep the Mac awake while agents work" : "Keep the computer awake while agents work"}</span>
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
            <span>{keyHint("⌘O opens in")}</span>
            <select value={s.editor} onChange={(e) => set("editor", e.target.value)}>
              {targets.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </select>
          </label>
          <div className="setting">
            <span>Workspaces folder</span>
            <code className="muted">{backend?.home ? `${backend.home}/workspaces` : home}</code>
          </div>
        </section>
        <section>
          <h3>Exports</h3>
          <label className="setting">
            <span>Add “Made with NekoTrail” to exported sessions</span>
            <input type="checkbox" className="switch" checked={s.exportFooter} onChange={(e) => set("exportFooter", e.target.checked)} />
          </label>
        </section>
        <CommandLine />
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
          {defaultsFor.length < AGENTS.length && <p className="muted setting-note">Other agents show up here once they're installed.</p>}
        </section>

        </>}

        {pane === "integrations" && <LinearSettings />}

        {pane === "prompts" && <PromptsPane workspaceId={workspaceId} repoName={repoName} />}

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

        <div className="row end settings-foot">
          <span className="muted version num">NekoTrail v{version}</span>
          <span className="foot-dot" aria-hidden>·</span>
          <button className="link" onClick={() => openUrl("https://github.com/Harshil-Jani/nekotrail/blob/main/CHANGELOG.md").catch(() => {})}>What's new</button>
          <span className="foot-dot" aria-hidden>·</span>
          {update?.kind === "checking" ? (
            <span className="muted update-status" role="status">Checking for updates…</span>
          ) : update?.kind === "latest" ? (
            <span className="muted update-status" role="status">You're on the latest</span>
          ) : update?.kind === "dev" ? (
            <span className="muted update-status" role="status">Update checks run in release builds</span>
          ) : (
            <button className="link" onClick={onCheckUpdates}>Check for updates</button>
          )}
          {/* Sponsor links are a different kind of link from the version ones: set apart, each with its icon. */}
          <span className="foot-sep" aria-hidden />
          <button className="link sponsor" onClick={() => openUrl("https://github.com/sponsors/Harshil-Jani").catch(() => {})} title="NekoTrail is free and open source. Sponsoring keeps it that way."><Heart size={12} aria-hidden />Sponsor</button>
          <button className="link sponsor" onClick={() => openUrl("https://buymeacoffee.com/harshiljani").catch(() => {})} title="Buy the maintainer a coffee: a one-off thank you, no account needed."><Coffee size={12} aria-hidden />Buy me a coffee</button>
          <span className="spacer" />
          <button className="primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}
