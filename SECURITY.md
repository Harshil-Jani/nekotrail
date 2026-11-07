# Security policy

nekotrail runs coding agents and shell commands on your machine, reads your repos and uses your `gh` login, so security reports matter a lot to us. Thank you for taking the time.

## Reporting a vulnerability

Please **don't open a public issue** for a security problem. Report it privately through GitHub instead:

1. Go to the [Security tab](https://github.com/Harshil-Jani/nekotrail/security) of this repo.
2. Click **Report a vulnerability** and fill in the form ([GitHub's guide](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)).

Only the maintainers can see the report. Please include:

- the nekotrail version (Settings, next to the version, or the output of ⌘K, "Copy diagnostics for a bug report"),
- your OS and, if it matters, the agent involved,
- the steps to reproduce, and what an attacker gains (for example, a repo that runs a command without your approval),
- a proof of concept if you have one. Please use a throwaway repo, and no real tokens or personal data.

We aim to reply within a week. We'll keep you updated, credit you in the advisory and the changelog if you'd like, and ask you to hold off on public disclosure until a fixed release is out.

## Supported versions

Only the latest release gets security fixes. The app updates itself, so the fix reaches you on the next launch or within 6 hours.

## What's in scope

Some examples of what we'd treat as a vulnerability:

- A repo, branch, PR, issue or `nekotrail.json` that makes nekotrail run a command, or start an agent with repo config (hooks, MCP servers and the like), without your approval.
- An agent turn that gets more permission than the mode you picked, or writes to protected files.
- Secrets that end up somewhere nekotrail says they won't: its database, logs, exports, diagnostics or hand-off briefs.
- The Preview tab or a deep link (`nekotrail://`) reaching anything beyond what it's documented to.
- The updater accepting an update that isn't signed with the release key.

Out of scope: problems in the agent CLIs, `gh` or `git` themselves (please report those upstream), and anything that needs someone to already have control of your user account.
