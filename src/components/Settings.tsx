import { useEffect, useState } from "react";
import { Volume2 } from "lucide-react";
import { chime, getSettings, saveSettings, type Settings as S } from "../settings";

const SHORTCUTS: [string, string][] = [
  ["⌘K", "Jump anywhere, run any command"],
  ["⌘N", "New workspace"],
  ["⌘J", "Next workspace with news"],
  ["⌘1 to ⌘4", "Chat, Changes, Terminal, Notes"],
  ["⌘↵", "Send message"],
  ["⌘O", "Open workspace in your editor"],
  ["⌘+ / ⌘- / ⌘0", "Zoom"],
  ["⌘,", "Settings"],
  ["@", "Mention a file in the composer"],
];

/** ⌘, : a few preferences and the shortcut list. Changes save as you make them. */
export function Settings({ onClose, home }: { onClose: () => void; home: string }) {
  const [s, setS] = useState<S>(getSettings);
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
          <h3>Workspaces</h3>
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
