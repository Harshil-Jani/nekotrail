import { useEffect, useState } from "react";
import { Copy, Download, Lock, Pencil, Plus, Trash2, Upload } from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, errText } from "../api";
import { keyHint } from "../platform";
import { loadPrompts, personalOf, type PersonalPrompt, type PromptEntry } from "../prompts";
import { ask } from "./Confirm";
import { PromptFields, SegText } from "./PromptMenu";

type Draft = PersonalPrompt & { was: string | null };

/** Settings, Prompts: your prompts (editable), the repo's and Claude Code's (read-only, can be copied). */
export function PromptsPane({ workspaceId, repoName }: { workspaceId?: string; repoName?: string }) {
  const where = { id: workspaceId };
  const [list, setList] = useState<PromptEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [draftError, setDraftError] = useState<string | null>(null);
  const reload = () => loadPrompts(where, true).then((l) => { setList(l); setError(null); }).catch((e) => setError(errText(e)));
  useEffect(() => { reload(); }, [workspaceId]);

  const saveAll = async (next: PersonalPrompt[]) => {
    await api.promptsSave(next);
    await reload();
    window.dispatchEvent(new Event("nekotrail-prompts"));
  };
  const mine = list ? personalOf(list) : [];

  const saveDraft = async () => {
    if (!draft) return;
    setDraftError(null);
    const item = { name: draft.name.trim(), description: draft.description.trim(), text: draft.text };
    const clash = mine.find((p) => p.name.toLowerCase() === item.name.toLowerCase() && p.name !== draft.was);
    if (clash) return setDraftError(`You already have /${item.name}. Pick another name.`);
    const next = draft.was === null ? [...mine, item] : mine.map((p) => (p.name === draft.was ? item : p));
    try {
      await saveAll(next);
      setDraft(null);
    } catch (e) {
      setDraftError(errText(e));
    }
  };
  const duplicate = async (e: PromptEntry) => {
    try {
      const name = await api.promptsDuplicate(where, e.name, e.scope);
      await reload();
      window.dispatchEvent(new Event("nekotrail-prompts"));
      setNote(`Copied to your prompts as /${name}.`);
    } catch (err) { setError(errText(err)); }
  };
  const remove = async (e: PromptEntry) => {
    const ok = await ask({ title: `Delete /${e.name}?`, body: "It's gone from every workspace. Export your prompts first if you might want it back.", ok: "Delete", danger: true });
    if (!ok) return;
    try { await saveAll(mine.filter((p) => p.name !== e.name)); } catch (err) { setError(errText(err)); }
  };
  const doExport = async () => {
    try {
      const path = await saveDialog({ defaultPath: "nekotrail-prompts.json", filters: [{ name: "JSON", extensions: ["json"] }] });
      if (!path) return;
      const n = await api.promptsExport(path);
      setNote(`Exported ${n} ${n === 1 ? "prompt" : "prompts"}.`);
    } catch (err) { setError(errText(err)); }
  };
  const doImport = async () => {
    try {
      const path = await openDialog({ multiple: false, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (!path || Array.isArray(path)) return;
      const r = await api.promptsImport(path);
      await reload();
      window.dispatchEvent(new Event("nekotrail-prompts"));
      const parts = [
        `Added ${r.added.length + r.renamed.length}.`,
        r.renamed.length ? `Renamed so nothing of yours was replaced: ${r.renamed.map(([a, b]) => `/${a} to /${b}`).join(", ")}.` : "",
        r.skipped.length ? `Already had ${r.skipped.length}.` : "",
      ];
      setNote(parts.filter(Boolean).join(" "));
    } catch (err) { setError(errText(err)); }
  };

  const groups: [string, PromptEntry[]][] = list ? [
    ["Yours", list.filter((e) => e.scope === "personal")],
    [repoName ? `In ${repoName}` : "In this repo", list.filter((e) => e.scope === "repo")],
    ["From Claude Code", list.filter((e) => e.scope === "claude")],
  ] : [];

  const editor = (
    <div className="prompt-edit">
      {draft && <PromptFields name={draft.name} description={draft.description} text={draft.text} autoFocus
        onName={(name) => setDraft({ ...draft, name })} onDescription={(description) => setDraft({ ...draft, description })} onText={(text) => setDraft({ ...draft, text })} />}
      {draftError && <p className="bad setting-note" role="alert">{draftError}</p>}
      <div className="row end">
        <button onClick={() => { setDraft(null); setDraftError(null); }}>Cancel</button>
        <button className="primary" onClick={saveDraft} disabled={!draft?.name.trim() || !draft?.text.trim()}>Save</button>
      </div>
    </div>
  );

  return (
    <section className="prompts-pane">
      <div className="row prompts-head">
        <h3>Prompts</h3>
        <span className="spacer" />
        <button className="quiet" onClick={doImport} title="Add prompts from an export file. Nothing of yours is replaced."><Upload size={14} />Import</button>
        <button className="quiet" onClick={doExport} disabled={!mine.length} title="Save your prompts to a JSON file"><Download size={14} />Export</button>
        <button onClick={() => { setDraftError(null); setDraft({ name: "", description: "", text: "", was: null }); }} disabled={draft?.was === null}><Plus size={14} />New prompt</button>
      </div>
      <p className="muted setting-note">Type /name in the chat box, in New workspace or in {keyHint("⌘K")} to insert one. A team can share prompts in the repo, in <code>.nekotrail/prompts/*.md</code> or a <code>prompts</code> list in <code>nekotrail.json</code>.</p>
      {note && <p className="muted setting-note" role="status">{note}</p>}
      {error && <p className="bad setting-note" role="alert">{error}</p>}
      {draft?.was === null && editor}
      {list === null && !error && <p className="muted setting-note"><span className="spinner" /> Loading prompts…</p>}
      {groups.map(([title, items], gi) => (gi === 0 || items.length > 0) && (
        <div key={title} className="prompt-group">
          <h4>{title}{gi > 0 && <span className="muted"> · read-only</span>}</h4>
          {gi === 2 && <p className="muted setting-note">Your <code>.claude/commands</code>. Claude runs them itself, so they show in the / menu of Claude chats. Duplicate one to use it with any agent.</p>}
          {gi === 0 && items.length === 0 &&<p className="muted setting-note">No prompts yet. Save one from the chat box, or click New prompt.</p>}
          {items.map((e) => draft?.was === e.name && e.scope === "personal" ? <div key={`${e.scope}-${e.name}`}>{editor}</div> : (
            <div key={`${e.scope}-${e.name}`} className={`prompt-row ${e.hidden_by ? "hidden" : ""}`}>
              <div className="prompt-row-main">
                <span className="prompt-row-name">
                  {e.read_only && <Lock size={11} aria-label="Read-only" />}
                  <b>/{e.name}</b>
                  {e.description && <span className="muted">{e.description}</span>}
                </span>
                <span className="prompt-row-text muted"><SegText segs={e.segs} max={160} /></span>
                {(e.source || e.hidden_by) && (
                  <span className="prompt-row-meta muted">
                    {e.source && <code>{e.source}</code>}
                    {e.hidden_by && <span>{e.hidden_by === "repo" ? `Not used here: the repo has its own /${e.name}` : `Not used: you have your own /${e.name}`}</span>}
                  </span>
                )}
              </div>
              <span className="prompt-row-actions">
                {!e.read_only && <button className="icon" aria-label={`Edit /${e.name}`} title="Edit" onClick={() => { setDraftError(null); setDraft({ name: e.name, description: e.description, text: e.text, was: e.name }); }}><Pencil size={14} /></button>}
                <button className="icon" aria-label={`Duplicate /${e.name}`} title={e.read_only ? "Copy to your prompts, where you can edit it" : "Duplicate"} onClick={() => duplicate(e)}><Copy size={14} /></button>
                {!e.read_only && <button className="icon" aria-label={`Delete /${e.name}`} title="Delete" onClick={() => remove(e)}><Trash2 size={14} /></button>}
              </span>
            </div>
          ))}
        </div>
      ))}
    </section>
  );
}
