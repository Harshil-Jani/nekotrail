import { useState } from "react";
import { GitPullRequest, Plus } from "lucide-react";
import { displayName, statusOf, subName, STATUS_LABEL, type Repo, type Sky, type Workspace, type WsStatus } from "../api";

const COLUMNS = ["backlog", "in_progress", "in_review", "done"] as const;
const HINT: Record<(typeof COLUMNS)[number], string> = {
  backlog: "Ideas not started yet",
  in_progress: "An agent is on it",
  in_review: "PR open, waiting on review or CI",
  done: "Merged, closed or finished",
};

/**
 * Every workspace as a card on a board. Columns follow each workspace's status (from its PR unless set
 * by hand); drag a card to another column to set it. Respects the sidebar's project filter.
 */
export function Board({
  workspaces, repos, sky, prStates, lines, running, costs, repoFilter, onOpen, onSetStatus, onNew,
}: {
  workspaces: Workspace[];
  repos: Repo[];
  sky: Sky;
  prStates: Record<string, string>;
  lines: Record<string, [number, number]>;
  running: Set<string>;
  costs: Record<string, number>;
  repoFilter: string[];
  onOpen: (id: string) => void;
  onSetStatus: (id: string, status: WsStatus) => void;
  onNew: () => void;
}) {
  const [over, setOver] = useState<string | null>(null);
  const live = workspaces.filter((w) => w.state === "active" && (!repoFilter.length || repoFilter.includes(w.repo_id)));
  const repoName = (id: string) => repos.find((r) => r.id === id)?.name ?? "";
  return (
    <div className="board">
      <div className="board-head" data-tauri-drag-region>
        <h1>Board</h1>
        {live.length > 0 && <span className="muted">{live.length} workspaces{repoFilter.length ? ` in ${repoFilter.map(repoName).join(", ")}` : ""}. Drag a card to change its status.</span>}
        <span className="spacer" />
        {/* Empty, the page's own button is the one to press. */}
        {live.length > 0 && <button className="primary" onClick={onNew}><Plus size={14} />Create workspace</button>}
      </div>
      {live.length === 0 ? (
        <div className="empty board-empty">
          <h2>{repoFilter.length ? "No workspaces in this repo yet" : "No workspaces yet"}</h2>
          <p>Each task you start becomes a card here, and moves along as its PR goes from draft to merged.</p>
          {repos.length
            ? <button className="primary" onClick={onNew}><Plus size={14} />Create workspace</button>
            : <><p className="muted">Add a repo first: every workspace is a copy of one.</p><button className="primary" onClick={onNew}><Plus size={14} />Add a repo</button></>}
        </div>
      ) : (
      <div className="board-cols">
        {COLUMNS.map((col) => {
          const cards = live.filter((w) => statusOf(w, prStates[w.id]) === col);
          return (
            <section
              key={col}
              className={`board-col ${over === col ? "drop" : ""}`}
              onDragOver={(e) => { if (e.dataTransfer.types.includes("application/x-nekotrail-workspace")) { e.preventDefault(); setOver(col); } }}
              onDragLeave={() => setOver((o) => (o === col ? null : o))}
              onDrop={(e) => {
                const id = e.dataTransfer.getData("application/x-nekotrail-workspace");
                setOver(null);
                if (!id) return;
                const w = live.find((x) => x.id === id);
                // Dropping into the column it would land in anyway clears a manual status.
                const derived = w ? statusOf({ ...w, status: "" }, prStates[w.id]) : null;
                onSetStatus(id, derived === col ? "" : col);
              }}
            >
              <header>
                <span className={`status-dot st-${col}`} />
                <b>{STATUS_LABEL[col]}</b>
                <span className="muted num">{cards.length}</span>
              </header>
              <p className="muted col-hint">{HINT[col]}</p>
              <div className="cards">
                {cards.map((w) => {
                  const l = lines[w.id];
                  return (
                    <button
                      key={w.id}
                      className="card"
                      draggable
                      onDragStart={(e) => { e.dataTransfer.setData("application/x-nekotrail-workspace", w.id); e.dataTransfer.effectAllowed = "move"; }}
                      onClick={() => onOpen(w.id)}
                    >
                      <span className="card-top">
                        {running.has(w.id) ? <span className="spinner" aria-label="Agent working" /> : w.unread ? <span className="unread" aria-label="Unread" /> : null}
                        <span className="card-title">{displayName(sky, w)}</span>
                      </span>
                      <span className="card-meta muted">{subName(sky, w) && `${subName(sky, w)} · `}{repoName(w.repo_id)} · {w.branch}</span>
                      <span className="card-foot num">
                        {l && (l[0] > 0 || l[1] > 0) && <><span className="add">+{l[0]}</span><span className="del">-{l[1]}</span></>}
                        {prStates[w.id] && <span className="muted"><GitPullRequest size={11} /> {prStates[w.id].toLowerCase()}</span>}
                        {costs[w.id] ? <span className="muted">${costs[w.id].toFixed(2)}</span> : null}
                        {w.status && <span className="manual" title="Set by hand. Drag back to its automatic column to clear.">manual</span>}
                      </span>
                    </button>
                  );
                })}
                {cards.length === 0 && (over === col
                  ? <div className="card-empty">Drop here</div>
                  : col === "backlog" && <div className="card-empty">Drag a workspace here</div>)}
              </div>
            </section>
          );
        })}
      </div>
      )}
    </div>
  );
}
