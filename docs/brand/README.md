# Vice Mail brand

The mark is a bright orange V over a dark envelope. As the app icon it sits in a near-black
rounded square with a glowing orange rim. Every file here is vector artwork drawn from one set of
shapes in `make_svgs.py`. The script uses only the Python standard library, and its letters are
drawn as outlines, so no font is involved.

| File | Use |
|---|---|
| `icon.svg` | App icon, full detail, on the macOS icon grid (the same file as `app/src-tauri/icons/source.svg`) |
| `mark.svg` | The envelope and V without the square, for headers, the website and documents |
| `glyph-mono.svg` | Single-colour glyph (`currentColor`) for tray, notification or menu use. On macOS, export it as a black template image |
| `wordmark-dark.svg` | Mark and "Vice Mail" for dark backgrounds ("Vice" white) |
| `wordmark-light.svg` | Mark and "Vice Mail" for light backgrounds ("Vice" near-black) |
| `make_svgs.py` | Writes all of the above plus the icon sources in `app/src-tauri/icons/` |

To change anything, edit `make_svgs.py`, then run `app/src-tauri/icons/generate.sh`. That script
runs `make_svgs.py` itself and then rebuilds every PNG, `icon.icns` and `icon.ico` from the SVGs.

## Colours

| Name | Hex | Where |
|---|---|---|
| Ember | `#FFC21F` | top of the V, outer edge of the rim |
| Vice orange | `#FF7A00` | the V, the envelope outline, "Mail" (primary brand colour) |
| Deep orange | `#F04E00` | bottom of the V and of the outlines |
| Night | `#0E0A0B` | icon background |
| Charcoal | `#1B1A1F` | envelope |
| Steel | `#5C5D68` | top flap corners |
| Ink | `#1A1619` | "Vice" on light backgrounds |
| White | `#FFFFFF` | "Vice" on dark backgrounds |

"Mail" runs from `#FF8C00` at the top to `#F45800` at the baseline. On flat media, such as
one-colour print or embroidery, use Vice orange alone.

## Clear space and minimum sizes

- **Wordmark:** keep at least one cap height (the height of the "V" in "Vice") free on every
  side. Use it at 28 px high or more on screen, and 12 mm or more in print. Below that, use the
  mark or the app icon.
- **Mark and app icon:** keep a quarter of their height free around them. Show the mark at
  24 px or more.
- **App icon at small sizes:** from 16 to 48 px, the app uses the simplified sources
  (`source-16.svg` to `source-48.svg`, and `source-macos-16.svg` and `source-macos-32.svg`). They
  have no glow, use thicker lines, and are aligned to the pixel grid. Use these whenever the icon
  is shown below 64 px.
- **Windows sizes:** Windows picks the icon entry by display scale: the taskbar uses 24, 30, 36
  and 48 px at 100, 125, 150 and 200 %, the title bar and small icons 16, 20, 24 and 32 px, and
  Start and the window icon 32, 40, 48 and 64 px. Each of 16, 20, 24, 30, 32, 36, 40 and 48 px
  has its own source drawn on that size's pixel grid; a frame scaled from a neighbouring size
  looks blurry. The plate fills 87 to 92 % of the canvas (16 px: all of it), so the square sits
  in the taskbar at the optical size of the Windows 11 icons around it.
- **Mono glyph:** 16 px or more.

## Do

- Put the wordmark on plain backgrounds: white, light grey, near-black, or the dark app chrome.
- Scale the files as they are. They are vectors.
- Use the app icon, with its square, wherever the app itself is meant, such as download buttons
  or store listings.

## Don't

- Don't recolour the V or the envelope, apart from the mono glyph, which takes any single colour.
- Don't set "Vice Mail" in a font. Use the outlined wordmark.
- Don't add the smoke, heavy glows, outlines or drop shadows from the original sketches.
- Don't stretch the files, rotate them, or put them on busy photos.
- Don't put the wordmark on orange backgrounds, where "Mail" disappears.

## Origin

The artwork was redrawn on 23 Nov 2026 from the human's reference images, which stay outside the
repository. Deliberate differences from those images:

- The icon follows the macOS grid (an 824 px body in 1024 px, continuous corners, a soft shadow),
  and the Windows icon fills 88 % of its canvas (94 % looked too heavy next to Windows 11 icons).
- The glow is lighter, so the icon stays crisp.
- The wordmark uses the app icon's mark, without the extra orange frame of the sketch.
