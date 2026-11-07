# Roadmap

What we plan to work on next, and what we have decided not to build. It is a direction, not a promise: priorities move with your feedback. What already shipped is in the [changelog](../CHANGELOG.md).

Want something on this list sooner, or something that isn't on it? Open a [feature request](https://github.com/Harshil-Jani/nekotrail/issues/new/choose) or say so in [Discussions](https://github.com/Harshil-Jani/nekotrail/discussions).

## Next

- **A signed, notarized Mac app.** So macOS opens NekoTrail on first launch without the `xattr` step. Updates are already signed and install from inside the app.
- **Linux, tested for real.** The `.deb` and `.AppImage` are new and have only run in CI. Reports from real machines (other desktops, Wayland, ARM) decide what gets fixed first.
- **More agents over the Agent Client Protocol (ACP).** Gemini over ACP would give it "Ask before changes". Forking ACP chats, and each agent's live model list in the model picker.
- **Speed at scale.** Fast switching and a flat CPU graph with 30 or more workspaces across several repos.
- **Polish from your reports.** First-run setup, error messages and the Conductor move, wherever new users get stuck.

## Later, maybe

- Mirroring a workspace into the repo's main checkout, for tools that only run there. It overwrites files in your main checkout, so it needs a careful design first.
- Port forwarding beyond localhost. Today the Preview tab shows local dev servers only.

## Not planned

- **Cloud workspaces, teams, accounts, mobile apps.** NekoTrail stays local-first: your code, chats and worktrees stay on your machine.
- **Provider and API key settings.** The agent CLIs own those, with your own logins and subscriptions; NekoTrail doesn't proxy them.
- **A full code editor.** ⌘P edits a file in place, and "Open in" hands off to your editor for anything bigger.
- **A theme marketplace.** Light and dark follow your system.
