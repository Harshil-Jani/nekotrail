"""Real clicks on the real UI (the browser preview, headless). Run: python3 tests/ui/clicks.py

Covers what the backend e2e can't: navigation that only exists in the DOM. Every bug a user hits
by clicking gets a check here. Needs Python Playwright (pip install playwright; playwright install chromium).
"""
import subprocess, sys, time, urllib.request
from playwright.sync_api import sync_playwright

PORT = 5391
fails = []

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
        b.close()
finally:
    srv.terminate()
sys.exit(1 if fails else 0)
