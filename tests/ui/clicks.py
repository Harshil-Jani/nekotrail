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
        b.close()
finally:
    srv.terminate()
sys.exit(1 if fails else 0)
