import { ask } from "./components/Confirm";

const KEY = "nekotrail.fullAutoOk";

/**
 * The first time Full auto is picked anywhere (chat, new workspace, default in Settings), say what
 * it means and ask once. Resolves true to go ahead; after a yes it never asks again.
 */
export async function okFullAuto(mode: string): Promise<boolean> {
  if (mode !== "bypassPermissions" || localStorage.getItem(KEY)) return true;
  const ok = await ask({
    title: "Use Full auto?",
    body: [
      "The agent runs commands, installs packages and edits files without stopping to ask you.",
      "It stays in this workspace's own folder and branch. Every message saves a checkpoint you can rewind to.",
      "Protected files from Repo settings still apply.",
      "You'll only be asked this once.",
    ].join("\n"),
    ok: "Use Full auto",
  });
  if (ok) localStorage.setItem(KEY, new Date().toISOString());
  return ok;
}
