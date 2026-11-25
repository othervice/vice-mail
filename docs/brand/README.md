# Vice Mail brand

The mark is a bright blue V over a dark envelope. As the app icon it sits in a near-black
rounded square with a glowing blue rim. The blues are GitHub's, the same family as the app's own
colour scheme (decision 0053); the human approved the blue logo on 25 Nov 2026 (decision 0068),
replacing the orange of the first artwork. Every file here is vector artwork drawn from one set of
shapes in `make_svgs.py`. The script uses only the Python standard library, and its letters are
drawn as outlines, so no font is involved.

| File | Use |
|---|---|
| `icon.svg` | App icon, full detail, on the macOS icon grid (the same file as `app/src-tauri/icons/source.svg`) |
| `mark.svg` | The envelope and V without the square, for headers, the website and documents |
| `glyph-mono.svg` | Single-colour glyph (`currentColor`) for tray, notification or menu use. The macOS menu-bar icon is this glyph as a black template image (`app/src-tauri/icons/tray/macos-template.png`) |
| `wordmark-dark.svg` | Mark and "Vice Mail" for dark backgrounds ("Vice" white) |
| `wordmark-light.svg` | Mark and "Vice Mail" for light backgrounds ("Vice" near-black) |
| `make_svgs.py` | Writes all of the above plus the icon sources in `app/src-tauri/icons/` |

To change anything, edit `make_svgs.py`, then run `scripts/icons.sh`. That script runs
`make_svgs.py` itself and then rebuilds every PNG, `icon.icns`, `icon.ico` and the menu-bar
template from the SVGs. The app's inline copy of icon and wordmark
(`app/src/components/Brand.tsx`) is updated by hand from `icon.svg` and the wordmark;
`app/tests/brand.test.tsx` fails while its colours differ from the files here.

## Colours

| Name | Hex | Where |
|---|---|---|
| Sky | `#A5D6FF` | top of the V, outer edge of the rim |
| Vice blue | `#4493F8` | the V, the envelope outline, "Mail" (primary brand colour) |
| Deep blue | `#1F6FEB` | bottom of the V and of the outlines |
| Night | `#0D1117` | icon background (GitHub dark) |
| Charcoal | `#161B22` | envelope |
| Steel | `#59636E` | top flap corners |
| Ink | `#1F2328` | "Vice" on light backgrounds |
| White | `#FFFFFF` | "Vice" on dark backgrounds |

"Mail" runs from `#4493F8` at the top to `#1F6FEB` at the baseline. On flat media, such as
one-colour print or embroidery, use Vice blue alone. The gradients in between use GitHub's blue
scale (`#CAE8FF`, `#B6E3FF`, `#79C0FF`, `#6CB6FF`, `#58A6FF`, `#388BFD`, `#2F81F7`, `#1A62DC`,
`#1158C7`); `make_svgs.py` lists every stop.

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
- **Mono glyph:** 16 px or more. In the macOS menu bar it is 18 pt high (36 px on Retina).
- **Notification area (Windows):** the app icon's own 16, 20, 24 and 32 px entries, by display
  scale, with the unread count drawn by the app (decision 0068).

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
- Don't put the wordmark on blue backgrounds, where "Mail" disappears.

## Origin

The artwork was redrawn on 23 Nov 2026 from the human's reference images, which stay outside the
repository, and recoloured from orange to blue on 25 Nov 2026 (decision 0068; same shapes, every
colour mapped to its GitHub counterpart). Deliberate differences from the reference images:

- The icon follows the macOS grid (an 824 px body in 1024 px, continuous corners, a soft shadow),
  and the Windows icon fills 88 % of its canvas (94 % looked too heavy next to Windows 11 icons).
- The glow is lighter, so the icon stays crisp.
- The wordmark uses the app icon's mark, without the extra orange frame of the sketch.
