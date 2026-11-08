"""Builds the README images in docs/readme/ from the app screenshots, and the animated GIFs.

    python3 docs/readme/build_assets.py            # cards + buttons + icons
    python3 docs/readme/build_assets.py --gifs     # also records and encodes every GIF (about 4 minutes)
    python3 docs/readme/build_assets.py --gifs hero review   # just these GIFs

Needs Pillow. After re-shooting docs/screenshots/*.png (3200x2000, dark theme), run this and every README
image is redrawn with the same file names. --gifs needs the preview harness running
(`bun run dev -- --port 1441`), Playwright with Chromium, ffmpeg and gifski: see clips.py.

- cards/<name>.png : each screenshot cropped to its feature, all the same size (2168x1388, 2x), rounded corners,
  a hairline edge (so a dark card still reads on GitHub's dark page) and a soft shadow (for the light page).
- gifs/<name>.gif  : short loops of the real UI (clips.py): the hero at 1600 px, the Tour's features at 1200 px.
- buttons/, icons/ : SVGs in a -light and a -dark variant, picked by <picture> in the README.
"""
import os, subprocess, sys
from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SHOTS = os.path.join(REPO, "docs", "screenshots")

S = 2              # the screenshots are 2x (3200x2000); crops below are in 1600x1000 CSS px
W, H = 2080, 1300  # card image (16:10), 2x for sharp text on retina screens
PAD = 44           # room for the shadow, same on every side
R = 36             # corner radius

# name: (x, y, w, h) in the 1600x1000 source, all 16:10 (sidebar 264px wide)
CROPS = {
    "new-workspace": (315, 150, 1000, 625),
    "plan":          (440, 400, 960, 600),
    "changes":       (260, 60, 1120, 700),
    "rewind":        (285, 110, 1000, 625),
    "preview":       (264, 0, 1120, 700),
    "compare":       (265, 200, 1040, 650),
    "scorecard":     (285, 200, 1000, 625),
    "night-sky":     (264, 10, 1216, 760),
    "model-picker":  (245, 375, 1000, 625),
    "opencode":      (390, 225, 1040, 650),
    "stacked":       (264, 0, 1336, 835),
    "board":         (264, 0, 1300, 812),
    "storage":       (285, 180, 1000, 625),
    "palette":       (265, 130, 1040, 650),
    "mcp":           (352, 55, 1248, 780),
    "welcome":       (420, 60, 1040, 650),
    "prompt-rail":   (480, 0, 1120, 700),
    "environment":   (0, 0, 1600, 1000),
    "search":        (285, 120, 1040, 650),
    "archive":       (0, 0, 1000, 625),
}

# Buttons and icons. Neutral greys from GitHub's own UI, plus the brand's Moonlight for the one
# primary button (brand/palette.json): change these two lines if the palette changes.
THEMES = {
    "light": {"primary": ("#3D3D85", "#3D3D85", "#ffffff"), "secondary": ("#f6f8fa", "#d0d7de", "#1f2328"),
              "heart": "#bf3989", "tile": ("#f6f8fa", "#d0d7de"), "glyph": "#1f2328"},
    "dark": {"primary": ("#CCCCFF", "#CCCCFF", "#1A1A4A"), "secondary": ("#21262d", "#3d444d", "#f0f6fc"),
             "heart": "#db61a2", "tile": ("#161b22", "#30363d"), "glyph": "#e6edf3"},
}
FONT = ('font-family="-apple-system, BlinkMacSystemFont, &quot;Segoe UI&quot;, &quot;Noto Sans&quot;, '
        'Helvetica, Arial, sans-serif"')
DOWNLOAD = ('<path d="M8 2.5v7.5M4.75 7 8 10.25 11.25 7M2.75 13.25h10.5" fill="none" stroke="{c}" '
            'stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>')
GLOBE = ('<g fill="none" stroke="{c}" stroke-width="1.5" stroke-linecap="round"><circle cx="8" cy="8" r="6.25"/>'
         '<path d="M1.75 8h12.5M8 1.75c-2.2 2.4-2.2 10.1 0 12.5M8 1.75c2.2 2.4 2.2 10.1 0 12.5"/></g>')
# Heart: Primer Octicons heart-fill-16 (MIT, github.com/primer/octicons), as in docs/sponsor/github-sponsors.svg.
HEART = ('<path fill="{c}" d="M7.655 14.916v-.001h-.002l-.006-.003-.018-.01a22.066 22.066 0 0 1-3.744-2.584C2.045 '
         '10.731 0 8.35 0 5.5 0 2.836 2.086 1 4.25 1 5.797 1 7.153 1.802 8 3.02 8.847 1.802 10.203 1 11.75 1 13.914 1 '
         '16 2.836 16 5.5c0 2.85-2.044 5.231-3.886 6.818a22.094 22.094 0 0 1-3.433 2.414 7.152 7.152 0 0 '
         '1-.31.17l-.018.01-.008.004a.75.75 0 0 1-.69 0Z"/>')
ICONS = {  # 24-grid line icons
    "code": '<path d="M8.5 7 3.5 12l5 5M15.5 7l5 5-5 5M13.5 4.5l-3 15"/>',
    "key": '<circle cx="8" cy="15" r="4.5"/><path d="M11.2 11.8 19.5 3.5M16.5 6.5l2.5 2.5M14 9l2 2"/>',
    "handoff": '<path d="M4 8.5h13.5M14 5l3.5 3.5L14 12M20 15.5H6.5M10 12l-3.5 3.5L10 19"/>',
    "shield": ('<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.2 7.5 9.5 4.3-1.3 7.5-4.9 7.5-9.5V6L12 3Z"/>'
               '<path d="m8.75 12.25 2.25 2.25 4.25-4.5"/>'),
}


def frame(img, r=R):
    """RGB image -> RGBA with rounded corners, hairline edge and soft shadow, PAD px on every side."""
    w, h = img.size
    canvas = Image.new("RGBA", (w + 2 * PAD, h + 2 * PAD), (0, 0, 0, 0))
    sh = Image.new("L", canvas.size, 0)
    ImageDraw.Draw(sh).rounded_rectangle([PAD, PAD + 12, PAD + w - 1, PAD + h - 1 + 12], r, fill=70)
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 255))
    shadow.putalpha(sh.filter(ImageFilter.GaussianBlur(22)))
    canvas = Image.alpha_composite(canvas, shadow)
    mask = Image.new("L", (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, w - 1, h - 1], r, fill=255)
    card = img.convert("RGBA")
    card.putalpha(mask)
    canvas.alpha_composite(card, (PAD, PAD))
    edge = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle([PAD, PAD, PAD + w - 1, PAD + h - 1], r,
                                           outline=(255, 255, 255, 34), width=3)
    return Image.alpha_composite(canvas, edge)


def cards():
    os.makedirs(os.path.join(HERE, "cards"), exist_ok=True)
    for name, (x, y, w, h) in CROPS.items():
        im = Image.open(os.path.join(SHOTS, name + ".png")).convert("RGB")
        im = im.crop((x * S, y * S, (x + w) * S, (y + h) * S)).resize((W, H), Image.LANCZOS)
        out = os.path.join(HERE, "cards", name + ".png")
        frame(im).save(out, optimize=True)
        print(f"cards/{name}.png {os.path.getsize(out) // 1024} KB")


def button(name, label, glyph, kind, text_len, width, theme, glyph_color=None):
    fill, stroke, text = THEMES[theme][kind]
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="40" viewBox="0 0 {width} 40" role="img" aria-label="{label}">
  <title>{label}</title>
  <rect x="0.5" y="0.5" width="{width - 1}" height="39" rx="8" fill="{fill}" stroke="{stroke}"/>
  <g transform="translate(16 12)">{glyph.format(c=glyph_color or text)}</g>
  <text x="42" y="25.5" {FONT} font-size="15" font-weight="600" fill="{text}" textLength="{text_len}" lengthAdjust="spacingAndGlyphs">{label}</text>
</svg>
'''
    open(os.path.join(HERE, "buttons", f"{name}-{theme}.svg"), "w").write(svg)


def icon(name, body, theme):
    tile_fill, tile_stroke = THEMES[theme]["tile"]
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="56" height="56" viewBox="0 0 56 56" aria-hidden="true">
  <rect x="0.5" y="0.5" width="55" height="55" rx="14" fill="{tile_fill}" stroke="{tile_stroke}"/>
  <g transform="translate(14 14) scale(1.1667)" fill="none" stroke="{THEMES[theme]["glyph"]}" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{body}</g>
</svg>
'''
    open(os.path.join(HERE, "icons", f"{name}-{theme}.svg"), "w").write(svg)


def svgs():
    os.makedirs(os.path.join(HERE, "buttons"), exist_ok=True)
    os.makedirs(os.path.join(HERE, "icons"), exist_ok=True)
    for t in THEMES:
        button("download-mac", "Download for Mac", DOWNLOAD, "primary", 130, 190, t)
        button("download-linux", "Linux", DOWNLOAD, "secondary", 40, 100, t)
        button("website", "Website", GLOBE, "secondary", 60, 120, t)
        button("sponsor", "Sponsor on GitHub", HEART, "secondary", 140, 200, t, THEMES[t]["heart"])
        for n, b in ICONS.items():
            icon(n, b, t)
    print("buttons/ and icons/ written")


if __name__ == "__main__":
    svgs()
    cards()
    if "--gifs" in sys.argv:
        names = [a for a in sys.argv[1:] if not a.startswith("--")]
        subprocess.run([sys.executable, os.path.join(HERE, "clips.py"), *names], check=True)
