import type { PortRow, WsPorts } from "./api";

/** "web · PORT" for a named port, "PORT" for the main one, "NEKOTRAIL_PORT_3" for the rest. */
export function portLabel(row: PortRow): string {
  if (row.outside) return "Not this workspace's port";
  // NEKOTRAIL_PORT_n is always there; show the friendlier names when the port has any.
  const own = row.vars.filter((v) => !v.startsWith("NEKOTRAIL_PORT"));
  const vars = own.length ? own : row.vars;
  return [row.name, ...vars].filter(Boolean).join(" · ");
}

/** Who listens, in a few words: "Nothing listening", "node (pid 412)", "node, from Fix login". */
export function holderText(row: PortRow, wsId: string, label: (id: string) => string): string {
  const h = row.holder;
  if (!h) return "Nothing listening";
  const who = `${h.command} (pid ${h.pid})`;
  if (!h.workspace_id) return `${who}, not started here`;
  if (h.workspace_id !== wsId) return `${who}, from ${label(h.workspace_id)}`;
  return h.how === "folder" ? `${who}, in this folder` : who;
}

/** The selected workspace first, then the rest by their first port. */
export function orderWorkspaces(list: WsPorts[], selected: string | null): WsPorts[] {
  return [...list].sort((a, b) => Number(b.id === selected) - Number(a.id === selected) || a.base - b.base);
}

/** The ports a block spans, for its heading: "4110 to 4119". */
export function blockRange(ws: Pick<WsPorts, "base">): string {
  return `${ws.base} to ${ws.base + 9}`;
}

/** The ports in the block that no row shows: free and unnamed, summed up in one line. */
export function quietPorts(ws: WsPorts): number {
  const shown = ws.rows.filter((r) => !r.outside).length;
  return Math.max(0, 10 - shown);
}
