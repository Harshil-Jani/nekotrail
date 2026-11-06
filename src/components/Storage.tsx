import { useEffect, useState } from "react";
import { HardDrive, X } from "lucide-react";
import { api } from "../api";

type Item = Awaited<ReturnType<typeof api.workspaceStorage>>[number];
const size = (kb: number) => (kb >= 1024 * 1024 ? `${(kb / 1024 / 1024).toFixed(1)} GB` : kb >= 1024 ? `${Math.round(kb / 1024)} MB` : `${kb} KB`);

/** Disk used per workspace, and one click to delete build output git ignores (dependencies, caches). */
export function Storage({ names, onClose, onSay }: { names: (id: string) => string; onClose: () => void; onSay: (t: string, err?: boolean) => void }) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [free, setFree] = useState<number | null>(null);
  const load = () => {
    api.workspaceStorage().then((x) => setItems(x.sort((a, b) => b.total_kb - a.total_kb))).catch(() => setItems([]));
    api.freeDiskMb().then(setFree).catch(() => {});
  };
  useEffect(load, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const clean = async (ids: string[]) => {
    setBusy(ids.length > 1 ? "all" : ids[0]);
    let freed = 0;
    for (const id of ids) {
      try { freed += await api.cleanWorkspace(id); } catch (e) { onSay(String(e), true); }
    }
    setBusy(null);
    onSay(`Freed ${size(freed)}. Your agents rebuild these folders when they need them.`);
    load();
  };
  const total = items?.reduce((n, i) => n + i.total_kb, 0) ?? 0;
  const cleanable = items?.filter((i) => i.cleanable_kb > 1024) ?? [];
  const cleanableKb = cleanable.reduce((n, i) => n + i.cleanable_kb, 0);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal storage" role="dialog" aria-modal="true" aria-label="Storage" onClick={(e) => e.stopPropagation()}>
        <div className="row"><HardDrive size={16} /><h2>Storage</h2><span className="spacer" /><button className="icon" aria-label="Close" onClick={onClose}><X size={15} /></button></div>
        <p className="muted">
          {items ? (items.length ? `Workspaces use ${size(total)}.` : "") : "Measuring…"} {free !== null ? `${size(free * 1024)} free on this disk.` : ""} Cleaning deletes only dependency and build folders that git ignores (like node_modules, .venv, target, .next). Code, commits and .env files are never touched.
        </p>
        {items === null && <p className="muted"><span className="spinner" /> Measuring workspace folders…</p>}
        {items?.length === 0 && <p className="muted">No workspaces yet, so there is nothing to clean. Each workspace you create shows up here with its size.</p>}
        {!!items?.length && (
          <ul className="storage-list">
            {items.map((i) => (
              <li key={i.workspace_id}>
                <div>
                  <b>{names(i.workspace_id)}</b>
                  <span className="muted num">{size(i.total_kb)}{i.cleanable_kb > 1024 ? ` · ${size(i.cleanable_kb)} is build output` : ""}</span>
                </div>
                <div className="storage-bar"><span style={{ width: `${total ? Math.max(1, (i.total_kb / items[0].total_kb) * 100) : 0}%` }} /></div>
                <button disabled={!!busy || i.cleanable_kb <= 1024} onClick={() => clean([i.workspace_id])} title={i.dirs.map((d) => d.path).join("\n")}>
                  {busy === i.workspace_id ? <span className="spinner" /> : null}Clean
                </button>
              </li>
            ))}
          </ul>
        )}
        {!!items?.length && <div className="row end">
          <span className="spacer" />
          <button className="accent" disabled={!!busy || cleanable.length === 0} onClick={() => clean(cleanable.map((i) => i.workspace_id))}>
            {busy === "all" ? <span className="spinner" /> : null}Clean all · free {size(cleanableKb)}
          </button>
        </div>}
      </div>
    </div>
  );
}
