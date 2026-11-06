/**
 * Facts the "Start an agent from your editor?" confirm adds above the task: which file and lines the
 * selection came from, and how long the task is when it is too long to read at a glance.
 */
export function taskFacts(prompt: string): [string, string][] {
  const out: [string, string][] = [];
  const ref = /^From (\S+?:\d+(?:-\d+)?):\s*$/m.exec(prompt);
  if (ref) out.push(["Code", ref[1]]);
  if (prompt.length > 2000) {
    const lines = prompt.split("\n").length;
    out.push(["Length", `${lines.toLocaleString("en-US")} lines, ${prompt.length.toLocaleString("en-US")} characters`]);
  }
  return out;
}
