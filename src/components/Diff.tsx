import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { api, errText, type FileChange } from "../api";
import { Mascot } from "./Mascot";

function DiffLines({ text }: { text: string }) {
  const lines = text.split("\n");
  const start = lines.findIndex((l) => l.startsWith("@@"));
  return (
    <pre className="diff">
      {(start >= 0 ? lines.slice(start) : lines).map((l, i) => {
        const cls = l.startsWith("@@") ? "hunk" : l.startsWith("+") ? "add" : l.startsWith("-") ? "del" : "";
        return <div key={i} className={cls}>{l || " "}</div>;
      })}
    </pre>
  );
}

export function Diff({ workspaceId, refreshKey }: { workspaceId: string; refreshKey: number }) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [diff, setDiff] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    api.changes(workspaceId).then((f) => {
      setFiles(f);
      setError(null);
      setOpen((cur) => (cur && f.some((x) => x.path === cur) ? cur : f[0]?.path ?? null));
    }).catch((e) => setError(errText(e)));
  };

  useEffect(load, [workspaceId, refreshKey]);
  useEffect(() => {
    if (open) api.fileDiff(workspaceId, open).then(setDiff).catch((e) => setDiff(errText(e)));
    else setDiff("");
  }, [open, workspaceId, refreshKey]);

  if (error) return <div className="banner error">{error}</div>;
  if (!files) return <div className="empty"><span className="spinner" /></div>;
  if (files.length === 0)
    return (
      <div className="empty">
        <Mascot size={64} mood="sleep" />
        <h2>Nothing changed yet</h2>
        <p>Edits the agent makes in this workspace show up here, compared with the target branch.</p>
      </div>
    );

  const add = files.reduce((n, f) => n + f.additions, 0);
  const del = files.reduce((n, f) => n + f.deletions, 0);
  return (
    <div className="diff-view">
      <div className="file-list">
        <div className="file-list-head">
          <span className="num">{files.length} {files.length === 1 ? "file" : "files"}</span> <span className="add">+{add}</span> <span className="del">-{del}</span>
          <span className="spacer" /><button className="icon" onClick={load} title="Refresh" aria-label="Refresh changes"><RefreshCw size={16} /></button>
        </div>
        {files.map((f) => (
          <button key={f.path} className={`file ${open === f.path ? "selected" : ""}`} onClick={() => setOpen(f.path)}>
            <span className={`status s-${f.status}`}>{f.status}</span>
            <span className="file-path" title={f.path}>{f.path}</span>
            <span className="add">+{f.additions}</span>
            <span className="del">-{f.deletions}</span>
          </button>
        ))}
      </div>
      <div className="file-diff">{open && <DiffLines text={diff} />}</div>
    </div>
  );
}
