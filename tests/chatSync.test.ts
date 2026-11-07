// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { droppedFrom, forgetOutgoing, mergeLive, pendingRows, readOutbox, reconcileOutbox, refetchSize, restoreOutbox, reuseRows, settleOutbox, type Outgoing } from "../src/chatSync";

const ev = (seq: number | null, prev: number | null, text = `e${seq}`) => ({ event: { kind: "text", text }, seq, prev });
const texts = (r: { add: { text: string }[] }) => r.add.map((e) => e.text);

describe("live events merged with a loaded tail", () => {
  test("events the tail already has are dropped, the ones after it are added", () => {
    // Subscribed first, so 4 and 5 were buffered while the tail (up to 5) loaded.
    const r = mergeLive(5, [ev(4, 3), ev(5, 4), ev(6, 5), ev(7, 6)]);
    expect(texts(r)).toEqual(["e6", "e7"]);
    expect(r).toMatchObject({ last: 7, gap: false, rest: [] });
  });

  test("ids from other chats in between are fine: prev links the chat's own messages", () => {
    const r = mergeLive(10, [ev(15, 10), ev(42, 15)]);
    expect(texts(r)).toEqual(["e15", "e42"]);
    expect(r.last).toBe(42);
  });

  test("a missing event is a gap: merging stops and the rest waits for a fresh tail", () => {
    const r = mergeLive(5, [ev(6, 5), ev(9, 8), ev(10, 9)]);
    expect(texts(r)).toEqual(["e6"]);
    expect(r.gap).toBe(true);
    expect(r.last).toBe(6);
    expect(r.rest.map((x) => x.seq)).toEqual([9, 10]);
    // The fresh tail ends at 9: only 10 is new.
    const again = mergeLive(9, r.rest);
    expect(texts(again)).toEqual(["e10"]);
    expect(again.gap).toBe(false);
  });

  test("two events emitted the other way round are put back in order, not a gap", () => {
    const r = mergeLive(5, [ev(7, 6), ev(6, 5)]);
    expect(texts(r)).toEqual(["e6", "e7"]);
    expect(r.gap).toBe(false);
  });

  test("a repeat after the merge is dropped", () => {
    const a = mergeLive(5, [ev(6, 5)]);
    const b = mergeLive(a.last, [ev(6, 5), ev(7, 6)]);
    expect(texts(b)).toEqual(["e7"]);
  });

  test("an empty chat starts at 0", () => {
    expect(texts(mergeLive(0, [ev(1, 0)]))).toEqual(["e1"]);
  });

  test("an older backend without ids: events are added as they come", () => {
    const r = mergeLive(null, [ev(null, null, "a"), ev(null, null, "b")]);
    expect(texts(r)).toEqual(["a", "b"]);
    expect(r.last).toBeNull();
  });

  test("an unknown starting point takes the first id it sees", () => {
    const r = mergeLive(null, [ev(3, 2), ev(4, 3)]);
    expect(texts(r)).toEqual(["e3", "e4"]);
    expect(r.last).toBe(4);
  });

  test("a fresh tail keeps at least what is loaded", () => {
    expect(refetchSize(10, 600)).toBe(600);
    expect(refetchSize(2000, 600)).toBe(2050);
  });
});

const out = (text: string, state: Outgoing["state"], extra: Partial<Outgoing> = {}): Outgoing => ({ id: text, text, state, at: 0, base: 10, ...extra });

describe("messages on their way", () => {
  test("a message leaves the outbox when the chat shows it as yours", () => {
    const box = [out("fix it", "sending"), out("then test", "queued")];
    expect(settleOutbox(box, [{ kind: "text", text: "fix it" }], 10)).toBe(box);
    expect(settleOutbox(box, [{ kind: "user", text: "fix it\n" }], 10).map((o) => o.text)).toEqual(["then test"]);
  });

  test("an older message with the same words doesn't settle it", () => {
    // "continue" was event 4; it was sent again when the chat had 10 events. A fresh tail still holds the old one.
    const box = [out("continue", "sent")];
    const tail = [{ kind: "user", text: "continue" }, { kind: "text", text: "ok" }];
    expect(settleOutbox(box, tail, 4)).toHaveLength(1);
    expect(settleOutbox(box, [...tail, { kind: "user", text: "continue" }], 8)).toHaveLength(0);
  });

  test("two identical messages: one user event settles one", () => {
    const box = [out("go", "queued"), { ...out("go", "queued"), id: "go2" }];
    expect(settleOutbox(box, [{ kind: "user", text: "go" }], 12)).toHaveLength(1);
  });

  test("what the backend queued is marked queued", () => {
    const box = reconcileOutbox([out("next", "sent")], ["next"], 100);
    expect(box[0]).toMatchObject({ state: "queued" });
    expect(box[0].leftAt).toBeUndefined();
  });

  test("a queued message that leaves the queue without showing up is noticed, then dropped once idle", () => {
    let box = reconcileOutbox([out("next", "queued")], [], 1000);
    expect(box[0].leftAt).toBe(1000);
    // Still within the grace, or the agent is working (the next turn may be starting): not dropped.
    expect(droppedFrom(box, false, 2000)).toEqual([]);
    expect(droppedFrom(box, true, 9000)).toEqual([]);
    expect(droppedFrom(box, false, 9000).map((o) => o.text)).toEqual(["next"]);
    // Back in the queue: no longer counted as gone.
    box = reconcileOutbox(box, ["next"], 9500);
    expect(box[0].leftAt).toBeUndefined();
    expect(droppedFrom(box, false, 20000)).toEqual([]);
  });

  test("a message still being sent is never counted as dropped", () => {
    const box = reconcileOutbox([out("hi", "sending")], [], 0);
    expect(droppedFrom(box, false, 60_000)).toEqual([]);
  });

  test("unchanged outbox keeps its identity (no re-render)", () => {
    const box = [out("next", "queued")];
    expect(reconcileOutbox(box, ["next"], 0)).toBe(box);
  });

  test("the end of the chat shows the backend queue, then what hasn't reached it", () => {
    const rows = pendingRows([out("a", "queued"), out("b", "sending")], ["a", "from the CLI"]);
    expect(rows.map((r) => [r.text, r.state, r.index])).toEqual([["a", "queued", 0], ["from the CLI", "queued", 1], ["b", "sending", null]]);
  });

  test("removing from the queue forgets one copy", () => {
    expect(forgetOutgoing([out("a", "queued"), out("b", "queued")], "a").map((o) => o.text)).toEqual(["b"]);
    expect(forgetOutgoing([out("a", "queued")], "zzz")).toHaveLength(1);
  });

  test("a saved outbox survives bad JSON and drops what the chat already shows", () => {
    expect(readOutbox("{nope")).toEqual([]);
    expect(readOutbox(null)).toEqual([]);
    const saved = readOutbox(JSON.stringify([out("sent before quit", "queued"), out("lost", "queued"), { junk: 1 }]));
    const back = restoreOutbox(saved, [{ kind: "user", text: "sent before quit" }], 11);
    expect(back.map((o) => [o.text, o.state])).toEqual([["lost", "sent"]]);
  });
});

describe("rows keep their identity when unchanged", () => {
  test("unchanged rows are the same objects, so memoized rows skip rendering", () => {
    const done = { kind: "done" };
    const prev = [{ type: "text", text: "a" }, { type: "tool", name: "Bash", files: ["x"], pending: true }, { type: "done", ev: done }];
    const next = [{ type: "text", text: "a" }, { type: "tool", name: "Bash", files: ["x"], pending: false }, { type: "done", ev: done }, { type: "text", text: "b" }];
    const out = reuseRows<object>(prev, next);
    expect(out[0]).toBe(prev[0]);
    expect(out[1]).toBe(next[1]);
    expect(out[2]).toBe(prev[2]);
    expect(out[3]).toBe(next[3]);
  });

  test("nothing changed: the same array back", () => {
    const prev = [{ type: "text", text: "a" }];
    expect(reuseRows(prev, [{ type: "text", text: "a" }])).toBe(prev);
  });
});
