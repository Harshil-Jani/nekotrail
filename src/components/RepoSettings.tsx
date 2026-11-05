import { useState } from "react";
import { api, errText, type Repo } from "../api";

export function RepoSettings({ repo, onClose }: { repo: Repo; onClose: () => void }) {
  const [s, setS] = useState({ setup: repo.setup_script ?? "", run: repo.run_script ?? "", archive: repo.archive_script ?? "" });
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    try {
      const n = (v: string) => (v.trim() ? v : null);
      await api.saveRepoScripts(repo.id, { setup: n(s.setup), run: n(s.run), archive: n(s.archive) });
      onClose();
    } catch (e) {
      setError(errText(e));
    }
  };
  const field = (key: keyof typeof s, label: string, hint: string) => (
    <label className="field">
      <span>{label}</span>
      <textarea rows={2} value={s[key]} placeholder={hint} onChange={(e) => setS({ ...s, [key]: e.target.value })} />
    </label>
  );
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{repo.name} scripts</h2>
        <p className="muted">
          Run in each workspace folder. A <code>nekotrail.json</code> committed in the repo overrides these, so your team can share one setup.
        </p>
        {field("setup", "Setup: runs when a workspace is created", "bun install && cp ../.env .env")}
        {field("run", "Run: the Run button", "bun dev")}
        {field("archive", "Archive: runs before the folder is removed", "docker compose down")}
        {error && <div className="banner error">{error}</div>}
        <div className="row end">
          <button className="quiet" onClick={onClose}>Cancel</button>
          <button className="primary" onClick={save}>Save</button>
        </div>
      </div>
    </div>
  );
}
