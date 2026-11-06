import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Send, Trash2 } from "lucide-react";
import { api, errText } from "../api";
import { IS_MAC } from "../platform";
import { ask } from "./Confirm";
import { EVENTS, SERVICES, secretReady, serviceInfo, statusLine, type AlertEvent, type AlertSecret, type AlertService, type AlertsConfig, type AlertsView, type Destination } from "../alerts";

const EMPTY: AlertSecret = { url: "", token: "", user: "" };
const KEYCHAIN = IS_MAC ? "your Keychain" : "your keyring";
const AWAY = IS_MAC ? "Away from your Mac" : "Away from your computer";

/** One destination: where it sends, which events, the last line, a test, and how it went last. */
function DestinationCard({ d, view, onChange, onView }: {
  d: Destination; view: AlertsView;
  onChange: (next: Destination) => void; onView: (v: AlertsView) => void;
}) {
  const info = serviceInfo(d.service);
  const [editing, setEditing] = useState(!d.saved);
  const [secret, setSecret] = useState<AlertSecret>(EMPTY);
  const [busy, setBusy] = useState<"save" | "test" | null>(null);
  const [note, setNote] = useState<{ text: string; bad?: boolean } | null>(null);
  const [name, setName] = useState(d.name);
  useEffect(() => setName(d.name), [d.name]);
  const save = async () => {
    setBusy("save"); setNote(null);
    try {
      onView(await api.alertsSetSecret(d.id, secret));
      setSecret(EMPTY); setEditing(false);
      setNote({ text: `Saved in ${KEYCHAIN}.` });
    } catch (e) { setNote({ text: errText(e), bad: true }); } finally { setBusy(null); }
  };
  const test = async () => {
    setBusy("test"); setNote(null);
    try { onView(await api.alertsTest(d.id)); setNote({ text: "Test sent. Check that it arrived." }); }
    catch (e) { setNote({ text: errText(e), bad: true }); api.alertsView().then(onView).catch(() => {}); }
    finally { setBusy(null); }
  };
  const remove = async () => {
    if (!(await ask({ title: `Remove ${d.name}?`, body: `Its details are deleted from ${KEYCHAIN} too.`, ok: "Remove", danger: true }))) return;
    try { onView(await api.alertsRemove(d.id)); } catch (e) { setNote({ text: errText(e), bad: true }); }
  };
  const toggle = (ev: AlertEvent, on: boolean) =>
    onChange({ ...d, events: on ? EVENTS.map((e) => e.id).filter((id) => id === ev || d.events.includes(id)) : d.events.filter((x) => x !== ev) });
  const status = statusLine(d, view.status[d.id], view.held[d.id] ?? 0, view.quiet_now);
  return (
    <div className={`alert-dest ${d.on ? "" : "off"}`}>
      <div className="alert-dest-head">
        <input className="alert-dest-name" value={name} aria-label="Destination name" maxLength={40}
          onChange={(e) => setName(e.target.value)} onBlur={() => name !== d.name && onChange({ ...d, name })} />
        <span className="chip read-only">{info.name}</span>
        {d.saved && !editing && <span className="muted alert-dest-host" title="Where it sends. The full address stays in the keychain.">{d.hint}</span>}
        <span className="spacer" />
        <input type="checkbox" className="switch" checked={d.on} aria-label={`Send to ${d.name}`} onChange={(e) => onChange({ ...d, on: e.target.checked })} />
        <button className="icon quiet" aria-label={`Remove ${d.name}`} title="Remove" onClick={remove}><Trash2 size={14} /></button>
      </div>

      {editing ? <div className="alert-secret">
        <p className="muted setting-note">{info.note} Kept in {KEYCHAIN}, never in nekotrail's database or logs.</p>
        {info.fields.map((f) => (
          <label key={f.key} className="alert-field">
            <span>{f.label}{f.optional && <span className="muted"> (optional)</span>}</span>
            <input type="password" autoComplete="off" spellCheck={false} placeholder={f.placeholder} value={secret[f.key]}
              onChange={(e) => setSecret({ ...secret, [f.key]: e.target.value })}
              onKeyDown={(e) => { if (e.key === "Enter" && secretReady(d.service, secret)) save(); }} />
          </label>
        ))}
        <div className="row end">
          {d.saved && <button className="quiet" onClick={() => { setEditing(false); setSecret(EMPTY); }}>Cancel</button>}
          <button className="primary" disabled={!secretReady(d.service, secret) || busy !== null} onClick={save}>{busy === "save" ? "Saving…" : "Save"}</button>
        </div>
      </div> : <>
        <div className="alert-events" role="group" aria-label={`Events sent to ${d.name}`}>
          {EVENTS.map((ev) => (
            <label key={ev.id}><input type="checkbox" checked={d.events.includes(ev.id)} onChange={(e) => toggle(ev.id, e.target.checked)} />{ev.label}</label>
          ))}
        </div>
        <label className="setting">
          <span>Include the agent's last line<span className="muted setting-sub">Secrets in it are hidden, as in Export session.</span></span>
          <input type="checkbox" className="switch" checked={d.last_line} onChange={(e) => onChange({ ...d, last_line: e.target.checked })} />
        </label>
      </>}

      {(d.saved || note) && <div className="alert-dest-foot">
        <span className={`alert-status ${(note ? note.bad : status.bad) ? "bad" : "muted"}`} role="status">{note?.text ?? status.text}</span>
        <span className="spacer" />
        {d.saved && !editing && <button className="quiet" onClick={() => setEditing(true)}>Change details</button>}
        {d.saved && !editing && <button onClick={test} disabled={busy !== null}><Send size={13} />{busy === "test" ? "Sending…" : "Send a test"}</button>}
      </div>}
    </div>
  );
}

/** Settings, Notifications: away alerts. Off until a destination is added and saved. */
export function AwayAlerts() {
  const [view, setView] = useState<AlertsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const load = () => api.alertsView().then(setView).catch((e) => setError(errText(e)));
    load();
    const un = listen("alerts-status", load);
    return () => { un.then((f) => f()); };
  }, []);
  const save = async (config: AlertsConfig) => {
    setView((v) => (v ? { ...v, config } : v));
    try { setView(await api.alertsSave(config)); setError(null); } catch (e) { setError(errText(e)); }
  };
  const add = async (service: AlertService) => {
    try { setView(await api.alertsAdd(service)); setError(null); } catch (e) { setError(errText(e)); }
  };
  if (!view) return error ? <section><h3>{AWAY}</h3><p className="bad setting-note" role="alert">{error}</p></section> : null;
  const cfg = view.config;
  const setDest = (next: Destination) => save({ ...cfg, destinations: cfg.destinations.map((d) => (d.id === next.id ? next : d)) });
  const hasMorning = cfg.destinations.some((d) => d.events.includes("morning_summary"));
  return <>
    <section className="away-alerts">
      <div className="row">
        <h3>{AWAY}</h3>
        <span className="spacer" />
        <select className="alert-add" value="" aria-label="Add a destination" onChange={(e) => e.target.value && add(e.target.value as AlertService)}>
          <option value="">Add a destination…</option>
          {SERVICES.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
      </div>
      <p className="muted setting-note">
        A short message on your phone or in a team chat. Off until you add a destination. Each message names the workspace, its repo and what happened, with a link that opens it on {IS_MAC ? "this Mac" : "this computer"}. Never your code, prompts, commands or diffs.
      </p>
      {error && <p className="bad setting-note" role="alert">{error}</p>}
      {cfg.destinations.map((d) => <DestinationCard key={d.id} d={d} view={view} onChange={setDest} onView={setView} />)}
      {cfg.destinations.length > 0 && <p className="muted setting-note">Several updates within a minute arrive as one message, and each destination gets at most one a minute.</p>}
    </section>

    {cfg.destinations.length > 0 && <section>
      <h3>Quiet hours</h3>
      <label className="setting">
        <span>Hold messages overnight<span className="muted setting-sub">They arrive as one message when quiet hours end.</span></span>
        <input type="checkbox" className="switch" checked={cfg.quiet.on} onChange={(e) => save({ ...cfg, quiet: { ...cfg.quiet, on: e.target.checked } })} />
      </label>
      <div className={`setting ${cfg.quiet.on ? "" : "dim"}`}>
        <span>Quiet from</span>
        <span className="row alert-times">
          <input type="time" aria-label="Quiet hours start" value={cfg.quiet.from} disabled={!cfg.quiet.on} onChange={(e) => e.target.value && save({ ...cfg, quiet: { ...cfg.quiet, from: e.target.value } })} />
          <span className="muted">to</span>
          <input type="time" aria-label="Quiet hours end" value={cfg.quiet.to} disabled={!cfg.quiet.on} onChange={(e) => e.target.value && save({ ...cfg, quiet: { ...cfg.quiet, to: e.target.value } })} />
        </span>
      </div>
      {hasMorning && <div className="setting">
        <span>Send the morning summary at<span className="muted setting-sub">What the queue did since the last one. Skipped when nothing ran.</span></span>
        <input type="time" aria-label="Morning summary time" value={cfg.morning} onChange={(e) => e.target.value && save({ ...cfg, morning: e.target.value })} />
      </div>}
    </section>}
  </>;
}
