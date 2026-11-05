import { useEffect, useState } from "react";
import { Plus, X } from "lucide-react";
import { api, errText, type Repo, type RepoPrefs } from "../api";

type Section = "scripts" | "agent" | "files";

/** Everything nekotrail knows about one repo: scripts, instructions for the agent, files and env. */
export function RepoSettings({ repo, onClose }: { repo: Repo; onClose: () => void }) {
  const [section, setSection] = useState<Section>("scripts");
  const [s, setS] = useState({ setup: repo.setup_script ?? "", run: repo.run_script ?? "", archive: repo.archive_script ?? "" });
  const [p, setP] = useState<RepoPrefs>(() => ({
    ...{ general_prompt: "", review_prompt: "", pr_prompt: "", copy_files: [], env: [], merge_method: "", actions: [] },
    ...(repo.prefs as Partial<RepoPrefs> | undefined),
  }));
  const [globs, setGlobs] = useState(p.copy_files.join("\n"));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = async () => {
    try {
      const n = (v: string) => (v.trim() ? v : null);
      await api.saveRepoScripts(repo.id, { setup: n(s.setup), run: n(s.run), archive: n(s.archive) });
      await api.setRepoPrefs(repo.id, {
        ...p,
        copy_files: globs.split("\n").map((g) => g.trim()).filter(Boolean),
        env: p.env.filter(([k]) => k.trim()).map(([k, v]) => [k.trim(), v]),
        actions: (p.actions ?? []).filter(([n, c]) => n.trim() && c.trim()).map(([n, c]) => [n.trim(), c.trim()]),
      });
      onClose();
    } catch (e) {
      setError(errText(e));
    }
  };

  const script = (key: keyof typeof s, label: string, hint: string) => (
    <label className="field">
      <span>{label}</span>
      <textarea rows={2} value={s[key]} placeholder={hint} onChange={(e) => setS({ ...s, [key]: e.target.value })} />
    </label>
  );
  const prompt = (key: "general_prompt" | "review_prompt" | "pr_prompt", label: string, hint: string, rows = 3) => (
    <label className="field">
      <span>{label}</span>
      <textarea rows={rows} value={p[key]} placeholder={hint} onChange={(e) => setP({ ...p, [key]: e.target.value })} />
    </label>
  );

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal repo-settings" role="dialog" aria-modal="true" aria-label={`${repo.name} settings`} onClick={(e) => e.stopPropagation()}>
        <h2>{repo.name}</h2>
        <nav className="seg" role="tablist">
          {([["scripts", "Scripts"], ["agent", "Agent"], ["files", "Files and env"]] as [Section, string][]).map(([id, label]) => (
            <button key={id} role="tab" aria-selected={section === id} className={section === id ? "on" : ""} onClick={() => setSection(id)}>{label}</button>
          ))}
        </nav>

        {section === "scripts" && (
          <>
            <p className="muted">
              These run in each workspace folder. A <code>nekotrail.json</code> committed in the repo overrides these, so your team can share one setup.
            </p>
            {script("setup", "Setup: runs when a workspace is created", "bun install")}
            {script("run", "Run: runs from the Run button (⌘R)", "bun dev --port $NEKOTRAIL_PORT")}
            {script("archive", "Archive: runs before the folder is removed", "docker compose down")}
            <div className="field">
              <span>More actions</span>
              <small className="muted">Named commands next to Run, like test or lint. Start them from the Run menu, ⌘K, or /run name in a chat. They run in the workspace terminal.</small>
              {(p.actions ?? []).map(([n, c], i) => (
                <div className="env-row" key={i}>
                  <input value={n} placeholder="test" aria-label="Action name" onChange={(e) => setP({ ...p, actions: p.actions.map((x, j) => (j === i ? [e.target.value.replace(/\s+/g, "-"), x[1]] : x)) })} />
                  <input value={c} placeholder="bun test" aria-label="Action command" onChange={(e) => setP({ ...p, actions: p.actions.map((x, j) => (j === i ? [x[0], e.target.value] : x)) })} />
                  <button className="icon" aria-label="Remove action" onClick={() => setP({ ...p, actions: p.actions.filter((_, j) => j !== i) })}><X size={14} /></button>
                </div>
              ))}
              <button className="quiet add-env" onClick={() => setP({ ...p, actions: [...(p.actions ?? []), ["", ""]] })}><Plus size={14} />Add action</button>
            </div>
          </>
        )}

        {section === "agent" && (
          <>
            {prompt("general_prompt", "Instructions for every chat in this repo", "Use pnpm, not npm. Run `pnpm test` before you say you're done.", 4)}
            {prompt("review_prompt", "Extra instructions for Review (⌘⇧R)", "Focus on security and missing tests.")}
            {prompt("pr_prompt", "Extra instructions for Create PR (⌘⇧P)", "Link the Jira ticket from the branch name. Keep the description under 10 lines.")}
            <label className="field">
              <span>Merge PRs with</span>
              <select value={p.merge_method || "squash"} onChange={(e) => setP({ ...p, merge_method: e.target.value })}>
                <option value="squash">Squash and merge</option>
                <option value="merge">Merge commit</option>
                <option value="rebase">Rebase and merge</option>
              </select>
            </label>
          </>
        )}

        {section === "files" && (
          <>
            <label className="field">
              <span>Files to copy into new workspaces</span>
              <textarea rows={3} value={globs} placeholder={".env*\napps/web/.env.local"} onChange={(e) => setGlobs(e.target.value)} />
              <small className="muted">One per line. These are git-ignored, so a new workspace wouldn't have them otherwise. <code>*</code> works in file names.</small>
            </label>
            <div className="field">
              <span>Environment variables</span>
              <small className="muted">For agents, scripts and terminals in this repo. Every workspace also gets <code>NEKOTRAIL_PORT</code> (10 free ports from there), <code>NEKOTRAIL_WORKSPACE_NAME</code> and <code>NEKOTRAIL_ROOT_PATH</code>.</small>
              {p.env.map(([k, v], i) => (
                <div className="env-row" key={i}>
                  <input value={k} placeholder="NAME" aria-label="Variable name" onChange={(e) => setP({ ...p, env: p.env.map((x, j) => (j === i ? [e.target.value, x[1]] : x)) })} />
                  <input value={v} placeholder="value" aria-label="Variable value" onChange={(e) => setP({ ...p, env: p.env.map((x, j) => (j === i ? [x[0], e.target.value] : x)) })} />
                  <button className="icon" aria-label="Remove variable" onClick={() => setP({ ...p, env: p.env.filter((_, j) => j !== i) })}><X size={14} /></button>
                </div>
              ))}
              <button className="quiet add-env" onClick={() => setP({ ...p, env: [...p.env, ["", ""]] })}><Plus size={14} />Add variable</button>
            </div>
          </>
        )}

        {error && <div className="banner error">{error}</div>}
        <div className="row end">
          <button className="quiet" onClick={onClose}>Cancel</button>
          <button className="primary" onClick={save}>Save</button>
        </div>
      </div>
    </div>
  );
}
