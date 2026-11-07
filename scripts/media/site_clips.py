"""The two site clips clips.py doesn't make, from the preview harness, headless, with a drawn pointer:
site/public/clips/<name>.mp4 (h264), .webm (vp9) and .jpg (poster, the first frame), 1200x750 at 20 fps.

    python3 scripts/media/site_clips.py [parallel] [sky]
"""
import os, shutil, subprocess, sys, tempfile, time
from playwright.sync_api import sync_playwright
from common import BASE, REPO, avatars

W, H = 1200, 750
OUT = os.path.join(REPO, "site", "public", "clips")

# A macOS-style pointer drawn into the page, since headless video has none.
POINTER = """
window.addEventListener('DOMContentLoaded', () => {
  const p = document.createElement('div');
  p.innerHTML = '<svg width="22" height="22" viewBox="0 0 22 22"><path d="M3 2 L3 18 L7.2 14 L10 20.5 L12.8 19.3 L10.1 13 L16 13 Z" fill="#000" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/></svg>';
  Object.assign(p.style, { position: 'fixed', left: '0', top: '0', zIndex: 2147483647, pointerEvents: 'none', transform: 'translate(-100px,-100px)', filter: 'drop-shadow(0 1px 1.5px rgba(0,0,0,.4))' });
  document.documentElement.appendChild(p);
  addEventListener('mousemove', (e) => { p.style.transform = `translate(${e.clientX - 3}px, ${e.clientY - 2}px)`; }, true);
});
"""


class Pointer:
    def __init__(self, pg):
        self.pg, self.x, self.y = pg, W * 0.6, H * 0.7
        pg.mouse.move(self.x, self.y)

    def to(self, x, y, ms=600):
        steps = max(8, ms // 16)
        for i in range(1, steps + 1):
            e = 1 - (1 - i / steps) ** 3
            self.pg.mouse.move(self.x + (x - self.x) * e, self.y + (y - self.y) * e)
            self.pg.wait_for_timeout(ms / steps)
        self.x, self.y = x, y

    def at(self, loc, ms=600):
        b = loc.first.bounding_box()
        self.to(b["x"] + b["width"] / 2, b["y"] + b["height"] / 2, ms)

    def click(self, loc, ms=600, pause=350):
        self.at(loc, ms)
        self.pg.wait_for_timeout(120)
        self.pg.mouse.down(); self.pg.wait_for_timeout(80); self.pg.mouse.up()
        self.pg.wait_for_timeout(pause)


def parallel(pg, m):
    # Mochi is mid-turn (the working row says Thinking, then what it reads and edits) while you look in
    # on the other agents, come back to see it further on, and open the board.
    side = pg.locator(".sidebar")
    m.to(700, 560, 500); pg.wait_for_timeout(2300)          # Thinking, then Reading 3 files
    m.click(side.locator(".ws", has_text="Tickets should breach").first, 800, 1500)
    m.click(side.locator(".ws", has_text="Rate limit the tickets API").first, 700, 1500)
    m.click(side.locator(".ws", has_text="Fix 502s after deploy").first, 700, 2600)  # back: further on
    m.click(side.locator(".nav-row", has_text="Board").first, 800, 1400)
    m.to(560, 210, 700); pg.wait_for_timeout(500)
    m.to(330, 230, 700); pg.wait_for_timeout(800)


def sky(pg, m):
    pg.wait_for_timeout(500)
    for name in ["Biscuit", "Dumpling", "Comet", "Pip", "Muffin"]:
        m.at(pg.locator(".sky-star", has_text=name), 900); pg.wait_for_timeout(700)
    m.to(300, 640, 700); pg.wait_for_timeout(600)


CLIPS = {"parallel": ("ws=w1&thinking", parallel), "sky": ("view=sky", sky)}


def ff(*args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def main(only):
    with sync_playwright() as p:
        br = p.chromium.launch(headless=True)
        for name, (q, act) in CLIPS.items():
            if only and name not in only:
                continue
            tmp = tempfile.mkdtemp()
            ctx = br.new_context(viewport={"width": W, "height": H}, color_scheme="dark", device_scale_factor=1,
                                 record_video_dir=tmp, record_video_size={"width": W, "height": H})
            ctx.add_init_script(POINTER)
            avatars(ctx)
            t_open = time.time()
            pg = ctx.new_page()
            pg.goto(f"{BASE}?{q}&theme=dark&zoom=1")
            # The live turn starts its first tool at 3s: begin while it still says Thinking.
            pg.wait_for_timeout(1600)
            m = Pointer(pg)
            pg.wait_for_timeout(200)
            start = time.time() - t_open
            act(pg, m)
            pg.wait_for_timeout(300)
            end = time.time() - t_open
            video = pg.video.path()
            ctx.close()
            src = os.path.join(tmp, "src.webm"); shutil.move(video, src)
            cut = ["-ss", f"{start:.2f}", "-t", f"{end - start:.2f}", "-i", src, "-r", "20"]
            mp4, webm, jpg = (os.path.join(OUT, f"{name}.{e}") for e in ("mp4", "webm", "jpg"))
            ff(*cut, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "30", "-preset", "slow", "-movflags", "+faststart", "-an", mp4)
            ff(*cut, "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-b:v", "0", "-crf", "42", "-row-mt", "1", "-an", webm)
            ff("-i", mp4, "-frames:v", "1", "-q:v", "4", jpg)
            shutil.rmtree(tmp, ignore_errors=True)
            print(f"clip {name} {end - start:.1f}s: mp4 {os.path.getsize(mp4) // 1024} KB, webm {os.path.getsize(webm) // 1024} KB")
        br.close()


if __name__ == "__main__":
    main(set(sys.argv[1:]))
