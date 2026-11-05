import { useEffect, useState } from "react";
import { Volume2 } from "lucide-react";
import { api } from "../api";
import { chime, getSettings, saveSettings, type Settings as S } from "../settings";

const SHORTCUTS: [string, string][] = [
  ["⌘K", "Jump anywhere, run any command"],
  ["⌘N", "New workspace"],
  ["⌘J", "Next workspace with news"],
  ["⌘1 to ⌘4", "Chat, Changes, Terminal, Notes"],
  ["↵ or ⌘↵", "Send message (pick which above)"],
  ["⌘⇧P / ⌘⇧Y / ⌘⇧M", "Create PR, commit and push, merge"],
  ["⌘⇧L / ⌘⇧R / ⌘⇧X", "Pull latest, review, fix failing checks"],
  ["⌘O", "Open workspace in your editor"],
  ["⌘+ / ⌘- / ⌘0", "Zoom"],
  ["⌘,", "Settings"],
  ["@", "Mention a file in the composer"],
];

/** ⌘, : a few preferences and the shortcut list. Changes save as you make them. */
export function Settings({ onClose, home }: { onClose: () => void; home: string }) {
  const [s, setS] = useState<S>(getSettings);
  const [backend, setBackend] = useState<{ caffeinate: "on" | "off"; auto_branch: "on" | "off" } | null>(null);
  useEffect(() => { api.appSettings().then(setBackend).catch(() => {}); }, []);
  const setB = (key: "caffeinate" | "auto_branch", on: boolean) => {
    const v = on ? "on" : "off";
    setBackend((b) => (b ? { ...b, [key]: v } : b));
    api.setAppSetting(key, v);
  };
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
            <select value={s.editor} onChange={(e) => set("editor", e.target.value as S["editor"])}>
              <option value="cursor">Cursor</option>
              <option value="vscode">VS Code</option>
              <option value="finder">Finder</option>
              <option value="terminal">Terminal</option>
            </select>
          </label>
          <div className="setting">
            <span>Folder</span>
            <code className="muted">{home}</code>
          </div>
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
          <button className="primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}
