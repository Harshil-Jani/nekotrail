// Starting the nekotrail app when it isn't running. Only ever on your click.

import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

export type Launcher = { cmd: string; args: string[] };

/**
 * How to start nekotrail, in order of preference. macOS opens the app by its bundle id. Linux runs
 * the `nekotrail` terminal command (Settings, General installs it), which starts the app.
 */
export function launchers(platform: string, home: string, dataDir: string, pathEnv: string | undefined, exists: (p: string) => boolean = fs.existsSync): Launcher[] {
  if (platform === "darwin") return [{ cmd: "open", args: ["-b", "dev.nekotrail.app"] }];
  const dirs = (pathEnv ?? "").split(path.delimiter).filter((d) => path.isAbsolute(d));
  const candidates = [
    ...dirs.map((d) => path.join(d, "nekotrail")),
    path.join(home, ".local", "bin", "nekotrail"),
    "/usr/local/bin/nekotrail",
    path.join(dataDir, "bin", "nekotrail"),
  ];
  const seen = new Set<string>();
  return candidates.filter((c) => !seen.has(c) && seen.add(c) && exists(c)).map((cmd) => ({ cmd, args: [] }));
}

/** Run the first launcher that starts. Resolves false when none did. */
export async function launch(list: Launcher[]): Promise<boolean> {
  for (const l of list) {
    const ok = await new Promise<boolean>((resolve) => {
      const child = spawn(l.cmd, l.args, { detached: true, stdio: "ignore" });
      child.once("error", () => resolve(false));
      // `open` exits once the app is asked to start; the Linux command keeps running, so don't wait for it.
      if (l.cmd === "open") child.once("exit", (code) => resolve(code === 0));
      else child.once("spawn", () => { child.unref(); resolve(true); });
    });
    if (ok) return true;
  }
  return false;
}
