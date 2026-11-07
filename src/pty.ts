/**
 * Terminal plumbing kept apart from the components so tests can read it: the words the Terminal tab
 * writes into the terminal itself (xterm text, so \r\n line ends and ANSI colour), and one shared
 * listener that hands each terminal only its own output.
 */

/** Shown in place of a shell that could not start, so the terminal never looks open but ignores typing. */
export function startFailedText(reason: string): string {
  const why = reason.trim().replace(/\r?\n/g, "\r\n") || "The shell didn't start.";
  return `\x1b[31mCouldn't start a terminal here.\x1b[0m\r\n${why}\r\n\x1b[2mOpen a new tab with + to try again.\x1b[0m\r\n`;
}

/**
 * Output that arrived while the scrollback was being fetched: the first chunks are already at the
 * end of the scrollback (printed before it was read); return only the ones after it.
 */
export function afterScrollback(scrollback: string, chunks: string[]): string[] {
  if (!scrollback || !chunks.length) return chunks;
  for (let k = chunks.length; k > 0; k--) {
    if (scrollback.endsWith(chunks.slice(0, k).join(""))) return chunks.slice(k);
  }
  return chunks;
}

type Unlisten = () => void;
type ListenFn = <T>(event: string, cb: (e: { payload: T }) => void) => Promise<Unlisten>;

/**
 * One `pty` and one `pty-exit` listener for the whole window, routing by terminal id, so a busy
 * terminal's output isn't parsed by every other terminal. It also remembers which terminals ended,
 * even when no view was open, so reopening one starts a fresh shell instead of showing a dead one.
 */
export function makePtyBus(listen: ListenFn) {
  const data = new Map<string, Set<(d: string) => void>>();
  const exit = new Map<string, Set<() => void>>();
  const taps = new Set<(id: string, d: string) => void>();
  const ended = new Set<string>();
  let ready: Promise<void> | null = null;
  const start = () =>
    (ready ??= Promise.all([
      listen<{ id: string; data: string }>("pty", ({ payload }) => {
        data.get(payload.id)?.forEach((f) => f(payload.data));
        taps.forEach((f) => f(payload.id, payload.data));
      }),
      listen<{ id: string }>("pty-exit", ({ payload }) => {
        ended.add(payload.id);
        exit.get(payload.id)?.forEach((f) => f());
      }),
    ]).then(
      () => {},
      (err) => { ready = null; throw err; }, // try again next time
    ));
  const add = <F>(m: Map<string, Set<F>>, id: string, f: F) => {
    const s = m.get(id) ?? new Set<F>();
    s.add(f);
    m.set(id, s);
    return () => { s.delete(f); if (!s.size) m.delete(id); };
  };
  return {
    /** Resolves once `id`'s output reaches `onData`: start or attach the process only after this. */
    async subscribe(id: string, onData: (d: string) => void, onExit: () => void): Promise<Unlisten> {
      const offData = add(data, id, onData);
      const offExit = add(exit, id, onExit);
      try {
        await start();
      } catch (err) {
        offData(); offExit();
        throw err;
      }
      return () => { offData(); offExit(); };
    },
    /** Every terminal's output (the run script's URL sniffer). */
    async tap(f: (id: string, d: string) => void): Promise<Unlisten> {
      taps.add(f);
      await start();
      return () => { taps.delete(f); };
    },
    /** Resolves when terminal `id` ends, or after `ms`; the listener is gone either way. */
    waitForExit(id: string, ms: number): Promise<void> {
      return new Promise((resolve) => {
        let off: Unlisten = () => {};
        const t = setTimeout(() => { off(); resolve(); }, ms);
        off = add(exit, id, () => { clearTimeout(t); off(); resolve(); });
        start().catch(() => {});
      });
    },
    /** Did `id` end since it was last started here? Clears the mark. */
    takeEnded(id: string): boolean {
      return ended.delete(id);
    },
    /** Listeners per terminal id (tests). */
    listeners(id: string): number {
      return (data.get(id)?.size ?? 0) + (exit.get(id)?.size ?? 0);
    },
  };
}

export type PtyBus = ReturnType<typeof makePtyBus>;
