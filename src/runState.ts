/**
 * Which agents are working, kept right when snapshots answer out of order (tests/runState.test.ts).
 *
 * The backend sends a `running` event when a turn starts or ends, and the window also reloads a full
 * snapshot. Two turns a moment apart (a turn ends, the queued one starts) ask for two snapshots,
 * and the older answer can arrive last. So: the event is applied straight away, every event voids the
 * snapshots already asked for, and only the answer to the newest question is used.
 */

/** Hands out a number per request; only the newest request's answer counts. */
export class Latest {
  private n = 0;
  /** A new request: returns its number. */
  begin(): number { return ++this.n; }
  /** Something newer than every request in flight happened (an event): their answers are stale. */
  invalidate(): void { this.n++; }
  /** Is this the newest request, with nothing newer since? */
  isCurrent(token: number): boolean { return token === this.n; }
}

export type RunningEvent = { session_id?: string; running: boolean; workspace_id?: string };
export type RunningSets = { sessions: Set<string>; workspaces: Set<string> };

/**
 * Apply one `running` event to the working sets. A workspace stops showing as working when its last
 * known working chat ends. `wsOf` remembers each chat's workspace (learned from events and kept up to date here).
 */
export function applyRunning(state: RunningSets, ev: RunningEvent, wsOf: Map<string, string>): RunningSets {
  const sid = ev.session_id;
  if (!sid) return state;
  if (ev.workspace_id) wsOf.set(sid, ev.workspace_id);
  const sessions = new Set(state.sessions);
  const workspaces = new Set(state.workspaces);
  if (ev.running) {
    sessions.add(sid);
    if (ev.workspace_id) workspaces.add(ev.workspace_id);
  } else {
    sessions.delete(sid);
    const ws = ev.workspace_id ?? wsOf.get(sid);
    if (ws && ![...sessions].some((s) => wsOf.get(s) === ws)) workspaces.delete(ws);
  }
  return { sessions, workspaces };
}

/** Same members: lets callers keep the old Set and skip a render. */
export function sameSet<T>(a: Set<T>, b: Set<T>): boolean {
  if (a.size !== b.size) return false;
  for (const x of a) if (!b.has(x)) return false;
  return true;
}

type Timers = { set: (f: () => void, ms: number) => unknown; clear: (t: unknown) => void };
const realTimers: Timers = { set: (f, ms) => setTimeout(f, ms), clear: (t) => clearTimeout(t as ReturnType<typeof setTimeout>) };

/**
 * A turn reported Done but the window never heard it stop (a lost `running:false`, or a snapshot
 * that raced it): after a while, ask for a fresh snapshot, and again a bit later, while the chat
 * still shows as working.
 */
export class DoneWatchdog {
  private timers = new Map<string, unknown>();
  constructor(
    private isRunning: (sid: string) => boolean,
    private onStale: (sid: string) => void,
    private delays: number[] = [10_000, 30_000, 60_000],
    private clock: Timers = realTimers,
  ) {}

  /** The chat's turn reported Done. */
  done(sid: string): void {
    this.ended(sid);
    this.arm(sid, 0);
  }

  /** The chat stopped (or started a new turn): nothing to watch. */
  ended(sid: string): void {
    const t = this.timers.get(sid);
    if (t !== undefined) this.clock.clear(t);
    this.timers.delete(sid);
  }

  dispose(): void {
    for (const t of this.timers.values()) this.clock.clear(t);
    this.timers.clear();
  }

  private arm(sid: string, i: number): void {
    if (i >= this.delays.length) return;
    this.timers.set(sid, this.clock.set(() => {
      this.timers.delete(sid);
      if (!this.isRunning(sid)) return;
      this.onStale(sid);
      this.arm(sid, i + 1);
    }, this.delays[i]));
  }
}
