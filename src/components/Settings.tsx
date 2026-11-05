import { useEffect, useState } from "react";
import { Volume2 } from "lucide-react";
import { api, type AppSettings } from "../api";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import pkg from "../../package.json";
import { chime, getSettings, saveSettings, ting, type Settings as S } from "../settings";

const SHORTCUTS: [string, string][] = [
  ["⌘K", "Jump anywhere, run any command"],
  ["⌘N / ⌘⇧N", "New workspace / from a pull request"],
  ["⌘1 to ⌘9", "Go to a workspace in the sidebar"],
  ["⌘⌥↑ / ⌘⌥↓", "Previous / next workspace"],
  ["⌘⌥L", "Next workspace with news"],
  ["⌘L / ⌘⇧D / ⌘J", "Chat / Changes / Terminal"],
  ["⌘T", "New chat or terminal tab"],
  ["⌘F / ⌘G", "Search this chat / next match"],
  ["↵ or ⌘↵", "Send message (pick which above)"],
  ["⌘⇧↵", "Approve the agent's plan"],
  ["⌘⇧⌫", "Stop the agent"],
  ["Ctrl O", "Show every tool call's output"],
  ["⌘⇧P / ⌘⇧Y / ⌘⇧M", "Create PR / commit and push / merge"],
  ["⌘⇧L / ⌘⇧R / ⌘⇧X", "Pull latest / review / fix failing checks"],
  ["⌘⇧G", "Open the PR on GitHub"],
  ["⌘P", "Open any file"],
  ["⌘⌥U", "Changes: all or uncommitted"],
  ["⌘R / ⌘⇧B", "Run or stop / open localhost"],
  ["⌘O", "Open workspace in your editor"],
  ["⌘⇧A", "Archive workspace"],
  ["⌘B", "Hide or show the sidebar"],
  ["⌘+ / ⌘- / ⌘0", "Zoom"],
  ["⌘,", "Settings"],
  ["@ and /", "Mention a file, run a command"],
];

/** ⌘, : a few preferences and the shortcut list. Changes save as you make them. */
export function Settings({ onClose, home }: { onClose: () => void; home: string }) {
  const [s, setS] = useState<S>(getSettings);
  const [backend, setBackend] = useState<AppSettings | null>(null);
  const [targets, setTargets] = useState<{ id: string; name: string }[]>([]);
  const [version, setVersion] = useState(pkg.version);
  useEffect(() => { getVersion().then(setVersion).catch(() => {}); }, []);
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
        <h2>Settings</h2>

        <section>
          <h3>When an agent needs you</h3>
          <p className="muted setting-note">A plan to approve, a question, a sign-in, a usage limit or an error.</p>
          <label className="setting">
            <span>Ting ting</span>
            <span className="row">
              <button className="icon" title="Play it" aria-label="Play the needs-you sound" onClick={() => ting()}><Volume2 size={16} /></button>
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
            <span>Chime</span>
            <span className="row">
              <button className="icon" title="Play the chime" aria-label="Play the chime" onClick={() => chime(true)}><Volume2 size={16} /></button>
              <input type="checkbox" className="switch" checked={s.sound} onChange={(e) => { set("sound", e.target.checked); if (e.target.checked) chime(true); }} />
            </span>
          </label>
        </section>

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
            <span>Folder</span>
            <code className="muted">{backend?.home ? `${backend.home}/workspaces` : home}</code>
          </div>
        </section>

        <section>
          <h3>New chats start with</h3>
          {(["claude", "codex"] as const).map((a) => (
            <div className="setting" key={a}>
              <span>{a === "claude" ? "Claude" : "Codex"}</span>
              <span className="row">
                <input
                  className="setting-model"
                  placeholder="Default model"
                  defaultValue={backend?.[`${a}_model`] ?? ""}
                  key={`${a}-${backend ? "loaded" : "loading"}`}
                  onBlur={(e) => setRaw(`${a}_model`, e.target.value.trim())}
                  aria-label={`${a} default model`}
                />
                <select value={backend?.[`${a}_effort`] ?? ""} onChange={(e) => setRaw(`${a}_effort`, e.target.value)} aria-label={`${a} default effort`}>
                  <option value="">Default effort</option>
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                  <option value="xhigh">Extra high</option>
                  {a === "claude" && <option value="max">Max</option>}
                </select>
                <select value={backend?.[`${a}_mode`] || "acceptEdits"} onChange={(e) => setRaw(`${a}_mode`, e.target.value)} aria-label={`${a} default permissions`}>
                  <option value="plan">Plan</option>
                  <option value="acceptEdits">Auto-accept edits</option>
                  <option value="bypassPermissions">Full auto</option>
                </select>
              </span>
            </div>
          ))}
        </section>

        <section>
          <h3>Keyboard</h3>
          <dl className="shortcuts">
            {SHORTCUTS.map(([k, v]) => (
              <div key={k}><dt><kbd>{k}</kbd></dt><dd>{v}</dd></div>
            ))}
          </dl>
        </section>

        <div className="row end">
          <span className="muted version num">nekotrail v{version}</span>
          <button className="link" onClick={() => openUrl("https://github.com/Harshil-Jani/nekotrail/blob/main/CHANGELOG.md").catch(() => {})}>What's new</button>
          <span className="spacer" />
          <button className="primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}
