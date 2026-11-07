/**
 * Words the Terminal tab writes into the terminal itself (xterm text, so \r\n line ends and ANSI
 * colour). Kept apart from the component so tests can read them.
 */

/** Shown in place of a shell that could not start, so the terminal never looks open but ignores typing. */
export function startFailedText(reason: string): string {
  const why = reason.trim().replace(/\r?\n/g, "\r\n") || "The shell didn't start.";
  return `\x1b[31mCouldn't start a terminal here.\x1b[0m\r\n${why}\r\n\x1b[2mOpen a new tab with + to try again.\x1b[0m\r\n`;
}
