/**
 * Keeping an open chat in step with the backend: live `agent` events merged with a loaded tail
 * (no event lost, none shown twice), and messages you sent that the backend has not shown yet.
 * Pure functions, so the ordering rules have tests (tests/chatSync.test.ts).
 */

/** A chat event as it arrives live: the event, its message id (`seq`) and the chat's id before it (`prev`). */
export type LiveEvent<E> = { event: E; seq?: number | null; prev?: number | null };

export type MergeResult<E> = {
  /** Events to append to the chat, in order. */
  add: E[];
  /** The newest message id the chat now shows (null when unknown: a backend without ids). */
  last: number | null;
  /** Events not merged: one before them is missing, so the tail has to be loaded again first. */
  rest: LiveEvent<E>[];
  /** An event is missing between what the chat shows and what arrived. */
  gap: boolean;
};

/**
 * Merge live events into a chat whose newest message id is `last`.
 * - Already shown (`seq <= last`): dropped, so a tail that overlaps the buffer never shows a row twice.
 * - Next in line (`prev === last`): appended.
 * - Anything else: a gap. Merging stops there and the rest waits for a fresh tail.
 * Events without ids (an older backend) are appended as they come, like before.
 */
export function mergeLive<E>(last: number | null, incoming: LiveEvent<E>[]): MergeResult<E> {
  // Two threads can store events a moment apart and emit them the other way round: put them back in order.
  const list = incoming.every((x) => typeof x.seq === "number") ? [...incoming].sort((a, b) => a.seq! - b.seq!) : incoming;
  const add: E[] = [];
  let at = last;
  for (let i = 0; i < list.length; i++) {
    const x = list[i];
    if (typeof x.seq !== "number") { add.push(x.event); continue; }
    if (at === null) { add.push(x.event); at = x.seq; continue; }
    if (x.seq <= at) continue;
    if (typeof x.prev !== "number" || x.prev === at) { add.push(x.event); at = x.seq; continue; }
    return { add, last: at, rest: list.slice(i), gap: true };
  }
  return { add, last: at, rest: [], gap: false };
}

/** How many events to load when a gap forces a fresh tail: at least what the chat shows now, plus room. */
export function refetchSize(loaded: number, min: number): number {
  return Math.max(min, loaded + 50);
}

// ---- messages on their way ----

/**
 * A message you sent that the chat doesn't show as yours yet.
 * - "sending": Send was pressed; the backend hasn't answered.
 * - "sent": the backend took it; waiting for it to show in the chat or the queue.
 * - "queued": in the backend's queue, sent when the agent's turn ends.
 * `base` is how many events the chat had when it was sent: only a `user` event at or after it can be
 * this message (an earlier "continue" is not). `leftAt` is when it was last seen nowhere.
 */
export type Outgoing = { id: string; text: string; state: "sending" | "sent" | "queued"; at: number; base: number; leftAt?: number };

/**
 * Remove the messages that now show in the chat as yours. `events` are chat events whose first one
 * is the chat's event number `first`; each `user` event settles at most one message sent before it.
 */
export function settleOutbox<E extends { kind: string; text?: string }>(outbox: Outgoing[], events: E[], first: number): Outgoing[] {
  if (!outbox.length) return outbox;
  const out = [...outbox];
  events.forEach((e, k) => {
    if (e.kind !== "user") return;
    const t = (e.text ?? "").trim();
    const i = out.findIndex((o) => o.text.trim() === t && first + k >= o.base);
    if (i >= 0) out.splice(i, 1);
  });
  return out.length === outbox.length ? outbox : out;
}

/**
 * Bring the outbox in line with the backend's queue: what's in the queue is "queued"; a sent message
 * that is nowhere gets `leftAt`, so a dropped one can be noticed (see `droppedFrom`).
 */
export function reconcileOutbox(outbox: Outgoing[], queue: string[], now: number): Outgoing[] {
  const left = new Map<string, number>();
  for (const q of queue) left.set(q.trim(), (left.get(q.trim()) ?? 0) + 1);
  let changed = false;
  const out = outbox.map((o) => {
    if (o.state === "sending") return o;
    const n = left.get(o.text.trim()) ?? 0;
    if (n > 0) {
      left.set(o.text.trim(), n - 1);
      if (o.state === "queued" && o.leftAt === undefined) return o;
      changed = true;
      return { id: o.id, text: o.text, state: "queued" as const, at: o.at, base: o.base };
    }
    if (o.leftAt !== undefined) return o;
    changed = true;
    return { ...o, leftAt: now };
  });
  return changed ? out : outbox;
}

/** How long a sent message may be nowhere, with the agent idle, before it counts as dropped. */
export const DROP_GRACE_MS = 4000;

/**
 * Messages the backend took but never ran: out of the queue, not in the chat, and the agent is idle.
 * They go back into the message box so nothing you wrote is lost.
 */
export function droppedFrom(outbox: Outgoing[], busy: boolean, now: number, grace = DROP_GRACE_MS): Outgoing[] {
  if (busy) return [];
  return outbox.filter((o) => o.state !== "sending" && o.leftAt !== undefined && now - o.leftAt >= grace);
}

/** One message waiting at the end of the chat. `index` is its place in the backend queue (for Edit and Remove). */
export type PendingRow = { key: string; text: string; state: "sending" | "queued"; index: number | null };

/**
 * What to show after the last message: everything in the backend's queue (queued from anywhere),
 * then your messages the backend hasn't shown anywhere yet.
 */
export function pendingRows(outbox: Outgoing[], queue: string[]): PendingRow[] {
  const rows: PendingRow[] = queue.map((text, index) => ({ key: `q${index}`, text, state: "queued", index }));
  const inQueue = new Map<string, number>();
  for (const q of queue) inQueue.set(q.trim(), (inQueue.get(q.trim()) ?? 0) + 1);
  for (const o of outbox) {
    const n = inQueue.get(o.text.trim()) ?? 0;
    if (n > 0) { inQueue.set(o.text.trim(), n - 1); continue; }
    rows.push({ key: o.id, text: o.text, state: o.state === "queued" ? "queued" : "sending", index: null });
  }
  return rows;
}

/** Remove the first message with this text (you removed or edited it in the queue). */
export function forgetOutgoing(outbox: Outgoing[], text: string): Outgoing[] {
  const i = outbox.findIndex((o) => o.text.trim() === text.trim());
  return i < 0 ? outbox : [...outbox.slice(0, i), ...outbox.slice(i + 1)];
}

/** Read a chat's saved outbox; anything unreadable is an empty one. */
export function readOutbox(raw: string | null): Outgoing[] {
  try {
    const v = JSON.parse(raw ?? "[]");
    return Array.isArray(v) ? v.filter((o) => o && typeof o.text === "string" && typeof o.id === "string" && typeof o.base === "number") : [];
  } catch {
    return [];
  }
}

/**
 * After a restart: saved messages already in the chat (a `user` event after they were sent) went
 * through; the rest stay, as "sent", so the queue check decides whether they were dropped.
 */
export function restoreOutbox<E extends { kind: string; text?: string }>(saved: Outgoing[], tail: E[], first: number): Outgoing[] {
  return settleOutbox(saved, tail, first).map((o) => ({ id: o.id, text: o.text, state: "sent", at: o.at, base: o.base }));
}

// ---- rows ----

/** Two rows with the same fields (arrays compared item by item) are the same row. */
function sameRow(a: object, b: object): boolean {
  const ka = Object.keys(a), kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    const x = (a as Record<string, unknown>)[k], y = (b as Record<string, unknown>)[k];
    if (x === y) continue;
    if (Array.isArray(x) && Array.isArray(y) && x.length === y.length && x.every((v, i) => v === y[i])) continue;
    return false;
  }
  return true;
}

/**
 * Keep the previous object for every row that didn't change, so memoized rows skip rendering and a
 * streamed event only redraws the rows it touched.
 */
export function reuseRows<T extends object>(prev: T[], next: T[]): T[] {
  let same = prev.length === next.length;
  const out = next.map((r, i) => {
    const p = prev[i];
    if (p !== undefined && (p === r || sameRow(p, r))) return p;
    same = false;
    return r;
  });
  return same ? prev : out;
}
