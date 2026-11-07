"""Shared bits for the media scripts: where the preview harness is, and GitHub avatars served locally."""
import os, tempfile, urllib.request

BASE = os.environ.get("BASE", "http://localhost:1441/preview.html")
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
AVATARS = os.path.join(tempfile.gettempdir(), "nekotrail-avatars")


def avatars(ctx):
    """Serve github.com/<login>.png from a local cache, fetched once: github.com can be slow from a
    headless browser, and an empty circle in a screenshot looks broken."""
    os.makedirs(AVATARS, exist_ok=True)

    def serve(route):
        login = route.request.url.split("github.com/")[1].split(".png")[0].split("?")[0]
        f = os.path.join(AVATARS, f"{login}.png")
        if not os.path.exists(f):
            try:
                with urllib.request.urlopen(f"https://github.com/{login}.png?size=96", timeout=15) as r:
                    data = r.read()
                open(f, "wb").write(data)
            except Exception:
                return route.abort()  # the app falls back to initials
        route.fulfill(path=f, content_type="image/png")

    ctx.route("https://github.com/*.png*", serve)
