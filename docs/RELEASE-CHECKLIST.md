# Release checklist

`scripts/release.sh` runs the build, clippy, the Rust tests and the end-to-end journeys
(`scripts/e2e.sh`) before it tags. Those drive the real app with fake agents. This pass is the
part they can't do: the real window, your real `claude` login, your eyes. It takes about five
minutes. Do it on the build you are about to ship, installed in /Applications, with a throwaway
repo or one you don't mind a test branch in.

Tick each box in your head or in the release notes draft. If one fails, don't tag.

1. **New workspace.** Make one. The sidebar shows it with its branch name.
   - [ ] It appears at once, and its folder exists (right-click, Open in Finder).

2. **Two messages.** Send `reply with the word ok`. While the agent is working, send
   `now reply with the word done`.
   - [ ] The second message shows under the chat right away, as queued.
   - [ ] The first answer arrives and the line under it says Done with a time.
   - [ ] The second one then runs on its own, and its answer arrives.
   - [ ] When it ends, the spinner stops in the chat tab and in the sidebar, and the composer
     no longer says "Agent is working".
   - [ ] The workspace title is not "There" or any other piece of a greeting.

3. **Terminal.** Open the Terminal tab.
   - [ ] A prompt shows within a second or two.
   - [ ] `echo hello` prints hello. A second terminal tab works too.
   - [ ] While the agent works (send `count to 20 slowly`), the terminal still answers.

4. **Queue.** Open the Queue screen.
   - [ ] It loads (empty is fine) and doesn't spin forever.

5. **Changes.** Ask the agent to `create a file called notes.txt with one line`.
   - [ ] Changes shows notes.txt with its line.

6. **Stop.** Send `count to 200 slowly`, then press Stop.
   - [ ] The turn ends within a few seconds and says it was stopped.

7. **Archive.** Archive the workspace.
   - [ ] It leaves the sidebar, and the app stays responsive afterwards (switch workspaces,
     open Settings).

If anything hangs, sample the app before you quit it, so the hang can be found:
`/usr/bin/sample "$(pgrep -f /Applications/nekotrail.app/Contents/MacOS/nekotrail)" 3 -file ~/Desktop/nekotrail-sample.txt`
