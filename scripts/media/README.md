# Screenshots, clips and GIFs

Everything the README and the site show of the app is shot from the preview harness (`preview.html`,
sample data in `src/preview.tsx`) in headless Chromium. After a UI change, re-shoot it all with:

```sh
scripts/media/shoot.sh          # everything, about 8 minutes
scripts/media/shoot.sh shots    # screenshots and cards only, about a minute
scripts/media/shoot.sh clips    # site clips and GIFs only
```

It starts the harness on port 1441 if nothing is there (`PORT=` to change it), then writes, with the
same names and sizes as before:

| Script | Writes |
|---|---|
| `scripts/media/shots.py [name ...]` | `docs/screenshots/*.png` (1600x1000 window at 2x, so 3200x2000, dark) and `site/public/shots/*.webp` |
| `docs/readme/build_assets.py` | `docs/readme/cards/*.png` (crops of the screenshots, 2x), buttons, icons |
| `docs/readme/clips.py [name ...]` | `docs/readme/gifs/*.gif` and `site/public/clips/{compare,handoff,conductor,parallel,sky}.*` (2400x1500, 30 fps) |

Needs ffmpeg, gifski and cwebp (`brew install ffmpeg gifski webp`), and Python with Playwright and
Pillow (`pip install playwright pillow && playwright install chromium`). GitHub avatars are cached in
`$TMPDIR/nekotrail-avatars` on the first run. A story that clicks something the UI no longer has fails
loudly: fix its selector in the script, not the app. Then look at the results, and `cd site && bun run build`.
