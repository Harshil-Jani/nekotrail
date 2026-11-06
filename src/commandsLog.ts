import type { ChatEvent } from "./api";

/** One shell command an agent ran, from a chat's history. */
export type RanCommand = {
  command: string;
  output: string;
  /** Exited with an error (or was refused). */
  failed: boolean;
  /** No result yet: still running, or the turn ended before it finished. */
  pending: boolean;
};

/**
 * The shell commands in a chat's events, in the order they ran. Every agent's shell tool is
 * already named "Bash" in the chat (Codex's command_execution, Gemini's run_shell_command,
 * OpenCode's execute calls), so this needs nothing new from the backend.
 */
export function commandsFrom(events: ChatEvent[]): RanCommand[] {
  const out: RanCommand[] = [];
  const byId = new Map<string, number>();
  for (const ev of events) {
    if (ev.kind === "tool" && ev.name === "Bash" && ev.input.trim()) {
      byId.set(ev.id, out.length);
      out.push({ command: ev.input, output: "", failed: false, pending: true });
    } else if (ev.kind === "tool_result") {
      const i = byId.get(ev.id);
      if (i !== undefined) out[i] = { ...out[i], output: ev.output, failed: ev.is_error, pending: false };
    }
  }
  return out;
}

/** Plain text for the clipboard: one command per line, failures marked. */
export function commandsText(list: { chat: string; commands: RanCommand[] }[]): string {
  return list
    .filter((c) => c.commands.length)
    .map((c) => [`# ${c.chat}`, ...c.commands.map((x) => `${x.failed ? "(failed) " : ""}${x.command}`)].join("\n"))
    .join("\n\n");
}
