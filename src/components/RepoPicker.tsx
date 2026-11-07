import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import type { Repo } from "../api";
import { Avatar } from "./Avatar";
import { useRepoAvatar } from "../repoAvatar";
import { useMenuPlace } from "../menuPlace";

/** A repo's picture with its letter as the fallback, the same one the sidebar shows. */
export function RepoAvatar({ repo, size = 20 }: { repo: Repo; size?: number }) {
  const src = useRepoAvatar(repo.id);
  return <Avatar key={src} src={src} size={size} label={repo.name} />;
}

/** The folder a repo sits in, "~/code" rather than the full home path: enough to tell two same-named repos apart. */
const shortPath = (p: string) => p.replace(/\/+$/, "").replace(/\/[^/]*$/, "").replace(/^\/(Users|home)\/[^/]+/, "~") || "/";

/**
 * The new-workspace repo picker: each repo with its picture, type to filter, Up and Down to move,
 * Enter to pick, Esc to close. The current repo is ticked.
 */
export function RepoPicker({ repos, value, onChange }: { repos: Repo[]; value: string; onChange: (id: string) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const root = useRef<HTMLSpanElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const current = repos.find((r) => r.id === value);
  const q = query.trim().toLowerCase();
  const shown = q ? repos.filter((r) => r.name.toLowerCase().includes(q) || r.root_path.toLowerCase().includes(q)) : repos;
  useMenuPlace(menu, open, [shown.length]);

  const show = () => { setQuery(""); setSel(Math.max(0, repos.findIndex((r) => r.id === value))); setOpen(true); };
  const close = (refocus = true) => { setOpen(false); if (refocus) btn.current?.focus(); };
  const choose = (r: Repo | undefined) => { if (!r) return; onChange(r.id); close(); };

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => { if (!root.current?.contains(e.target as Node)) setOpen(false); };
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [open]);
  // Keep the highlighted row in view while arrowing through a long list.
  useEffect(() => { list.current?.querySelector<HTMLElement>(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" }); }, [sel, open]);

  return (
    <span className="nw-repo-picker" ref={root}>
      <button ref={btn} className="nw-repo-btn" aria-haspopup="listbox" aria-expanded={open} onClick={() => (open ? close(false) : show())}
        onKeyDown={(e) => { if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !open) { e.preventDefault(); show(); } }}
        title={current?.root_path}>
        {current && <RepoAvatar repo={current} />}
        <span className="nw-repo-name">{current?.name ?? "Pick a repo"}</span>
        <ChevronDown size={14} aria-hidden />
      </button>
      {open && (
        <div ref={menu} className="menu nw-menu nw-repo-menu" role="dialog" aria-label="Pick a repo">
          <label className="nw-menu-search">
            <Search size={13} aria-hidden />
            <input
              autoFocus value={query} placeholder="Find a repo" aria-label="Find a repo" spellCheck={false}
              role="combobox" aria-expanded aria-controls="nw-repo-list" aria-activedescendant={shown[sel] ? `nw-repo-${shown[sel].id}` : undefined}
              onChange={(e) => { setQuery(e.target.value); setSel(0); }}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  if (shown.length) setSel((s) => (s + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length);
                } else if (e.key === "Enter") { e.preventDefault(); choose(shown[sel]); }
                else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); }
                else if (e.key === "Tab") setOpen(false);
              }}
            />
          </label>
          <div className="nw-menu-list" id="nw-repo-list" role="listbox" aria-label="Repos" ref={list}>
            {shown.map((r, i) => (
              <button key={r.id} id={`nw-repo-${r.id}`} data-i={i} role="option" tabIndex={-1} aria-selected={i === sel}
                className={i === sel ? "active" : ""} onMouseMove={() => i !== sel && setSel(i)} onClick={() => choose(r)} title={r.root_path}>
                {r.id === value ? <Check size={14} /> : <span className="check-space" />}
                <RepoAvatar repo={r} size={18} />
                <span className="nw-menu-name">{r.name}</span>
                <span className="nw-menu-path">{shortPath(r.root_path)}</span>
              </button>
            ))}
            {shown.length === 0 && <p className="muted nw-menu-empty">No repo matches “{query.trim()}”.</p>}
          </div>
        </div>
      )}
    </span>
  );
}
