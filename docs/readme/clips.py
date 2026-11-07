"""Records the README GIFs and the site clips from the preview harness, headless.

    bun run dev -- --port 1439                       # in another terminal
    python3 docs/readme/clips.py [name ...]          # record, then encode (all clips by default)
    python3 docs/readme/clips.py --encode [name ...] # encode again from the last recording

Each clip is one short story in the real UI (sample data from src/preview.tsx), at 1200x750 and device
scale 2, captured with Chrome's screencast (every painted frame, timestamped) and resampled to 30 fps.
A drawn pointer shows where the interaction happens, since headless Chrome has none. The last half
second fades back into the first frame, so every clip loops without a jump.

Writes:
- docs/readme/gifs/<name>.gif     : gifski, quality 96, 25 fps; the hero 1200 px wide, the rest 800 px,
                                    cropped to the feature, with the cards' rounded corners and margin
                                    (transparent, so it sits on GitHub's light and dark page alike).
- site/public/clips/<name>.mp4/.webm/.jpg : the whole window at 1600x1000, 30 fps (the clips marked site).

Needs Playwright (Python) with Chromium, Pillow, ffmpeg and gifski (brew install gifski).
Recordings are kept in $TMPDIR/nekotrail-clips/<name>/ (PNG frames at 2400x1500), so --encode is quick.
"""
import base64, glob, io, json, os, shutil, subprocess, sys, tempfile, time
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
BASE = os.environ.get("BASE", "http://localhost:1439/preview.html")
WORK = os.path.join(tempfile.gettempdir(), "nekotrail-clips")
W, H, DSF, FPS = 1200, 750, 2, 30
GIF_FPS, GIF_Q = 25, 96
GIF_EXTRA = os.environ.get("GIF_EXTRA", "--motion-quality 100 --lossy-quality 100").split()
SITE_W, SITE_H = 1600, 1000

# A macOS-style pointer, drawn into the page, that dips on click and leaves a soft ring.
POINTER = r"""
addEventListener('DOMContentLoaded', () => {
  const p = document.createElement('div');
  p.innerHTML = '<svg width="22" height="22" viewBox="0 0 22 22"><path d="M3 2 L3 18 L7.2 14 L10 20.5 L12.8 19.3 L10.1 13 L16 13 Z" fill="#000" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/></svg>';
  Object.assign(p.style, { position: 'fixed', left: '0', top: '0', zIndex: 2147483647, pointerEvents: 'none',
    transform: 'translate(-100px,-100px)', filter: 'drop-shadow(0 1px 1.5px rgba(0,0,0,.45))' });
  const svg = p.firstChild; svg.style.transition = 'transform .12s ease'; svg.style.transformOrigin = '3px 2px';
  document.documentElement.appendChild(p);
  addEventListener('mousemove', (e) => { p.style.transform = `translate(${e.clientX - 3}px, ${e.clientY - 2}px)`; }, true);
  addEventListener('mousedown', (e) => {
    svg.style.transform = 'scale(.82)';
    const r = document.createElement('div');
    Object.assign(r.style, { position: 'fixed', left: e.clientX - 14 + 'px', top: e.clientY - 14 + 'px', width: '28px', height: '28px',
      borderRadius: '50%', border: '2px solid rgba(204,204,255,.9)', zIndex: 2147483646, pointerEvents: 'none' });
    document.documentElement.appendChild(r);
    r.animate([{ transform: 'scale(.4)', opacity: 1 }, { transform: 'scale(1.5)', opacity: 0 }], { duration: 450, easing: 'ease-out' }).onfinish = () => r.remove();
  }, true);
  addEventListener('mouseup', () => { svg.style.transform = ''; }, true);
});
"""

# Lets a story change what the sample backend answers (window.__over[cmd] = (result, args) => result)
# and push events (window.__emit), the way the real backend would while agents work.
DIRECTOR = r"""
(() => {
  const T = {}; let inner;
  const wrapped = (cmd, args, opts) => Promise.resolve(inner(cmd, args, opts)).then((r) => {
    const f = (window.__over || {})[cmd]; return f ? f(r, args) : r; });
  Object.defineProperty(T, 'invoke', { configurable: true, get: () => wrapped, set: (v) => { inner = v; } });
  window.__TAURI_INTERNALS__ = T;
  window.__over = {};
  window.__emit = (event, payload) => inner('plugin:event|emit', { event, payload });
  // Glide the rows matching `sel` from where they were to where the next render puts them.
  window.__flipArm = (sel, key, ms = 650, bg = '') => {
    const k = (el) => (key ? el.querySelector(key) || el : el).textContent.trim();
    const before = new Map([...document.querySelectorAll(sel)].map((el) => [k(el), el.getBoundingClientRect()]));
    const mo = new MutationObserver(() => { mo.disconnect(); requestAnimationFrame(() => {
      for (const el of document.querySelectorAll(sel)) {
        const b = before.get(k(el));
        if (!b) { el.animate([{ opacity: 0, transform: 'scale(.97)' }, { opacity: 1, transform: 'none' }], { duration: ms, easing: 'cubic-bezier(.2,.8,.2,1)' }); continue; }
        const a = el.getBoundingClientRect(); const dx = b.left - a.left, dy = b.top - a.top;
        if (Math.abs(dx) + Math.abs(dy) < 1) continue;
        el.style.position = 'relative'; el.style.zIndex = '20'; if (bg) el.style.background = bg;
        el.animate([{ transform: `translate(${dx}px,${dy}px) scale(1.03)`, boxShadow: '0 10px 30px rgba(0,0,0,.45)' },
                    { transform: 'translate(0,0) scale(1)', boxShadow: '0 0 0 rgba(0,0,0,0)' }],
                   { duration: ms, easing: 'cubic-bezier(.3,.7,.2,1)' }).onfinish = () => { el.style.zIndex = ''; el.style.background = ''; };
      }
    }); });
    mo.observe(document.body, { subtree: true, childList: true, characterData: true });
  };
})();
"""

AV = os.environ.get("AVATARS")  # optional folder of <login>.png, so the profile photo doesn't wait on github.com


class Pointer:
    def __init__(self, pg, x, y):
        self.pg, self.x, self.y = pg, x, y
        self.start = (x, y)  # where it starts and ends, so the loop doesn't jump
        pg.mouse.move(x, y)

    def to(self, x, y, ms=600):
        # Paced by the clock, not by step count: each move costs a round trip at 2x.
        t0, t = time.monotonic(), 0
        while t < 1:
            t = min(1, (time.monotonic() - t0) * 1000 / ms)
            e = 1 - (1 - t) ** 3
            e = t * t * (3 - 2 * t) * 0.35 + e * 0.65  # ease in a little, ease out more
            self.pg.mouse.move(self.x + (x - self.x) * e, self.y + (y - self.y) * e)
            self.pg.wait_for_timeout(10)
        self.x, self.y = x, y

    def home(self, ms=600):
        self.to(*self.start, ms)

    def at(self, loc, ms=600, dx=0.5, dy=0.5):
        b = loc.first.bounding_box()
        self.to(b["x"] + b["width"] * dx, b["y"] + b["height"] * dy, ms)

    def click(self, loc, ms=600, pause=350, dx=0.5, dy=0.5):
        self.at(loc, ms, dx, dy)
        self.pg.wait_for_timeout(140)
        self.pg.mouse.down(); self.pg.wait_for_timeout(90); self.pg.mouse.up()
        self.pg.wait_for_timeout(pause)


def js(pg, code, arg=None):
    return pg.evaluate(code, arg)


# ---------------------------------------------------------------- stories

def hero_setup(pg):
    # Three agents at work; Dash's star is lit, not shining yet.
    js(pg, """() => {
      window.__over.snapshot = (r) => ({ ...r, running_workspaces: window.__running ?? ['w2', 'w3', 'w5'],
        shining: window.__shining ?? r.shining.filter((s) => s !== 'dash') });
      window.__emit('workspaces-changed', {});
    }""")
    pg.add_style_tag(content=".cards { overflow: visible !important; } .board-cols { overflow: visible !important; }")


PR = """([id, n, state]) => ({ workspace_id: id, pr: { number: n, url: 'https://github.com/pawprint/pawprint-api/pull/' + n,
  state, title: '', isDraft: false, baseRefName: 'main', additions: 12, deletions: 3, statusCheckRollup: [{ name: 'go test', conclusion: 'SUCCESS' }],
  latestReviews: [], reviewRequests: [] } })"""


def hero(pg, m):
    def move(id, n, state, running=None):
        js(pg, f"""([id, n, state, running]) => {{
          window.__flipArm('.board .card', '.card-title');
          if (running) window.__running = running;
          window.__emit('pr', ({PR})([id, n, state]));
          if (running) window.__emit('workspaces-changed', {{}});
        }}""", [id, n, state, running])
    pg.wait_for_timeout(700)
    move("w3", 486, "OPEN", ["w2", "w5"]); pg.wait_for_timeout(1100)  # Dash opens its PR
    move("w1", 482, "MERGED"); pg.wait_for_timeout(1000)              # Mochi merges
    move("w3", 486, "MERGED"); pg.wait_for_timeout(900)               # Dash merges
    m.click(pg.locator(".sidebar-foot").get_by_text("stars", exact=False), 800, 500)
    js(pg, "() => { window.__shining = undefined; window.__over.snapshot = (r) => ({ ...r, running_workspaces: ['w2', 'w5'] }); window.__emit('workspaces-changed', {}); }")
    pg.wait_for_timeout(1900)
    m.home(600); pg.wait_for_timeout(100)


def compare(pg, m):
    # Same task, Claude (Mochi) and Codex (Comet): read the numbers, keep the faster, cheaper one.
    pg.wait_for_timeout(400)
    claude, codex = pg.locator(".compare-col").nth(0), pg.locator(".compare-col").nth(1)
    m.at(claude.get_by_text("182s"), 600, dx=0.6); pg.wait_for_timeout(300)
    m.at(codex.get_by_text("141s"), 600, dx=0.6); pg.wait_for_timeout(300)
    m.at(codex.get_by_text("$0.88"), 400, dx=0.6); pg.wait_for_timeout(450)
    m.click(pg.get_by_role("button", name="Keep this one").nth(1), 600, 600)
    m.click(pg.get_by_role("button", name="Keep Comet"), 600, 1700)
    m.home(500); pg.wait_for_timeout(150)


HANDOFF_REPLY = [
    (400, {"kind": "tool", "id": "h2", "name": "Bash", "input": "git status --short && git diff --stat main"}),
    (600, {"kind": "tool_result", "id": "h2", "output": " M internal/server/http.go\n?? internal/server/http_test.go", "is_error": False}),
    (900, {"kind": "tool", "id": "h3", "name": "Edit", "input": "internal/server/config.go"}),
    (1050, {"kind": "tool_result", "id": "h3", "output": "", "is_error": False}),
    (1350, {"kind": "tool", "id": "h4", "name": "Bash", "input": "go test ./internal/server/..."}),
    (1650, {"kind": "tool_result", "id": "h4", "output": "ok  \tpawprint/internal/server\t0.388s", "is_error": False}),
    (2000, {"kind": "text", "text": "Picked up from Claude's change. `IdleTimeout` now comes from `Config`: 75s for the ALB listener, 360s for the NLB one, and the test injects a clock instead of sleeping 61s."}),
]


def handoff(pg, m):
    # Claude is out of usage; hand off to Codex, which carries on in the same branch.
    pg.wait_for_timeout(400)
    m.click(pg.get_by_role("button", name="Hand off to another agent").first, 700, 700)
    brief = pg.locator(".modal textarea, [role=dialog] textarea").first
    m.at(brief, 450, dy=0.6)
    for _ in range(6):
        pg.mouse.wheel(0, 60); pg.wait_for_timeout(50)
    pg.wait_for_timeout(300)
    m.click(pg.locator("button.primary", has_text="Hand off to").first, 600, 200)
    js(pg, """(steps) => {
      window.__emit('running', { running: true, workspace_id: 'w1' });
      for (const [ms, event] of steps) setTimeout(() => window.__emit('agent', { session_id: 's3', event }), ms);
    }""", HANDOFF_REPLY)
    pg.wait_for_timeout(2900)
    m.home(500); pg.wait_for_timeout(150)


def conductor(pg, m):
    # Import from Conductor, moving every worktree into NekoTrail's folder, each one checked after it moves.
    pg.wait_for_timeout(400)
    m.click(pg.locator(".cs-choice").nth(0), 700, 400)
    m.click(pg.locator(".modal button.primary, [role=dialog] button.primary").first, 700, 2200)
    lst = pg.locator(".cs-list").first
    m.at(lst, 400, dy=0.55)
    for _ in range(14):
        pg.mouse.wheel(0, 30); pg.wait_for_timeout(35)
    pg.wait_for_timeout(1800)
    m.home(500); pg.wait_for_timeout(150)


REVIEW_REPLY = [
    (400, {"kind": "tool", "id": "r1", "name": "Read", "input": "internal/server/config.go"}),
    (600, {"kind": "tool_result", "id": "r1", "output": "", "is_error": False}),
    (850, {"kind": "tool", "id": "r2", "name": "Edit", "input": "internal/server/http.go"}),
    (1000, {"kind": "tool_result", "id": "r2", "output": "", "is_error": False}),
    (1350, {"kind": "text", "text": "Done. `IdleTimeout` now reads `cfg.IdleTimeout`, with 75s as the default, so ops can set it per environment."}),
]


def review(pg, m):
    # Comment on a line in Changes, then Send review: the agent gets every comment in one message.
    js(pg, """(steps) => { window.__over.send = (r, args) => {
      window.__emit('agent', { session_id: args.id, event: { kind: 'user', text: args.text } });
      window.__emit('running', { running: true, workspace_id: 'w1' });
      for (const [ms, event] of steps) setTimeout(() => window.__emit('agent', { session_id: args.id, event }), ms);
      return r; }; }""", REVIEW_REPLY)
    pg.wait_for_timeout(400)
    m.click(pg.locator('button.add-comment[aria-label="Comment on line 22"]').first, 700, 200)
    pg.keyboard.type("Read this from config, so ops can tune it.", delay=12)
    pg.wait_for_timeout(250)
    m.click(pg.get_by_role("button", name="Add comment").first, 500, 500)
    m.click(pg.get_by_role("button", name="Send review").first, 700, 2400)
    m.home(500); pg.wait_for_timeout(150)


def queue_setup(pg):
    # The queue as the night starts: three running, nobody waiting on you yet.
    js(pg, """() => {
      window.__stage = 0;
      const now = () => Math.floor(Date.now() / 1000);
      window.__over.queue_view = (v) => {
        let tasks = v.tasks, asking = [];
        if (window.__stage >= 1) tasks = tasks.map((t) =>
          t.id === 'q1' ? { ...t, state: 'done', ended_at: now() - 5, note: 'Finished. Opened a draft PR.' }
          : t.id === 'q5' ? { ...t, state: 'running', ran_with: 'codex', started_at: now() - 2, workspace_id: 'w5' } : t);
        if (window.__stage >= 1) tasks = [...tasks.filter((t) => t.id !== 'q1'), tasks.find((t) => t.id === 'q1')];
        if (window.__stage >= 2) asking = ['q2'];
        return { ...v, tasks, asking };
      };
      window.__emit('task-queue', {});
    }""")


def queue(pg, m):
    def stage(n):
        js(pg, "(n) => { window.__flipArm('.tq-row', '.tq-title', 600, 'var(--surface)'); window.__stage = n; window.__emit('task-queue', {}); }", n)
    pg.wait_for_timeout(800)
    stage(1); pg.wait_for_timeout(1400)  # one finishes, the next in line starts: still 3 at once
    stage(2); pg.wait_for_timeout(900)   # one stops to ask: Needs you, in lantern gold
    row = pg.locator(".tq-row", has_text="Add rate limiting")
    m.at(row.locator(".tq-status"), 600); pg.wait_for_timeout(500)
    m.click(row.get_by_role("button", name="Open"), 500, 1700)
    m.home(500); pg.wait_for_timeout(150)


# Feature GIFs are 800 px wide at 16:10, about 0.76 of the app's size: the modal stories crop the
# middle of a 1200x750 window (a sheet needs that height), the others run in a 1056x660 window.
# The GIFs of those hide the sidebar, which the crop would cut in half; the site's clips (site=True)
# are recorded again with it, as the whole 1200x750 window.
MODAL_CROP = (72, 45, 1056, 660)
CLIPS = {
    # q: preview query; vw, vh: window; crop: GIF crop in CSS px (None: the whole window); gw: GIF width
    "hero":      dict(q="view=board", setup=hero_setup, story=hero, gw=1200),
    "compare":   dict(q="ws=w2&compare=p1", story=compare, crop=MODAL_CROP, site=True),
    "handoff":   dict(q="ws=w1&limit", story=handoff, crop=MODAL_CROP, site=True),
    "conductor": dict(q="rescue=import", story=conductor, crop=MODAL_CROP, site=True),
    "review":    dict(q="ws=w1&tab=changes&noreview&nosidebar", story=review, vw=1056, vh=660),
    "queue":     dict(q="view=queue&ws=w2", setup=queue_setup, story=queue, vw=1056, vh=660),
}
for c in CLIPS.values():
    c.setdefault("setup", None); c.setdefault("crop", None); c.setdefault("gw", 800)
    c.setdefault("vw", W); c.setdefault("vh", H); c.setdefault("site", False)


def spec(key):
    """What to record for a GIF (`name`) or a site clip (`name@site`): query, setup, story, window."""
    c = CLIPS[key.split("@")[0]]
    if key.endswith("@site"):
        return c["q"], c["setup"], c["story"], W, H
    return c["q"] + ("&nosidebar" if c["site"] else ""), c["setup"], c["story"], c["vw"], c["vh"]


# ---------------------------------------------------------------- recording

def record(names):
    from playwright.sync_api import sync_playwright
    with sync_playwright() as p:
        # Without the flag the screencast sends 1x frames even at device scale 2.
        br = p.chromium.launch(headless=True, args=[f"--force-device-scale-factor={DSF}"])
        for name in [k for n in names for k in ([n, n + "@site"] if CLIPS[n]["site"] else [n])]:
            q, setup, story, vw, vh = spec(name)
            out = os.path.join(WORK, name)
            shutil.rmtree(out, ignore_errors=True); os.makedirs(os.path.join(out, "raw"))
            ctx = br.new_context(viewport={"width": vw, "height": vh}, device_scale_factor=DSF, color_scheme="dark")
            ctx.add_init_script(DIRECTOR)
            ctx.add_init_script(POINTER)
            if AV:
                def serve(route):
                    f = os.path.join(AV, route.request.url.split("github.com/")[1].split(".png")[0] + ".png")
                    route.fulfill(path=f, content_type="image/png") if os.path.exists(f) else route.continue_()
                ctx.route("https://github.com/*.png*", serve)
            pg = ctx.new_page()
            pg.goto(f"{BASE}?{q}&theme=dark&zoom=1")
            pg.wait_for_timeout(2500)
            try:
                pg.wait_for_function("Array.from(document.images).every(i => i.complete)", timeout=8000)
            except Exception:
                pass
            m = Pointer(pg, vw * 0.62, vh * 0.72)
            if setup:
                setup(pg)
            pg.wait_for_timeout(800)
            cdp = ctx.new_cdp_session(pg)
            frames = []
            def on_frame(f):
                frames.append((f["metadata"]["timestamp"], f["data"]))
                try:
                    cdp.send("Page.screencastFrameAck", {"sessionId": f["sessionId"]})
                except Exception:
                    pass  # the page closed with a frame in flight
            cdp.on("Page.screencastFrame", on_frame)
            cdp.send("Page.startScreencast", {"format": "png", "maxWidth": vw * DSF, "maxHeight": vh * DSF, "everyNthFrame": 1})
            # Nudge a repaint so the first frame arrives at t=0.
            m.to(m.x + 1, m.y, 50)
            story(pg, m)
            pg.wait_for_timeout(150)
            cdp.send("Page.stopScreencast")
            ctx.close()
            assert Image.open(io.BytesIO(base64.b64decode(frames[0][1]))).width == vw * DSF, "screencast is not at 2x"
            t0 = frames[0][0]
            index = []
            for i, (t, data) in enumerate(frames):
                path = os.path.join(out, "raw", f"{i:05d}.png")
                open(path, "wb").write(base64.b64decode(data))
                index.append([t - t0, path])
            json.dump(index, open(os.path.join(out, "raw.json"), "w"))
            print(f"{name}: {len(frames)} painted frames over {index[-1][0]:.1f}s")
        br.close()


def resample(name, fps):
    """Constant-rate frame list: the newest painted frame at each tick."""
    index = json.load(open(os.path.join(WORK, name, "raw.json")))
    n = int(index[-1][0] * fps)
    seq, j = [], 0
    for k in range(n):
        t = k / fps
        while j + 1 < len(index) and index[j + 1][0] <= t:
            j += 1
        seq.append(index[j][1])
    return seq


def frames_for(name, fps, size, crop=None, rounded=0, pad=0):
    """PNG frames in a temp dir at `size`, with the loop fade baked in. For GIFs, rounded corners and a
    transparent `pad` on every side, so a GIF lines up with the framed PNG cards next to it."""
    seq = resample(name, fps)
    tmp = tempfile.mkdtemp()
    fade = int(fps * 0.5)
    first = mask = None
    cache = {}
    for k, src in enumerate(seq):
        if src not in cache:
            im = Image.open(src).convert("RGB")
            if crop:
                x, y, w, h = crop
                im = im.crop((x * DSF, y * DSF, (x + w) * DSF, (y + h) * DSF))
            cache.clear(); cache[src] = im.resize(size, Image.LANCZOS)
        im = cache[src]
        if first is None:
            first = im
        if k >= len(seq) - fade:
            a = (k - (len(seq) - fade) + 1) / fade
            im = Image.blend(im, first, a * a * (3 - 2 * a))
        if rounded:
            if mask is None:
                mask = Image.new("L", size, 0)
                ImageDraw.Draw(mask).rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], rounded, fill=255)
            im = im.convert("RGBA"); im.putalpha(mask)
        if pad:
            canvas = Image.new("RGBA", (size[0] + 2 * pad, size[1] + 2 * pad), (0, 0, 0, 0))
            canvas.paste(im, (pad, pad))
            im = canvas
        im.save(os.path.join(tmp, f"{k:05d}.png"), compress_level=1)
    return tmp, len(seq)


def encode(names):
    os.makedirs(os.path.join(HERE, "gifs"), exist_ok=True)
    for name in names:
        c = CLIPS[name]
        crop, gw = c["crop"], c["gw"]
        cw, ch = (crop[2], crop[3]) if crop else (c["vw"], c["vh"])
        size = (gw, round(gw * ch / cw))
        # The same corner and margin, relative to width, as build_assets.py gives the cards (18 and 22 on 1040).
        pad = round(22 * gw / 1040)
        tmp, n = frames_for(name, GIF_FPS, size, crop, rounded=round(18 * gw / 1040), pad=pad)
        size = (size[0] + 2 * pad, size[1] + 2 * pad)
        out = os.path.join(HERE, "gifs", f"{name}.gif")
        # gifski shrinks to about 800 px unless told the size.
        subprocess.run(["gifski", "--quiet", "--fps", str(GIF_FPS), "--quality", str(GIF_Q), *GIF_EXTRA, "--repeat", "0",
                        "--width", str(size[0]), "--height", str(size[1]), "-o", out,
                        *sorted(glob.glob(os.path.join(tmp, "*.png")))], check=True)
        shutil.rmtree(tmp)
        print(f"gifs/{name}.gif {size[0]}x{size[1]} {n} frames @ {GIF_FPS} fps, {os.path.getsize(out) / 1e6:.2f} MB")
        if c["site"]:
            site(name)


def site(name):
    tmp, n = frames_for(name + "@site", FPS, (SITE_W, SITE_H))
    clips = os.path.join(REPO, "site", "public", "clips")
    src = ["-framerate", str(FPS), "-i", os.path.join(tmp, "%05d.png")]
    ff = lambda *a: subprocess.run(["ffmpeg", "-v", "error", "-y", *a], check=True)
    ff(*src, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-preset", "veryslow", "-tune", "animation",
       "-movflags", "+faststart", "-an", os.path.join(clips, f"{name}.mp4"))
    ff(*src, "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-b:v", "0", "-crf", "30", "-row-mt", "1", "-an",
       os.path.join(clips, f"{name}.webm"))
    Image.open(os.path.join(tmp, "00000.png")).convert("RGB").save(os.path.join(clips, f"{name}.jpg"), quality=88)
    shutil.rmtree(tmp)
    print(f"site clips/{name}.mp4 {os.path.getsize(os.path.join(clips, name + '.mp4')) // 1024} KB, "
          f".webm {os.path.getsize(os.path.join(clips, name + '.webm')) // 1024} KB")


if __name__ == "__main__":
    args = sys.argv[1:]
    only_encode = "--encode" in args
    names = [a for a in args if not a.startswith("--")] or list(CLIPS)
    if not only_encode:
        record(names)
    encode(names)
