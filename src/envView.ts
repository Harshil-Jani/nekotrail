/** Helpers for the Environment sheet (EnvSheet.tsx). The data comes from `workspace_environment` (envview.rs). */
import type { EnvEntry } from "./api";

/** What a hidden value shows: always the same length, so it says nothing about the value. */
export const MASK = "••••••••";

/** Short names for where a value comes from. A .env file keeps its path. */
const LABEL: Record<string, string> = {
  Workspace: "NekoTrail",
  "Port block": "port block",
  "Review workspace": "review workspace",
  "Repo settings": "Repo settings",
  "Login shell": "login shell",
};
export function envSourceLabel(source: string): string {
  return LABEL[source] ?? source;
}

/**
 * The filter: matches the name, where it comes from (or what overrides it), and the value only when
 * that is shown anyway. A hidden value is never searched, so typing can't be used to guess it.
 */
export function envMatches(e: EnvEntry, q: string): boolean {
  const needle = q.trim().toLowerCase();
  if (!needle) return true;
  const hay = [e.key, e.source, envSourceLabel(e.source), e.overridden_by ?? "", e.overridden_by ? "overridden" : "", e.masked ? "" : e.value];
  return hay.some((h) => h.toLowerCase().includes(needle));
}
