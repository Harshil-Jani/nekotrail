/**
 * Finds things in a session export that look like secrets (tokens, keys, passwords, .env values,
 * email addresses) so the export can hide them. Everything runs locally on the text; nothing is sent
 * anywhere. Detection leans towards known formats: a generic "long random string" rule would hide
 * git SHAs and UUIDs, which a report needs.
 */

/** One place in a text that looks like a secret. `value` is what gets hidden. */
export type SecretSpan = { start: number; end: number; kind: string; value: string };

/** Where a text came from: `env` for output read from a .env file, where every value counts. */
export type ScanContext = { env?: boolean };

type Detector = { kind: string; re: RegExp; group?: number; ok?: (value: string, match: RegExpExecArray) => boolean };

/** Values that are clearly not a real secret: variables, templates, placeholders, numbers, booleans. */
export function isPlaceholder(v: string): boolean {
  const s = v.trim().replace(/^["']|["']$/g, "");
  if (s.length < 4) return true;
  if (/^(\$|%|<|\{\{|\$\{|\[redacted)/i.test(s)) return true;
  if (/^(process\.env|os\.environ|env\(|getenv|secrets\.)/i.test(s)) return true;
  if (/^(x{3,}|\*{3,}|\.{3,}|•+|-+)$/i.test(s)) return true;
  if (/^(changeme|change_me|your[_-]|example|placeholder|dummy|sample|null|none|nil|true|false|undefined|redacted|todo)/i.test(s)) return true;
  if (/^\d+(\.\d+)?[a-z]{0,2}$/i.test(s)) return true; // 3600, 30s, 1.5
  return false;
}

// Key names that hold secrets in env files, configs and command lines.
const SECRET_KEY = /(SECRET|TOKEN|PASSW(?:OR)?D|(?:^|_)PWD(?:_|$)|API_?KEY|PRIVATE_?KEY|ACCESS_?KEY|CREDENTIALS?|(?:^|_)AUTH(?:_|$)|(?:^|_)DSN(?:_|$)|DATABASE_URL|CONN(?:ECTION)?_?STRING|WEBHOOK_URL|SIGNING_KEY|CLIENT_KEY)/i;

/** Specific formats first: when two rules match the same text, the earlier one names it. */
const DETECTORS: Detector[] = [
  { kind: "Private key", re: /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g },
  { kind: "GitHub token", re: /\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{22,255})\b/g },
  { kind: "GitLab token", re: /\bglpat-[A-Za-z0-9_-]{20,}\b/g },
  { kind: "Anthropic key", re: /\bsk-ant-[A-Za-z0-9_-]{20,}/g },
  // OpenAI style: sk-..., sk-proj-... Needs a digit, so a word like "sk-learn-is-a-library" stays.
  { kind: "API key", re: /(?<![A-Za-z0-9_-])sk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,}/g, ok: (v) => /\d/.test(v) && /[A-Za-z]/.test(v.slice(3)) },
  { kind: "Stripe key", re: /\b(?:sk|rk|pk|whsec)_(?:live|test)_[A-Za-z0-9]{16,}\b|\bwhsec_[A-Za-z0-9]{24,}\b/g },
  { kind: "Slack token", re: /\bxox[abposr]-[A-Za-z0-9-]{10,}\b/g },
  { kind: "AWS access key", re: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g },
  { kind: "Google API key", re: /\bAIza[0-9A-Za-z_-]{35}\b/g },
  { kind: "npm token", re: /\bnpm_[A-Za-z0-9]{36}\b/g },
  { kind: "Hugging Face token", re: /\bhf_[A-Za-z0-9]{30,}\b/g },
  { kind: "JWT", re: /\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g },
  { kind: "Bearer token", re: /\b[Bb]earer\s+([A-Za-z0-9._~+/-]{16,}=*)/g, group: 1, ok: (v) => !isPlaceholder(v) },
  { kind: "Password in a URL", re: /\b[a-z][a-z0-9+.-]*:\/\/[^\s:/@'"`]+:([^\s@/'"`]+)@/g, group: 1, ok: (v) => !isPlaceholder(v) },
  // KEY=value with a secret-sounding upper-case key, on its own line or inline in a command.
  { kind: "Secret value", re: /(?<![A-Za-z0-9_])([A-Z][A-Z0-9_]*)[ \t]*=[ \t]*("[^"\n]*"|'[^'\n]*'|[^\s"'`;&|]+)/g, group: 2,
    ok: (v, m) => SECRET_KEY.test(m[1]) && !isPlaceholder(v) },
  // "apiKey": "...", password: '...' in JSON, YAML and code.
  { kind: "Secret value", re: /["']?([A-Za-z0-9_-]*(?:api[_-]?key|secret|token|passw(?:or)?d)[A-Za-z0-9_-]*)["']?[ \t]*[:=][ \t]*["']([^"'\s]{6,})["']/gi, group: 2,
    ok: (v) => !isPlaceholder(v) },
  { kind: "Email address", re: /(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,24}\b/g, ok: emailOk },
];

/** Only for text read from a .env file: every `KEY=value` value is hidden, not just the secret-sounding ones. */
const ENV_LINE: Detector = { kind: ".env value", re: /^[ \t]*(?:export[ \t]+)?[A-Za-z_][A-Za-z0-9_]*[ \t]*=[ \t]*(.+?)[ \t]*$/gm, group: 1, ok: (v) => !isPlaceholder(v) };

const FILE_TLD = /^(png|jpe?g|gif|svg|webp|ico|js|mjs|cjs|ts|tsx|jsx|css|scss|json|md|html?|lock|ya?ml|toml|rs|go|py|rb|sh)$/i;

/** Emails that are not personal: example domains, no-reply addresses, ssh remotes (git@github.com), image@2x.png. */
function emailOk(v: string): boolean {
  const [local, domain] = [v.slice(0, v.lastIndexOf("@")), v.slice(v.lastIndexOf("@") + 1).toLowerCase()];
  const tld = domain.slice(domain.lastIndexOf(".") + 1);
  if (FILE_TLD.test(tld)) return false;
  if (/(^|\.)example\.(com|org|net)$|\.(example|test|invalid|localhost)$/.test(domain)) return false;
  if (/noreply|no-reply/i.test(local) || domain === "users.noreply.github.com") return false;
  if (local === "git") return false;
  return true;
}

/** The secret-looking places in `text`, in order, without overlaps (the more specific rule wins). */
export function scanSecrets(text: string, ctx: ScanContext = {}): SecretSpan[] {
  if (!text) return [];
  const found: (SecretSpan & { rank: number })[] = [];
  const rules = ctx.env ? [...DETECTORS, ENV_LINE] : DETECTORS;
  rules.forEach((d, rank) => {
    d.re.lastIndex = 0;
    for (let m = d.re.exec(text); m; m = d.re.exec(text)) {
      if (m[0] === "") { d.re.lastIndex++; continue; }
      const g = d.group ?? 0;
      let value = m[g];
      if (value === undefined) continue;
      // A captured value sits at the end of its match (before a closing quote or @ at most).
      let start = m.index + (g ? m[0].lastIndexOf(value) : 0);
      // Quotes around a value stay; only what's inside is hidden.
      const q = /^(["'])(.*)\1$/.exec(value);
      if (q) { start += 1; value = q[2]; }
      if (!value || (d.ok && !d.ok(value, m))) continue;
      found.push({ start, end: start + value.length, kind: d.kind, value, rank });
    }
  });
  found.sort((a, b) => a.rank - b.rank || a.start - b.start);
  const kept: SecretSpan[] = [];
  for (const f of found) {
    if (kept.some((k) => f.start < k.end && k.start < f.end)) continue;
    kept.push({ start: f.start, end: f.end, kind: f.kind, value: f.value });
  }
  return kept.sort((a, b) => a.start - b.start);
}

/** What a hidden value turns into. */
export const maskLabel = (kind: string) => `[hidden: ${kind}]`;

/**
 * Hides every secret in `text` except the values in `keep` (the user chose to show those).
 * `alsoHide` are values found elsewhere in the report: a token read from .env and later printed by a
 * command is hidden there too, even though that line alone wouldn't look like a secret.
 */
export function maskText(text: string, ctx: ScanContext = {}, keep: Set<string> = new Set(), alsoHide: { value: string; kind: string }[] = []): string {
  if (!text) return text;
  let out = "";
  let at = 0;
  for (const s of scanSecrets(text, ctx)) {
    if (keep.has(s.value)) continue;
    out += text.slice(at, s.start) + maskLabel(s.kind);
    at = s.end;
  }
  out += text.slice(at);
  // Long values only: hiding a short one everywhere ("3000") would eat ordinary text.
  for (const h of [...alsoHide].sort((a, b) => b.value.length - a.value.length)) {
    if (h.value.length >= 12 && !keep.has(h.value) && out.includes(h.value)) out = out.split(h.value).join(maskLabel(h.kind));
  }
  return out;
}

/** A short, safe hint at a value for the un-mask list: its first characters and its length. */
export function hint(value: string): string {
  const flat = value.replace(/\s+/g, " ");
  const show = flat.length > 12 ? 4 : flat.length > 6 ? 2 : 0;
  return `${flat.slice(0, show)}${"•".repeat(Math.min(8, Math.max(3, flat.length - show)))} (${value.length} characters)`;
}

/** Replaces the home folder at the start of a path with `~`, anywhere in the text. */
export function shortenHome(text: string, home: string | null | undefined): string {
  const h = (home ?? "").replace(/\/+$/, "");
  if (!text || h.length < 2) return text;
  const re = new RegExp(`${h.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![A-Za-z0-9._-])`, "g");
  return text.replace(re, "~");
}
