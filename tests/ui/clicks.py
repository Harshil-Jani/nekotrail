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
        pg.locator(".ws-chats .chat-tab button[role=tab]").last.click(); pg.wait_for_timeout(300)
        check("Terminal -> click another chat's tab shows the chat", chat_shown())
        b.close()
finally:
    srv.terminate()
sys.exit(1 if fails else 0)
