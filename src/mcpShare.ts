import type { McpHas, McpRow, McpSecret, SecretChoices } from "./api";

export type LiveServer = { name: string; target: string; ok: boolean; status: string };

/** `claude mcp list` prints "name: target - ✔ Connected"; Codex and Gemini print their own formats. Parse what we can, show the rest raw. */
export function parseMcp(text: string): LiveServer[] {
  const out: LiveServer[] = [];
  for (const l of text.split("\n")) {
    const m = /^(.+?):\s+(.+?)\s+-\s+([✔✓✗✘!⚠])\s*(.+)$/.exec(l.trim());
    if (m) out.push({ name: m[1], target: m[2], ok: m[3] === "✔" || m[3] === "✓", status: m[4] });
  }
  return out;
}

/** Where a server lives, in plain words. */
export const scopeText = (s: McpHas["scope"]) =>
  s === "user" ? "all your folders" : s === "local" ? "this folder only" : "this repo's settings";

/** "codex mcp list" etc. may print a name with or without its scope; match on the name. */
export function liveFor(row: McpRow, live: LiveServer[]): LiveServer | undefined {
  return live.find((l) => l.name === row.name || l.name.endsWith(`:${row.name}`) || l.name.startsWith(`${row.name} `));
}

/** Every secret has an answer the backend will take (arguments and urls can only be copied). */
export function secretsAnswered(secrets: McpSecret[], choices: SecretChoices): boolean {
  return secrets.every((s) => choices[s.id] === "copy" || (s.can_ref && choices[s.id] === "ref"));
}

/** What the secret is, for the question next to it. */
export function secretLabel(s: McpSecret): string {
  if (s.kind === "env") return `${s.key} (environment variable)`;
  if (s.kind === "header") return `${s.key} header`;
  if (s.kind === "url") return "The server address";
  return `Command line: ${s.key}`;
}

/** "a", "a and b", "a, b and c". */
const andList = (xs: string[]) => (xs.length < 2 ? xs.join("") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);

/** Where OpenCode finds skills, in plain words. `dirs` is its own folder first, then the shared ones it reads. */
export function opencodeSkillsNote(dirs: string[]): string {
  const [own, ...shared] = dirs;
  if (!own) return "";
  if (shared.length === 0) return `OpenCode reads skills only from ${own}, because it's set to skip the shared folders.`;
  return `OpenCode already reads skills in ${andList(shared)}.`;
}
