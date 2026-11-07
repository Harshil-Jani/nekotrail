"""Every app screenshot from the preview harness, headless: docs/screenshots/<name>.png (1600x1000,
dark unless the name says light) and the site's copy, site/public/shots/<name>.webp.

    python3 scripts/media/shots.py [name ...]     # all by default
"""
import os, subprocess, sys
from playwright.sync_api import sync_playwright
from common import BASE, REPO, avatars

W, H = 1600, 1000
PNG = os.path.join(REPO, "docs", "screenshots")
WEBP = os.path.join(REPO, "site", "public", "shots")


def bottom(pg):
    pg.evaluate("document.querySelectorAll('.messages').forEach(m => m.scrollTop = m.scrollHeight)")
    pg.wait_for_timeout(300)


def new_ws(pg):
    pg.keyboard.type("Add rate limiting to /v1/tickets: 100 requests a minute per API key, with a Retry-After header.", delay=0)


def rewind(pg):
    msg = pg.locator(".msg.user").first
    msg.hover()
    pg.locator("button.rewind", has_text="Rewind to here").first.click()
    pg.wait_for_timeout(500)


def annotate(pg):
    pg.get_by_role("button", name="Annotate").click()
    pg.wait_for_timeout(1200)
    svg = pg.locator("svg.preview-marks, .preview-shot svg").first
    box = svg.bounding_box()
    x0, y0 = box["x"], box["y"]
    # A box round the first ticket row, then an arrow pointing at it.
    pg.mouse.move(x0 + 14, y0 + 146); pg.mouse.down(); pg.mouse.move(x0 + 372, y0 + 198, steps=8); pg.mouse.up()
    pg.get_by_role("radio", name="Arrow").click()
    pg.mouse.move(x0 + 640, y0 + 112); pg.mouse.down(); pg.mouse.move(x0 + 386, y0 + 168, steps=8); pg.mouse.up()
    note = pg.locator(".preview-bar.annotating input").first
    note.fill("Show the ticket number in bold and keep the title on one line.")
    pg.wait_for_timeout(300)


# name: (preview query, action after load). The workspace shots show the working row mid-turn.
SHOTS = {
    "workspace": ("ws=w1&thinking=edits", bottom),
    "workspace-light": ("ws=w1&thinking=edits&theme=light", bottom),
    "board": ("view=board", None),
    "changes": ("ws=w1&tab=changes", None),
    "compare": ("ws=w1&compare=p1", None),
    "mcp": ("ws=w1&mcp", None),
    "model-picker": ("ws=w1&picker", None),
    "new-workspace": ("new=r1", new_ws),
    "night-sky": ("view=sky", None),
    "opencode": ("ws=w2&opencode", bottom),
    "palette": ("ws=w1&palette", None),
    "plan": ("ws=w2", bottom),
    "preview": ("ws=w1&tab=preview", annotate),
    "rewind": ("ws=w1", rewind),
    "scorecard": ("ws=w1&scorecard", None),
    "stacked": ("ws=w3&stackmerged", None),
    "storage": ("ws=w1&storage", None),
    "welcome": ("fresh", None),
}


def main(only):
    os.makedirs(PNG, exist_ok=True)
    with sync_playwright() as p:
        br = p.chromium.launch(headless=True)
        for name, (q, act) in SHOTS.items():
            if only and name not in only:
                continue
            if "theme=" not in q:
                q += "&theme=dark"
            scheme = "light" if "theme=light" in q else "dark"
            ctx = br.new_context(viewport={"width": W, "height": H}, color_scheme=scheme, device_scale_factor=1)
            avatars(ctx)
            pg = ctx.new_page()
            pg.goto(f"{BASE}?{q}&zoom=1")
            pg.wait_for_timeout(2500)
            pg.wait_for_function("Array.from(document.images).every(i => i.complete && i.naturalWidth > 0)", timeout=15000)
            if act:
                act(pg)
            pg.mouse.move(W - 4, H - 4)
            pg.wait_for_timeout(400)
            png = os.path.join(PNG, f"{name}.png")
            pg.screenshot(path=png)
            ctx.close()
            webp = os.path.join(WEBP, f"{name}.webp")
            subprocess.run(["cwebp", "-quiet", "-q", "86", "-m", "6", png, "-o", webp], check=True)
            print(f"shot {name}: png {os.path.getsize(png) // 1024} KB, webp {os.path.getsize(webp) // 1024} KB")
        br.close()


if __name__ == "__main__":
    main(set(sys.argv[1:]))
