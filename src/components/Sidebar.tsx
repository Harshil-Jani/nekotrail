import { useEffect, useState } from "react";
import { Archive, ArchiveRestore, Pin, Plus, Search, Settings2, Star } from "lucide-react";
import { api, starFor, type GithubUser, type Repo, type RepoGithub, type Sky, type Workspace } from "../api";
import { Mascot, type Mood } from "./Mascot";
import { Avatar } from "./Avatar";

type Props = {
  repos: Repo[];
  workspaces: Workspace[];
  sky: Sky;
  lit: number;
  me: GithubUser;
  running: Set<string>; // workspace ids with a running agent
  selected: string | null;
  showArchived: boolean;
  onSelect: (id: string) => void;
  onNewWorkspace: (repoId: string) => void;
  onAddRepo: () => void;
  onRepoSettings: (repoId: string) => void;
  onToggleArchived: () => void;
  onSky: () => void;
  mood: Mood;
  purring: boolean;
  onPet: () => void;
  /** Workspace being archived; its row animates off before it disappears. */
  leaving: string | null;
};

function RepoHead({ repo, onNew, onSettings }: { repo: Repo; onNew: () => void; onSettings: () => void }) {
  const [gh, setGh] = useState<RepoGithub>(null);
  useEffect(() => { api.repoGithub(repo.id).then(setGh); }, [repo.id]);
  return (
    <div className="repo-head">
      {gh?.owner_avatar ? (
        <Avatar src={gh.owner_avatar} size={20} label={gh.nameWithOwner} />
      ) : (
        <span className="avatar" style={{ width: 20, height: 20 }} />
      )}
      <span className="repo-name" title={gh?.description || repo.root_path}>{repo.name}</span>
      {gh && gh.stargazerCount > 0 && (
        <span className="repo-meta num" title={`${gh.stargazerCount} stars on ${gh.nameWithOwner}`}>
          <Star size={12} />{gh.stargazerCount >= 1000 ? `${(gh.stargazerCount / 1000).toFixed(1)}k` : gh.stargazerCount}
        </span>
      )}
      <span className="repo-actions">
        <button className="icon" title="Repo scripts" aria-label={`${repo.name} scripts`} onClick={onSettings}><Settings2 size={16} /></button>
        <button className="icon" title="New workspace (⌘N)" aria-label={`New workspace in ${repo.name}`} onClick={onNew}><Plus size={18} /></button>
      </span>
    </div>
  );
}

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const label = (w: Workspace) => starFor(p.sky, w.name)?.name ?? w.name;
  const match = (w: Workspace) => !q || label(w).toLowerCase().includes(q) || w.branch.toLowerCase().includes(q);
  const total = p.sky.stars.length;
  const pct = total ? Math.round((p.lit / total) * 100) : 0;

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <Mascot size={26} mood={p.mood} follow purring={p.purring} onClick={p.onPet} title="Pet the cat" />
        <span className="wordmark">nekotrail</span>
        {p.mood === "sleep" && <span className="zzz" aria-hidden>z z</span>}
        <span className="spacer" />
        <button className="icon" onClick={p.onAddRepo} title="Add a local git repo" aria-label="Add repo"><Plus size={18} /></button>
      </div>
      <label className="search">
        <Search size={14} />
        <input placeholder="Search workspaces" value={query} onChange={(e) => setQuery(e.target.value)} id="workspace-search" aria-label="Search workspaces (⌘K)" />
      </label>
      <div className="sidebar-scroll">
        {p.repos.length === 0 && <p className="muted" style={{ padding: "0 16px" }}>No repos yet. Add one with +.</p>}
        {p.repos.map((r) => {
          const ws = p.workspaces.filter((w) => w.repo_id === r.id && match(w));
          return (
            <section key={r.id} className="repo">
              <RepoHead repo={r} onNew={() => p.onNewWorkspace(r.id)} onSettings={() => p.onRepoSettings(r.id)} />
              {ws.map((w) => (
                <button
                  key={w.id}
                  className={`ws ${p.selected === w.id ? "selected" : ""} ${w.state === "archived" ? "archived" : ""} ${p.leaving === w.id ? "leaving" : ""}`}
                  onClick={() => p.onSelect(w.id)}
                >
                  <span className="ws-mark">
                    {p.running.has(w.id) ? <span className="spinner" aria-label="Agent working" /> : w.unread ? <span className="unread" aria-label="Unread" /> : null}
                  </span>
                  <span className="ws-text">
                    <span className="ws-name">{label(w)}</span>
                    <span className="ws-branch">{w.branch}</span>
                  </span>
                  <span className="spacer" />
                  {w.pinned && <Pin size={14} className="muted" aria-label="Pinned" />}
                </button>
              ))}
            </section>
          );
        })}
      </div>
      <div className="sidebar-foot">
        <button className="me" onClick={p.onSky} title="Open your night sky">
          {p.me ? <Avatar src={p.me.avatar_url} size={28} label={p.me.login} /> : <Mascot size={28} />}
          <span className="me-text">
            <span className="me-name">{p.me?.name || p.me?.login || "Not signed in to gh"}</span>
            <span className="muted num">{p.me ? `@${p.me.login} · ` : ""}{p.lit}/{total} stars</span>
          </span>
        </button>
        <div className="progress-bar" aria-label={`${pct}% of stars lit`}><span style={{ width: `${pct}%` }} /></div>
        <button className="quiet" onClick={p.onToggleArchived}>
          {p.showArchived ? <ArchiveRestore size={16} /> : <Archive size={16} />}
          {p.showArchived ? "Hide archived" : "Show archived"}
        </button>
      </div>
    </aside>
  );
}
