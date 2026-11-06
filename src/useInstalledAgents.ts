import { useEffect, useState } from "react";
import { api, type Agent } from "./api";

/** The last answer, so a picker that opens again starts with it instead of jumping. */
let cache: Agent[] | null = null;

/**
 * The agents whose CLI is on PATH (a PATH lookup, nothing is run), asked again each time a picker
 * mounts so a CLI installed meanwhile shows up. Null until the first answer. The pickers list
 * these first and fold the rest under "More agents".
 */
export function useInstalledAgents(): Agent[] | null {
  const [list, setList] = useState<Agent[] | null>(cache);
  useEffect(() => {
    let live = true;
    api.installedAgents().catch(() => cache ?? []).then((l) => { cache = l; if (live) setList(l); });
    return () => { live = false; };
  }, []);
  return list;
}
