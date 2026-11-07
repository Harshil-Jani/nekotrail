# NekoTrail brand kit

The one source of truth for how NekoTrail looks. Everything here is drawn by
[`scripts/brand.ts`](../scripts/brand.ts) from the geometry in that file and the colours in
[`palette.json`](palette.json). Change either, run `bun scripts/brand.ts`, and every copy updates:
the app icons, the app favicon, the site (favicon, header, Open Graph image, colour tokens), the
VS Code extension icons and the GitHub social preview. The site serves this folder as a zip at
`/brandkit/`.

## Assets

| File | Use |
|---|---|
| `mark/nekotrail-mark.svg` / `.png` | The letter mark: the N drawn as a cat. Full colour, any background. |
| `mark/nekotrail-mark-dark.svg` / `.png` | One colour, Night. Light backgrounds, print, stamps. |
| `mark/nekotrail-mark-light.svg` / `.png` | One colour, white. Dark backgrounds and photos. |
| `wordmark/nekotrail-wordmark-dark.svg` / `.png` | The wordmark in Night with the orange star. Light backgrounds. |
| `wordmark/nekotrail-wordmark-light.svg` / `.png` | The wordmark in white with the orange star. Dark backgrounds. |
| `lockup/nekotrail-lockup-dark.svg` / `.png` | Mark plus wordmark. Light backgrounds. |
| `lockup/nekotrail-lockup-light.svg` / `.png` | Mark plus wordmark. Dark backgrounds. |
| `icon/nekotrail-icon.svg`, `-1024.png`, `-512.png` | The macOS app icon. |
| `mascot/nekotrail-cat-idle.svg`, `-happy.svg` (+ PNG) | The cat. For stickers, empty states and talks. |
| `social-preview.png` | 1280 x 640, the GitHub repository social preview. |
| `palette.json` | Colours, with names and where each one is used. |
| `wordmark/geometry.json` | The wordmark's stroke paths (the site's hero draws itself from these). |

## The wordmark

NekoTrail is written in one monoline stroke with round ends, built only from straight lines and
circles, so every letter is a little constellation. The i is dotted with a star. It is drawn, not
typed: never set the name in a font and call it the logo.

Always **NekoTrail**, capital N and capital T. Lowercase only in the CLI command, paths, URLs and
package ids (`nekotrail`).

## Typography

- Interface and site text: the system font (SF Pro on Apple platforms), as in the app.
- Code, branches, paths: SF Mono (`ui-monospace`).
- Headings: SF Pro Display, semibold or bold, tracking -0.02em to -0.035em.

## Clear space and minimum size

- Around the wordmark and the lockup, keep clear space equal to the height of the lowercase **e**
  (the x-height).
- Around the mark, keep a quarter of its width clear on every side.
- Minimum sizes: mark 16 px, wordmark 80 px wide, lockup 120 px wide. Below that use the mark alone.

## Do

- Use the dark variants on light backgrounds and the light variants on dark ones.
- Keep the orange star on the i.
- Scale proportionally from the SVG.

## Don't

- Recolour, outline, add shadows or gradients to the mark or wordmark.
- Stretch, rotate or rearrange the letters, or swap the star for another shape.
- Set "NekoTrail" in a typeface and use it as the logo.
- Put the mark on a busy photo without enough contrast.
- Combine our mark with another product's logo into one shape.
