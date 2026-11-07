// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { applyRunning, DoneWatchdog, Latest, sameSet } from "../src/runState";

describe("snapshots that answer out of order", () => {
  test("only the newest request's answer counts", () => {
    const l = new Latest();
    const a = l.begin();
    const b = l.begin();
    expect(l.isCurrent(a)).toBe(false); // answered last, but asked first: ignored
    expect(l.isCurrent(b)).toBe(true);
  });

  test("an event voids snapshots already in flight", () => {
    const l = new Latest();
    const a = l.begin();
    l.invalidate(); // running:false arrived while `a` was loading
    expect(l.isCurrent(a)).toBe(false);
    expect(l.isCurrent(l.begin())).toBe(true);
  });
});

const sets = (s: string[], w: string[]) => ({ sessions: new Set(s), workspaces: new Set(w) });

describe("running events applied straight away", () => {
  test("turn start and end move the chat and its workspace", () => {
    const wsOf = new Map<string, string>();
    let st = applyRunning(sets([], []), { session_id: "c1", running: true, workspace_id: "w1" }, wsOf);
    expect([...st.sessions, ...st.workspaces]).toEqual(["c1", "w1"]);
    st = applyRunning(st, { session_id: "c1", running: false, workspace_id: "w1" }, wsOf);
    expect(st.sessions.size + st.workspaces.size).toBe(0);
  });

  test("a workspace keeps working while another of its chats does", () => {
    const wsOf = new Map<string, string>();
    let st = applyRunning(sets([], []), { session_id: "c1", running: true, workspace_id: "w1" }, wsOf);
    st = applyRunning(st, { session_id: "c2", running: true, workspace_id: "w1" }, wsOf);
    st = applyRunning(st, { session_id: "c1", running: false, workspace_id: "w1" }, wsOf);
    expect([...st.workspaces]).toEqual(["w1"]);
    st = applyRunning(st, { session_id: "c2", running: false }, wsOf); // workspace remembered
    expect(st.workspaces.size).toBe(0);
  });

  test("turn end then the queued turn's start, in that order, leaves it working", () => {
    const wsOf = new Map<string, string>();
    let st = sets(["c1"], ["w1"]);
    st = applyRunning(st, { session_id: "c1", running: false, workspace_id: "w1" }, wsOf);
    st = applyRunning(st, { session_id: "c1", running: true, workspace_id: "w1" }, wsOf);
    expect(st.sessions.has("c1") && st.workspaces.has("w1")).toBe(true);
  });

  test("an event without a chat changes nothing", () => {
    const st = sets(["c1"], ["w1"]);
    expect(applyRunning(st, { running: false }, new Map())).toBe(st);
  });

  test("sameSet", () => {
    expect(sameSet(new Set([1, 2]), new Set([2, 1]))).toBe(true);
    expect(sameSet(new Set([1]), new Set([2]))).toBe(false);
  });
});

/** Timers you move by hand. */
function fakeClock() {
  let now = 0;
  const due = new Map<number, { at: number; f: () => void }>();
  let id = 0;
  return {
    set: (f: () => void, ms: number) => { due.set(++id, { at: now + ms, f }); return id; },
    clear: (t: unknown) => { due.delete(t as number); },
    advance(ms: number) {
      now += ms;
      for (;;) {
        const next = [...due.entries()].filter(([, d]) => d.at <= now).sort((a, b) => a[1].at - b[1].at)[0];
        if (!next) break;
        due.delete(next[0]);
        next[1].f();
      }
    },
    pending: () => due.size,
  };
}

describe("Done without a stop", () => {
  test("asks for a fresh snapshot while the chat still shows as working, with backoff", () => {
    const clock = fakeClock();
    const asked: string[] = [];
    let running = true;
    const w = new DoneWatchdog(() => running, (s) => asked.push(s), [10, 30], clock);
    w.done("c1");
    clock.advance(9);
    expect(asked).toEqual([]);
    clock.advance(1);
    expect(asked).toEqual(["c1"]);
    clock.advance(30);
    expect(asked).toEqual(["c1", "c1"]);
    clock.advance(1000);
    expect(asked).toHaveLength(2); // gives up after the last delay
    running = false;
  });

  test("the stop arriving cancels it", () => {
    const clock = fakeClock();
    const asked: string[] = [];
    const w = new DoneWatchdog(() => true, (s) => asked.push(s), [10], clock);
    w.done("c1");
    w.ended("c1");
    clock.advance(100);
    expect(asked).toEqual([]);
    expect(clock.pending()).toBe(0);
  });

  test("already stopped by the time it looks: nothing to do", () => {
    const clock = fakeClock();
    const asked: string[] = [];
    const w = new DoneWatchdog(() => false, (s) => asked.push(s), [10, 20], clock);
    w.done("c1");
    clock.advance(100);
    expect(asked).toEqual([]);
  });

  test("dispose clears every timer", () => {
    const clock = fakeClock();
    const w = new DoneWatchdog(() => true, () => {}, [10], clock);
    w.done("a");
    w.done("b");
    w.dispose();
    expect(clock.pending()).toBe(0);
  });
});
