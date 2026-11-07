// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { afterScrollback, makePtyBus } from "../src/pty";

describe("output that arrives while reattaching", () => {
  test("chunks already at the end of the scrollback are not written twice", () => {
    expect(afterScrollback("$ ls\r\na b\r\n$ ", ["a b\r\n", "$ ", "new"])).toEqual(["new"]);
  });
  test("a new shell (empty scrollback) writes everything", () => {
    expect(afterScrollback("", ["$ "])).toEqual(["$ "]);
  });
  test("nothing in common: everything is new", () => {
    expect(afterScrollback("old", ["x", "y"])).toEqual(["x", "y"]);
  });
});

/** A stand-in for Tauri's listen: `emit` calls the listeners; `failNext` makes the next listen reject. */
function fakeListen() {
  const subs = new Map<string, ((e: { payload: unknown }) => void)[]>();
  let calls = 0, fail = false;
  const listen = async <T,>(event: string, cb: (e: { payload: T }) => void) => {
    calls++;
    if (fail) { fail = false; throw new Error("no window"); }
    subs.set(event, [...(subs.get(event) ?? []), cb as (e: { payload: unknown }) => void]);
    return () => {};
  };
  return {
    listen,
    emit: (event: string, payload: unknown) => subs.get(event)?.forEach((f) => f({ payload })),
    calls: () => calls,
    failNext: () => { fail = true; },
  };
}

describe("one terminal listener for the window", () => {
  test("each terminal gets only its own output, and listening is ready before subscribe resolves", async () => {
    const f = fakeListen();
    const bus = makePtyBus(f.listen);
    const a: string[] = [], b: string[] = [];
    await bus.subscribe("w:term", (d) => a.push(d), () => {});
    await bus.subscribe("w:other", (d) => b.push(d), () => {});
    f.emit("pty", { id: "w:term", data: "$ " });
    expect(a).toEqual(["$ "]);
    expect(b).toEqual([]);
    expect(f.calls()).toBe(2); // pty and pty-exit, once for the window
  });

  test("an exit is remembered even with no view open, so reopening starts fresh", async () => {
    const f = fakeListen();
    const bus = makePtyBus(f.listen);
    const off = await bus.subscribe("w:term", () => {}, () => {});
    off();
    expect(bus.listeners("w:term")).toBe(0);
    f.emit("pty-exit", { id: "w:term" });
    expect(bus.takeEnded("w:term")).toBe(true);
    expect(bus.takeEnded("w:term")).toBe(false);
  });

  test("the exit reaches the open terminal", async () => {
    const f = fakeListen();
    const bus = makePtyBus(f.listen);
    let exited = 0;
    await bus.subscribe("w:term", () => {}, () => exited++);
    f.emit("pty-exit", { id: "w:term" });
    expect(exited).toBe(1);
  });

  test("waitForExit removes its listener on exit and on timeout", async () => {
    const f = fakeListen();
    const bus = makePtyBus(f.listen);
    const done = bus.waitForExit("w:run", 60_000);
    expect(bus.listeners("w:run")).toBe(1);
    await Promise.resolve();
    f.emit("pty-exit", { id: "w:run" });
    await done;
    expect(bus.listeners("w:run")).toBe(0);
    await bus.waitForExit("w:setup", 1);
    expect(bus.listeners("w:setup")).toBe(0);
  });

  test("a failed listen is tried again by the next terminal", async () => {
    const f = fakeListen();
    const bus = makePtyBus(f.listen);
    f.failNext();
    await expect(bus.subscribe("w:term", () => {}, () => {})).rejects.toThrow("no window");
    expect(bus.listeners("w:term")).toBe(0);
    const got: string[] = [];
    await bus.subscribe("w:term", (d) => got.push(d), () => {});
    f.emit("pty", { id: "w:term", data: "ok" });
    expect(got).toEqual(["ok"]);
  });

  test("taps see every terminal's output", async () => {
    const f = fakeListen();
    const bus = makePtyBus(f.listen);
    const seen: string[] = [];
    await bus.tap((id, d) => seen.push(`${id}=${d}`));
    f.emit("pty", { id: "w:run", data: "http://localhost:3000" });
    expect(seen).toEqual(["w:run=http://localhost:3000"]);
  });
});
