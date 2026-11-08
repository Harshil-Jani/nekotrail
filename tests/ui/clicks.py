"""Real clicks on the real UI (the browser preview, headless). Run: python3 tests/ui/clicks.py

Covers what the backend e2e can't: navigation that only exists in the DOM. Every bug a user hits
by clicking gets a check here. Needs Python Playwright (pip install playwright; playwright install chromium).
"""
import os, subprocess, sys, time, urllib.request
from playwright.sync_api import sync_playwright

PORT = int(os.environ.get("UI_PORT", "5391"))
fails = []

# Worst text contrast inside a selector, against the backgrounds it actually sits on (WCAG 2 formula).
CONTRAST = r"""(sel) => {
  const parse = (c) => { const m = c.match(/rgba?\(([^)]+)\)/); if (!m) { const cv=document.createElement('canvas').getContext('2d'); cv.fillStyle=c; const h=cv.fillStyle; if (h.startsWith('#')) return [parseInt(h.slice(1,3),16),parseInt(h.slice(3,5),16),parseInt(h.slice(5,7),16),1]; return parse(h);} const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number); return [p[0], p[1], p[2], p[3] ?? 1]; };
  const toRgb = (c) => { const cv=document.createElement('canvas'); cv.width=cv.height=1; const x=cv.getContext('2d'); x.fillStyle=c; x.fillRect(0,0,1,1); const d=x.getImageData(0,0,1,1).data; return [d[0],d[1],d[2],d[3]/255]; };
  const over = (a, b) => [0,1,2].map(i => a[i]*a[3] + b[i]*(1-a[3])).concat(1);
  const lum = (c) => { const f = (v) => { v/=255; return v <= 0.03928 ? v/12.92 : Math.pow((v+0.055)/1.055, 2.4); }; return 0.2126*f(c[0]) + 0.7152*f(c[1]) + 0.0722*f(c[2]); };
  const root = document.querySelector(sel); if (!root) return null;
  let base = toRgb(getComputedStyle(document.body).backgroundColor); if (base[3] === 0) base = toRgb(getComputedStyle(document.documentElement).backgroundColor);
  if (base[3] === 0) base = [20,20,30,1];
  const bgOf = (el) => { const chain=[]; for (let e=el; e; e=e.parentElement) { chain.unshift(e); if (e===root) break; } let b = over(toRgb(getComputedStyle(root.parentElement).backgroundColor), base); for (const e of chain) { const c = toRgb(getComputedStyle(e).backgroundColor); if (c[3] > 0) b = over(c, b); } return b; };
  let worst = 99, what = '';
  for (const el of root.querySelectorAll('*')) {
    const t = [...el.childNodes].filter(n => n.nodeType === 3 && n.textContent.trim()).map(n=>n.textContent.trim()).join(' ');
    if (!t || !el.getClientRects().length) continue;
    const cs = getComputedStyle(el); const fg0 = toRgb(cs.color); const bg = bgOf(el); const fg = over([fg0[0],fg0[1],fg0[2],fg0[3]*Number(cs.opacity)], bg);
    const L1 = lum(fg), L2 = lum(bg); const r = (Math.max(L1,L2)+0.05)/(Math.min(L1,L2)+0.05);
    if (r < worst) { worst = r; what = t.slice(0,40) + ' ' + cs.color; }
  }
  return [Math.round(worst*100)/100, what];
}"""

def check(name, ok):
    print(("PASS " if ok else "FAIL ") + name)
    if not ok: fails.append(name)

srv = subprocess.Popen(["bunx", "vite", "--port", str(PORT), "--strictPort"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(100):
        try: urllib.request.urlopen(f"http://localhost:{PORT}/preview.html"); break
        except Exception: time.sleep(0.2)
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        pg = b.new_page(viewport={"width": 1440, "height": 900})
        chat_shown = lambda: pg.evaluate("() => { const c = document.querySelector('.panel .chat'); return !!c && !c.closest('.pane').hidden }")

        # 2026-11-08: on Changes, clicking the chat tab did nothing (portal events skip the slot's React handler).
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=changes")
        pg.wait_for_selector(".ws-chats .chat-tab button", timeout=20000)
        pg.click(".ws-chats .chat-tab.active button"); pg.wait_for_timeout(300)
        check("Changes -> click the open chat's tab shows the chat", chat_shown())
        pg.click("nav.view-tabs button:has-text('Terminal')"); pg.wait_for_timeout(300)
        # 2026-11-08: off the chat, the chat tab looked like plain text; it must keep its icon and outline.
        marked = pg.evaluate("() => { const t = document.querySelector('.ws-chats .chat-tab.active'); return !!t && !!t.querySelector('.chat-tab-icon, .spinner') && getComputedStyle(t).boxShadow !== 'none' }")
        check("On Terminal, the chat tab still looks like a tab (icon + outline)", marked)
        pg.locator(".ws-chats .chat-tab button[role=tab]").last.click(); pg.wait_for_timeout(300)
        check("Terminal -> click another chat's tab shows the chat", chat_shown())

        # 2026-11-08: the prompt rail lists your messages; clicking one brings it to the screen and lights its tick.
        on = "() => [...document.querySelectorAll('.prompt-ticks .tick')].findIndex(t => t.classList.contains('on'))"
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat&prompts=24")
        pg.wait_for_selector(".prompt-ticks .tick", timeout=20000); pg.wait_for_timeout(800)
        n = pg.locator(".prompt-ticks .tick").count()
        check(f"Prompt rail has one tick per message you sent (25, got {n})", n == 25)
        check("At the bottom, the latest message's tick is lit", pg.evaluate(on) == n - 1)
        pg.hover(".prompt-ticks"); pg.wait_for_timeout(300)
        check("Hovering the rail lists your messages", pg.locator(".prompt-list button").count() == n)
        pg.locator(".prompt-list button").nth(3).click(); pg.wait_for_timeout(900); pg.mouse.move(400, 400); pg.wait_for_timeout(200)
        shown = pg.evaluate("() => { const r = document.querySelector('.messages .row-flash'); if (!r) return false; const b = r.getBoundingClientRect(), m = document.querySelector('.messages').getBoundingClientRect(); return b.top >= m.top && b.bottom <= m.bottom }")
        check("Clicking message 4 brings it on screen and lights its tick", shown and pg.evaluate(on) == 3)

        # 2026-11-08: a finished turn shows its answer; its steps fold into one line that opens on click.
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat")
        pg.wait_for_selector(".steps-fold", timeout=20000)
        check("A finished turn folds its steps (no step rows drawn)", pg.locator(".messages .step").count() == 0)
        pg.locator(".steps-fold").first.click(); pg.wait_for_timeout(300)
        check("Clicking 'Worked for ... steps' shows the steps", pg.locator(".messages .step").count() > 0)
        # 2026-11-08: "Worked for 41s" and "Done · 41.2s" on one reply read as two timers. The time shows once.
        check("A folded turn shows its time once (no 'Done · 41.2s' under it)", pg.evaluate("() => ![...document.querySelectorAll('.messages .msg.done')].some(d => /Done · \\d/.test(d.textContent))") and pg.locator(".steps-fold").count() > 0)

        # 2026-11-08: a not-started todo drew its circle above centred text (the queued-messages list's .pending
        # style leaked onto li.todo.pending). Every todo row is one line: icon left of the text.
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat&long")
        pg.wait_for_selector(".todo.pending", timeout=20000); pg.wait_for_timeout(300)
        flat = pg.evaluate("() => [...document.querySelectorAll('.todo')].every(li => { const i = li.querySelector('svg').getBoundingClientRect(), t = li.getBoundingClientRect(); return getComputedStyle(li).flexDirection === 'row' && i.left - t.left < 4 })")
        check("Todo rows (done, doing, not started) all put the icon left of the text", flat)
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat"); pg.wait_for_selector("#workspace-search", timeout=20000)

        # 2026-11-08: the sidebar Search box reaches inside every chat (Enter opens the full search with the words typed).
        pg.fill("#workspace-search", "timeout"); pg.wait_for_timeout(200)
        check("Typing in sidebar Search offers 'Search all chats'", pg.locator(".deep-search").count() == 1)
        pg.press("#workspace-search", "Enter"); pg.wait_for_timeout(1200)
        check("Enter opens chat search with the words and finds matches", pg.locator(".gs-snippet").count() > 0)

        # 2026-11-08: the New workspace sheet. At 1100px "More" spilled past the sheet and its menu floated over the chat;
        # agents had no logos; the repo picker was a plain select with no pictures.
        inside_win = "(sel) => { const m = document.querySelector(sel); if (!m) return false; const r = m.getBoundingClientRect(); return r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight }"
        for w, h in ((1100, 900), (1100, 640), (1800, 1000)):
            pg.set_viewport_size({"width": w, "height": h})
            pg.goto(f"http://localhost:{PORT}/preview.html?new=r1&agents=all&repos=15")
            pg.wait_for_selector(".nw-panel .nw-agent-btn", timeout=20000); pg.wait_for_timeout(500)
            fits = pg.evaluate("() => { const p = document.querySelector('.nw-panel').getBoundingClientRect(); return [...document.querySelectorAll('.nw-head > *, .nw-head > * *')].every(e => { const r = e.getBoundingClientRect(); return !r.width || (r.right <= p.right + 0.5 && r.left >= p.left - 0.5) }) }")
            check(f"{w}x{h}: every New workspace header item fits inside the sheet", fits)
            pg.click(".nw-agent-btn"); pg.wait_for_timeout(200)
            rows = pg.locator(".nw-agent-menu [role^=menuitem]").count()
            logos = pg.evaluate("() => [...document.querySelectorAll('.nw-agent-menu [role^=menuitem]')].filter(b => b.querySelector('.agent-logo')).length")
            check(f"{w}x{h}: agent menu shows a logo on every agent ({logos}/{rows})", rows >= 16 and logos == rows)
            check(f"{w}x{h}: agent menu stays inside the window", pg.evaluate(inside_win, ".nw-agent-menu"))
            pg.keyboard.press("Escape"); pg.wait_for_timeout(150)
            check(f"{w}x{h}: Esc closes the agent menu, not the sheet", pg.locator(".nw-agent-menu").count() == 0 and pg.locator(".nw-panel").count() == 1)
            pg.click(".nw-repo-btn"); pg.wait_for_timeout(300)
            rows = pg.locator(".nw-repo-menu [role=option]").count()
            pics = pg.evaluate("() => [...document.querySelectorAll('.nw-repo-menu [role=option]')].filter(b => b.querySelector('img.avatar, .avatar-fallback')).length")
            check(f"{w}x{h}: repo picker shows a picture per repo ({pics}/{rows}) and ticks the current one", rows == 15 and pics == rows and pg.locator(".nw-repo-menu [aria-selected=true] svg").count() == 1)
            check(f"{w}x{h}: repo menu stays inside the window", pg.evaluate(inside_win, ".nw-repo-menu"))
            pg.keyboard.type("kib"); pg.wait_for_timeout(150)
            check(f"{w}x{h}: typing filters the repo list", pg.locator(".nw-repo-menu [role=option]").count() == 1)
            pg.keyboard.press("Enter"); pg.wait_for_timeout(200)
            check(f"{w}x{h}: Enter picks the repo and keeps the sheet open", pg.locator(".nw-repo-menu").count() == 0 and pg.locator(".nw-panel").count() == 1 and "kibble-billing" in pg.inner_text(".nw-repo-btn"))
        pg.click(".nw-repo-btn"); pg.keyboard.press("ArrowDown"); pg.keyboard.press("Enter"); pg.wait_for_timeout(200)
        check("Repo picker: Down then Enter picks the next repo", "litterbox-infra" in pg.inner_text(".nw-repo-btn"))
        pg.click(".nw-foot .picker-btn"); pg.wait_for_timeout(200); pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
        check("Esc in the model picker closes the picker, not the sheet", pg.locator(".nw-panel").count() == 1 and pg.locator(".nw-foot .picker-menu").count() == 0)
        pg.click(".nw-perm-btn"); pg.keyboard.press("ArrowUp"); pg.keyboard.press("Enter"); pg.wait_for_timeout(200)
        check("Permissions menu: arrows and Enter change the mode", "Ask before changes" in pg.inner_text(".nw-perm-btn"))
        pg.click(".nw-prompt")
        left = []
        for _ in range(20):
            pg.keyboard.press("Tab")
            if not pg.evaluate("() => !!document.activeElement.closest('.nw-panel')"): left.append(1)
        check("Tab stays inside the New workspace sheet", not left)
        pg.fill(".nw-prompt", "Keep this prompt"); pg.keyboard.press("Escape"); pg.wait_for_timeout(200)
        pg.click("button:has-text('Create workspace')"); pg.wait_for_selector(".nw-prompt", timeout=5000)
        check("Esc then reopen keeps what you typed", pg.input_value(".nw-prompt") == "Keep this prompt")
        # 2026-11-08: the Environment sheet. It opens from the workspace's More actions menu and lists every source.
        pg.set_viewport_size({"width": 1440, "height": 900})
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat")
        pg.wait_for_selector(".tb-more", timeout=20000)
        pg.click(".tb-more"); pg.click(".ws-menu [role=menuitem]:has-text('Environment')")
        pg.wait_for_selector(".env-sheet .env-sec", timeout=5000)
        titles = pg.locator(".env-sheet .env-sec h3").all_inner_texts()
        check(f"More actions -> Environment opens the sheet with its 4 sections ({titles})", titles == ["NekoTrail", "Repo settings", ".env files in this workspace", "From your login shell"])
        # A .env value is masked until its eye is clicked, and hidden again on a second click.
        val = ".env-sec[data-section=files] .env-row-item >> nth=0"
        masked = pg.locator(val).locator(".env-value").inner_text()
        pg.locator(val).locator("button[aria-label^='Show the value']").click(); pg.wait_for_timeout(150)
        shown = pg.locator(val).locator(".env-value").inner_text()
        pg.locator(val).locator("button[aria-label^='Hide the value']").click(); pg.wait_for_timeout(150)
        check("A .env value is masked until the eye is clicked", masked == "••••••••" and shown.startswith("postgres://") and pg.locator(val).locator(".env-value").inner_text() == "••••••••")

        # ---- QA sweep, 2026-11-08 ----
        pg.set_viewport_size({"width": 1440, "height": 900})
        menus_up = "() => [...document.querySelectorAll('[role=menu]')].filter(m => m.getClientRects().length).length"
        def open_ws(q, sel=".tb-more"):
            pg.goto(f"http://localhost:{PORT}/preview.html?{q}"); pg.wait_for_selector(sel, timeout=20000); pg.wait_for_timeout(500)
        # P0: menus closed only on mouseleave, so one left open sat over the tabs and ate the next click.
        MENUS = [  # (page, trigger, menu, what)
            ("ws=w1&tab=chat", ".tb-more", ".ws-menu", "Workspace ... menu"),
            ("ws=w1&tab=chat", ".newchat > button", ".newchat-menu", "New chat + menu"),
            ("ws=w1&tab=chat", ".composer-more", ".composer-menu", "Composer + menu"),
            ("ws=w1&tab=chat", ".git-actions button[aria-label='More git actions']", ".git-actions .menu", "PR bar git menu"),
            ("multi&ws=g1&tab=chat", ".git-actions button[aria-label='More git actions']", ".git-actions .menu", "Multi-repo git menu"),
            ("ws=w1&tab=chat", "button[aria-label='Workspace list options']", ".list-menu", "Sidebar list menu"),
            ("ws=w1&tab=chat", "button[aria-label='This week options']", ".sec-menu", "Sidebar section menu"),
            ("ws=w1&tab=changes", ".review-toggle", ".review-pop", "Review (n) popover"),
        ]
        for q, trig, menu, what in MENUS:
            open_ws(q, trig)
            pg.click(trig); pg.wait_for_timeout(150)
            opened = pg.locator(menu).count() == 1
            pg.keyboard.press("Escape"); pg.wait_for_timeout(150)
            back = pg.evaluate("(s) => document.activeElement === document.querySelector(s)", trig)
            check(f"{what}: Esc closes it and focus goes back to its button", opened and pg.locator(menu).count() == 0 and back)
            pg.click(trig); pg.wait_for_timeout(150)
            away = ".messages" if "chat" in q else ".file-diff"
            box = pg.locator(away).first.bounding_box()
            pg.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] - 40); pg.wait_for_timeout(150)
            check(f"{what}: a click in {away} closes it", pg.locator(menu).count() == 0)
        open_ws("ws=w1&tab=chat")
        pg.click(".tb-more"); pg.wait_for_timeout(100); pg.click(".composer-more"); pg.wait_for_timeout(150)
        check("Opening one menu closes the other (one [role=menu] on screen)", pg.evaluate(menus_up) == 1 and pg.locator(".ws-menu").count() == 0)
        pg.keyboard.press("Escape")
        pg.click(".tb-more"); pg.wait_for_timeout(150)
        pg.click("nav.view-tabs button:has-text('Changes')", timeout=3000); pg.wait_for_timeout(400)
        check("With ... open, a normal click on the Changes tab switches tab", pg.locator(".ws-menu").count() == 0 and pg.locator(".file-list").count() > 0)

        # P1a: the section's ... covered its fold chevron.
        open_ws("ws=w1&tab=chat", ".section-head")
        head = pg.locator(".section-head:has-text('This week')"); head.hover(); pg.wait_for_timeout(250)
        fold_hit = pg.evaluate("() => { const h=[...document.querySelectorAll('.section-head')].find(e=>e.textContent.includes('This week')); const f=h.querySelector('.fold'); const r=f.getBoundingClientRect(); const e=document.elementFromPoint(r.left+r.width/2, r.top+r.height/2); return !!e && f.contains(e) }")
        was = head.locator(".fold").get_attribute("aria-expanded")
        pg.mouse.click(*(lambda b: (b["x"] + b["width"] / 2, b["y"] + b["height"] / 2))(head.locator(".fold").bounding_box())); pg.wait_for_timeout(200)
        check("Section fold chevron is on top at its centre and a click toggles aria-expanded", fold_hit and head.locator(".fold").get_attribute("aria-expanded") != was)
        pg.mouse.click(*(lambda b: (b["x"] + b["width"] / 2, b["y"] + b["height"] / 2))(head.locator(".fold").bounding_box())); pg.wait_for_timeout(150)

        # P2: an empty section is hidden, and shows again while you drag a workspace (so you can drop into it).
        names = lambda: pg.evaluate("() => [...document.querySelectorAll('.section-head .repo-name')].map(e => e.textContent)")
        hidden = "Waiting on review" not in names()
        pg.locator(".sidebar .ws:has-text('Rate limit the tickets API')").first.dispatch_event("dragstart", {"dataTransfer": pg.evaluate_handle("() => new DataTransfer()")}); pg.wait_for_timeout(200)
        shown = "Waiting on review" in names()
        pg.locator(".sidebar .ws:has-text('Rate limit the tickets API')").first.dispatch_event("dragend"); pg.wait_for_timeout(150)
        check("An empty section is hidden, shown while dragging, hidden again after", hidden and shown and "Waiting on review" not in names())

        # P1b: agent logos on chat tabs, hand-off choices, queue rows and board cards.
        open_ws("ws=w1&tab=chat", ".ws-chats .chat-tab")
        check("Every chat tab shows its agent's logo", pg.evaluate("() => { const t=[...document.querySelectorAll('.ws-chats .chat-tab')]; return t.length > 1 && t.every(x => x.querySelector('.agent-logo.chat-tab-icon, .spinner')) }"))
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&handoff=w1"); pg.wait_for_selector(".handoff-agents button", timeout=20000)
        check("Every Hand off agent choice shows its logo", pg.evaluate("() => [...document.querySelectorAll('.handoff-agents button')].every(b => b.querySelector('.agent-logo'))"))
        pg.goto(f"http://localhost:{PORT}/preview.html?view=queue"); pg.wait_for_selector(".tq-row", timeout=20000)
        check("Every queue row shows its agent's logo", pg.evaluate("() => [...document.querySelectorAll('.tq-row')].every(r => r.querySelector('.tq-meta .agent-logo'))"))
        pg.goto(f"http://localhost:{PORT}/preview.html?view=board"); pg.wait_for_selector(".board .card", timeout=20000); pg.wait_for_timeout(500)
        check("Board cards show their agents' logos", pg.evaluate("() => [...document.querySelectorAll('.board .card')].every(c => c.querySelector('.card-agents .agent-logo'))"))

        # P1c: the Queue Add form, Search filters and first run use the app's menus, not system selects.
        pg.goto(f"http://localhost:{PORT}/preview.html?view=queue&queueAdd"); pg.wait_for_selector(".tq-form .mselect-btn", timeout=20000)
        check("Queue Add form has no system selects", pg.locator(".tq-form select").count() == 0 and pg.locator(".tq-form .mselect-btn").count() == 5)
        perm = pg.locator(".tq-form .mselect-btn[aria-label^='Permissions']")
        perm.click(); pg.wait_for_timeout(150); pg.keyboard.press("ArrowDown"); pg.keyboard.press("Enter"); pg.wait_for_timeout(150)
        check("Queue Add: arrows and Enter change Permissions, the form stays open", pg.locator(".tq-form").count() == 1 and "Full auto" in perm.inner_text())
        pg.locator(".tq-form .mselect-btn").first.click(); pg.wait_for_timeout(150); pg.keyboard.press("Escape"); pg.wait_for_timeout(150)
        check("Queue Add: Esc closes the menu, not the form", pg.locator(".mselect-menu").count() == 0 and pg.locator(".tq-form").count() == 1)
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat"); pg.wait_for_selector("#workspace-search", timeout=20000)
        pg.fill("#workspace-search", "timeout"); pg.press("#workspace-search", "Enter"); pg.wait_for_selector(".gs-filters", timeout=5000)
        check("Search filters have no system selects", pg.locator(".gs-filters select").count() == 0 and pg.locator(".gs-filters .mselect-btn").count() == 3)
        pg.locator(".gs-filters .mselect-btn[aria-label^='Date']").click(); pg.wait_for_timeout(150)
        pg.locator(".mselect-menu [role=menuitemradio]").nth(1).click(); pg.wait_for_timeout(200)
        check("Search: picking a date filter marks it set", pg.locator(".gs-filters .mselect.set").count() == 1)
        fr = b.new_page(viewport={"width": 1440, "height": 900})  # a clean profile: first run shows only on a fresh install
        fr.goto(f"http://localhost:{PORT}/preview.html?fresh&step=try"); fr.wait_for_selector(".fr-try", timeout=20000)
        check("First run Try it has no system selects", fr.locator(".fr-try select").count() == 0 and fr.locator(".fr-fields .mselect-btn").count() >= 1)
        fr.close()

        # P1d: at the smallest window, Changes kept its controls whole.
        pg.set_viewport_size({"width": 900, "height": 560})
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=changes"); pg.wait_for_selector(".file-head", timeout=20000); pg.wait_for_timeout(500)
        cut = pg.evaluate("""() => [...document.querySelectorAll('.file-head button, .seg button, .review-ctl button')].filter(b => {
            const r = b.getBoundingClientRect(); if (!r.width) return false;
            const e = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
            const out = r.left < 0 || r.right > innerWidth || r.top < 0 || r.bottom > innerHeight;
            let clipped = false; for (let p = b.parentElement; p; p = p.parentElement) { const s = getComputedStyle(p); if (s.overflowX !== 'visible') { const q = p.getBoundingClientRect(); if (r.left < q.left - 0.5 || r.right > q.right + 0.5) clipped = true; } }
            return out || clipped || !(e && b.contains(e)) || b.scrollWidth > b.clientWidth + 1 }).map(b => b.textContent.trim() || b.getAttribute('aria-label'))""")
        check(f"900x560 Changes: every .file-head, .seg and .review-ctl button whole and visible (cut: {cut})", not cut)
        tab_ok = pg.evaluate("""() => { const t = document.querySelector('.ws-chats .chat-tab.active'), s = t.closest('.chat-tab-strip'), n = t.querySelector('.chat-tab-title');
            const r = t.getBoundingClientRect(), q = s.getBoundingClientRect();
            return r.left >= q.left - 0.5 && r.right <= q.right + 0.5 && (n.scrollWidth <= n.clientWidth || getComputedStyle(n).textOverflow === 'ellipsis') }""")
        check("900x560: the active chat tab sits whole in the strip, its title ellipsised", tab_ok)
        pg.set_viewport_size({"width": 1440, "height": 900})

        # P2: the palette listed "Show archived workspaces" twice (same id, React key error).
        errs = []
        pg.on("console", lambda m: errs.append(m.text) if m.type == "error" else None)
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat"); pg.wait_for_selector(".tb-more", timeout=20000); pg.wait_for_timeout(300)
        pg.keyboard.press("Meta+k"); pg.wait_for_selector(".palette", timeout=3000); pg.keyboard.type("archived"); pg.wait_for_timeout(200)
        n = pg.locator(".palette-list .palette-label:has-text('archived workspaces')").count()
        check(f"Palette lists 'Show archived workspaces' once, no duplicate-key error ({n})", n == 1 and not any("same key" in e for e in errs))
        pg.keyboard.press("Escape")
        # P2: Settings took no focus and let Tab walk out behind it.
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat&settings"); pg.wait_for_selector(".modal.settings", timeout=20000); pg.wait_for_timeout(300)
        took = pg.evaluate("() => !!document.activeElement.closest('.modal.settings')")
        left = 0
        for i in range(40):
            pg.keyboard.press("Shift+Tab" if i % 3 == 2 else "Tab")
            if not pg.evaluate("() => !!document.activeElement.closest('.modal.settings')"): left += 1
        check("Settings takes focus when it opens and keeps Tab inside", took and left == 0)
        # P2: faint text in the Ports popover read 2.2:1.
        pg.goto(f"http://localhost:{PORT}/preview.html?ws=w1&tab=chat&ports"); pg.wait_for_selector(".ports-pop .ports-row", timeout=20000); pg.wait_for_timeout(300)
        worst = pg.evaluate(CONTRAST, ".ports-pop")
        check(f"Ports popover: every text at least 4.5:1 (worst {worst[0]}: {worst[1]})", worst[0] >= 4.5)
        b.close()
finally:
    srv.terminate()
sys.exit(1 if fails else 0)
