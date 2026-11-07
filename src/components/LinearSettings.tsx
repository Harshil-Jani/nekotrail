import { useEffect, useState } from "react";
import { Lock } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errText } from "../api";
import { IS_MAC } from "../platform";
import { ask } from "./Confirm";
import { accountLine, looksLikeKey, type LinearStatus } from "../linear";

const KEYCHAIN = IS_MAC ? "your Keychain" : "your keyring";
const KEYS_PAGE = "https://linear.app/settings/account/security";

/** Settings, Integrations: connect Linear with a personal API key, kept in the keychain. */
export function LinearSettings() {
  const [status, setStatus] = useState<LinearStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState<"connect" | "test" | null>(null);
  const [note, setNote] = useState<{ text: string; bad?: boolean } | null>(null);
  useEffect(() => { api.linearStatus().then(setStatus).catch((e) => setError(errText(e))); }, []);

  const connect = async () => {
    setBusy("connect"); setNote(null);
    try {
      const s = await api.linearConnect(key);
      setStatus(s); setKey("");
      setNote({ text: `Connected as ${accountLine(s.account)}. The key is saved in ${KEYCHAIN}.` });
    } catch (e) { setNote({ text: errText(e), bad: true }); } finally { setBusy(null); }
  };
  const test = async () => {
    setBusy("test"); setNote(null);
    try { const s = await api.linearTest(); setStatus(s); setNote({ text: `Works. Signed in as ${accountLine(s.account)}.` }); }
    catch (e) { setNote({ text: errText(e), bad: true }); } finally { setBusy(null); }
  };
  const disconnect = async () => {
    if (!(await ask({ title: "Disconnect Linear?", body: `The key is deleted from ${KEYCHAIN}. Workspaces made from issues keep their branch, and their PRs still say which issue they fix.`, ok: "Disconnect", danger: true }))) return;
    try { setStatus(await api.linearDisconnect()); setNote(null); } catch (e) { setNote({ text: errText(e), bad: true }); }
  };
  const setMove = async (on: boolean) => {
    setStatus((s) => (s ? { ...s, move_started: on } : s));
    try { setStatus(await api.linearSetMove(on)); } catch (e) { setNote({ text: errText(e), bad: true }); }
  };

  if (!status) return error ? <section><h3>Linear</h3><p className="bad setting-note" role="alert">{error}</p></section> : null;
  return (
    <section className="linear-settings">
      <h3>Linear</h3>
      <p className="muted setting-note">Start a workspace from a Linear issue: it gets the issue's branch name, its title, and the issue as its first message.</p>
      <p className="alert-privacy setting-note">
        <Lock size={12} aria-hidden /><span>Your API key is kept in {KEYCHAIN}, never in NekoTrail's database, logs or exports. NekoTrail reads issues and, if you allow it below, moves one to In Progress. It never comments on Linear.</span>
      </p>
      {status.connected ? (
        <div className="alert-dest">
          <div className="alert-dest-head">
            <span className="linear-account">{accountLine(status.account) || "Connected"}</span>
            <span className="muted alert-dest-host" title={`The key is saved in ${KEYCHAIN}.`}><Lock size={11} aria-hidden />Key in {KEYCHAIN}</span>
            <span className="spacer" />
            <button onClick={test} disabled={busy !== null}>{busy === "test" ? "Testing…" : "Test"}</button>
            <button className="quiet" onClick={disconnect} disabled={busy !== null}>Disconnect</button>
          </div>
          <label className="setting">
            <span>Move the issue to In Progress when the agent starts<span className="muted setting-sub">Only an issue that hasn't started yet, once, on the workspace's first turn.</span></span>
            <input type="checkbox" className="switch" checked={status.move_started} onChange={(e) => setMove(e.target.checked)} />
          </label>
          {(note || status.last_error) && <p className={`alert-status ${(note ? note.bad : true) ? "bad" : "muted"}`} role="status">{note?.text ?? status.last_error}</p>}
        </div>
      ) : (
        <div className="alert-dest">
          <div className="alert-secret">
            <p className="muted setting-note">
              In Linear, open Settings, Security & access, and make a personal API key.{" "}
              <button className="link" onClick={() => openUrl(KEYS_PAGE).catch(() => {})}>Open that page</button>
            </p>
            <label className="alert-field">
              <span>API key</span>
              <input type="password" autoComplete="off" spellCheck={false} placeholder="lin_api_…" value={key}
                onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && looksLikeKey(key) && !busy) connect(); }} />
            </label>
            <div className="row end">
              <button className="primary" disabled={!looksLikeKey(key) || busy !== null} onClick={connect}>{busy === "connect" ? "Checking…" : "Connect"}</button>
            </div>
          </div>
          {note && <p className={`alert-status ${note.bad ? "bad" : "muted"}`} role="status">{note.text}</p>}
        </div>
      )}
    </section>
  );
}
