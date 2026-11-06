// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { blockRange, holderText, orderWorkspaces, portLabel, quietPorts } from "../src/ports";
import type { PortRow, WsPorts } from "../src/api";

const row = (p: Partial<PortRow>): PortRow => ({ port: 4110, vars: ["NEKOTRAIL_PORT"], name: null, outside: false, holder: null, ...p });
const ws = (id: string, base: number, rows: PortRow[] = []): WsPorts => ({ id, base, main: base, rows, problems: [] });

describe("ports popover", () => {
  test("labels prefer the names you use", () => {
    expect(portLabel(row({ vars: ["NEKOTRAIL_PORT", "PORT"] }))).toBe("PORT");
    expect(portLabel(row({ vars: ["NEKOTRAIL_PORT_1", "API_PORT"], name: "api" }))).toBe("api · API_PORT");
    expect(portLabel(row({ vars: ["NEKOTRAIL_PORT_3"] }))).toBe("NEKOTRAIL_PORT_3");
    expect(portLabel(row({ vars: [], outside: true }))).toBe("Not this workspace's port");
  });

  test("says who listens and where it came from", () => {
    const label = (id: string) => (id === "b" ? "Fix login" : id);
    expect(holderText(row({}), "a", label)).toBe("Nothing listening");
    const h = { pid: 412, command: "node", workspace_id: "a", how: "tree" as const, can_free: true };
    expect(holderText(row({ holder: h }), "a", label)).toBe("node (pid 412)");
    expect(holderText(row({ holder: { ...h, how: "folder", can_free: false } }), "a", label)).toBe("node (pid 412), in this folder");
    expect(holderText(row({ holder: { ...h, workspace_id: "b", can_free: false } }), "a", label)).toBe("node (pid 412), from Fix login");
    expect(holderText(row({ holder: { ...h, workspace_id: null, how: null, can_free: false } }), "a", label)).toBe("node (pid 412), not started here");
  });

  test("the open workspace comes first, then by port", () => {
    const list = [ws("c", 4120), ws("a", 4100), ws("b", 4110)];
    expect(orderWorkspaces(list, "b").map((w) => w.id)).toEqual(["b", "a", "c"]);
    expect(orderWorkspaces(list, null).map((w) => w.id)).toEqual(["a", "b", "c"]);
    // A multi-repo task's other repos follow the open one.
    expect(orderWorkspaces(list, "b", ["b", "c"]).map((w) => w.id)).toEqual(["b", "c", "a"]);
  });

  test("block range and the free ports left unlisted", () => {
    expect(blockRange({ base: 4110 })).toBe("4110 to 4119");
    expect(quietPorts(ws("a", 4100, [row({}), row({ port: 4101 }), row({ port: 3000, outside: true })]))).toBe(8);
  });
});
