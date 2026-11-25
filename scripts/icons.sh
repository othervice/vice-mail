#!/usr/bin/env bash
# Regenerates every app icon from the brand artwork, in one command:
#
#   scripts/icons.sh
#
# The artwork is docs/brand/make_svgs.py (shapes and colours, standard-library Python). A new
# artwork or new colours means editing that script and running this one; nothing else is drawn by
# hand. Steps:
#
# 1. docs/brand/make_svgs.py writes the SVG sources (app/src-tauri/icons/source*.svg and the brand
#    files in docs/brand: icon, mark, mono glyph, wordmarks).
# 2. `tauri icon` (resvg) rasterises each SVG at the sizes it is meant for:
#    - source.svg            full-detail master on the macOS icon grid (824 px body in 1024, shadow):
#                            macOS 64 px and up, and the PNG files tauri.conf.json lists
#    - source-macos-16.svg, source-macos-32.svg
#                            simplified icon for the small macOS sizes (Finder lists, menus)
#    - source-windows.svg    the master framed for Windows (body fills 88 %, no shadow): 60 px and up
#    - source-16/20/24/30/32/36/40/48.svg
#                            simplified icon, each drawn on its own pixel grid and rendered only at
#                            its own size (never scaled from a neighbour): taskbar (24/30/36/48 at
#                            100/125/150/200 %), title bar, notification area and small icons
#                            (16/20/24/32), Start and the window icon (32/40/48/64)
#    - docs/brand/glyph-mono.svg
#                            the macOS menu-bar icon (a template image: black with alpha, 36 px for
#                            the 18 pt the menu bar gives it, decision 0069)
# 3. icon.icns is assembled with iconutil, icon.ico (PNG entries, Windows Vista and later) by the
#    Python below. The macOS disk image takes its volume icon from the app's icon.icns
#    (scripts/build-macos.sh), the NSIS installer uses icon.ico (tauri.conf.json).
# 4. The app's inline artwork (app/src/components/Brand.tsx) is not rewritten here; a Vitest test
#    (app/tests/brand.test.tsx) fails when its colours are not the ones of docs/brand.
#
# Needs macOS (iconutil), pnpm with the app's node_modules installed, and python3 (standard library).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/app"
icons="$app/src-tauri/icons"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

python3 -I "$root/docs/brand/make_svgs.py" >/dev/null

# render <svg> <name> <sizes>: writes $tmp/<name>/<size>x<size>.png. `tauri icon` lists every file
# it writes; its output is shown only when it fails.
render() {
  local log
  if ! log="$(pnpm --dir "$app" tauri icon "$1" -o "$tmp/$2" -p "$3" 2>&1)"; then
    printf '%s\n' "$log" >&2
    exit 1
  fi
}

render "$icons/source.svg" mac 64,128,256,512,1024
render "$icons/source-macos-16.svg" mac16 16
render "$icons/source-macos-32.svg" mac32 32
render "$icons/source-windows.svg" win 60,64,72,80,96,128,256
for size in 16 20 24 30 32 36 40 48; do
  render "$icons/source-$size.svg" "w$size" "$size"
done
render "$root/docs/brand/glyph-mono.svg" tray 36

# The PNG files tauri.conf.json bundles (Linux, default window icon outside Windows and macOS dev).
cp "$tmp/w32/32x32.png" "$icons/32x32.png"
cp "$tmp/mac/128x128.png" "$icons/128x128.png"
cp "$tmp/mac/256x256.png" "$icons/128x128@2x.png"
cp "$tmp/mac/512x512.png" "$icons/icon.png"
# macOS menu bar (decision 0069): embedded by the shell with `include_image!`.
mkdir -p "$icons/tray"
cp "$tmp/tray/36x36.png" "$icons/tray/macos-template.png"

# icon.icns: the simplified icon at 16 and 32 px, the master from 64 px (32@2x) up.
iconset="$tmp/icon.iconset"
mkdir "$iconset"
cp "$tmp/mac16/16x16.png" "$iconset/icon_16x16.png"
cp "$tmp/mac32/32x32.png" "$iconset/icon_16x16@2x.png"
cp "$tmp/mac32/32x32.png" "$iconset/icon_32x32.png"
cp "$tmp/mac/64x64.png" "$iconset/icon_32x32@2x.png"
cp "$tmp/mac/128x128.png" "$iconset/icon_128x128.png"
cp "$tmp/mac/256x256.png" "$iconset/icon_128x128@2x.png"
cp "$tmp/mac/256x256.png" "$iconset/icon_256x256.png"
cp "$tmp/mac/512x512.png" "$iconset/icon_256x256@2x.png"
cp "$tmp/mac/512x512.png" "$iconset/icon_512x512.png"
cp "$tmp/mac/1024x1024.png" "$iconset/icon_512x512@2x.png"
iconutil -c icns "$iconset" -o "$icons/icon.icns"

# icon.ico: every size Windows asks for at 100 to 250 % scaling, each from its own source. Windows
# picks entries by size, not by position. Tauri embeds the whole file as the exe's icon resource;
# the shell loads the entry it needs from there (`Image::from_app_icon_resource`): the window icon
# at the taskbar's size for the window's scale (24/30/36/48/60), the notification-area icon at the
# small-icon size (16/20/24/32/40). The first entry is 32 px, the size `tauri icon` itself puts first.
python3 -I - "$tmp" "$icons/icon.ico" <<'PY'
import struct
import sys
from pathlib import Path

tmp, out = Path(sys.argv[1]), Path(sys.argv[2])
sources = {size: f"w{size}" for size in (32, 16, 20, 24, 30, 36, 40, 48)}
sources.update({size: "win" for size in (60, 64, 72, 80, 96, 128, 256)})
images = [(size, (tmp / name / f"{size}x{size}.png").read_bytes()) for size, name in sources.items()]
# ICONDIR, then one ICONDIRENTRY (16 bytes) per image, then the PNG data. 256 is written as 0.
data = struct.pack("<HHH", 0, 1, len(images))
offset = 6 + 16 * len(images)
for size, png in images:
    side = 0 if size >= 256 else size
    data += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
data += b"".join(png for _, png in images)
out.write_bytes(data)
print(f"{out.name}: {len(images)} sizes ({', '.join(str(s) for s, _ in images)}), {len(data)} bytes")
PY
echo "icon.icns: $(stat -f %z "$icons/icon.icns") bytes"
echo "tray/macos-template.png: $(stat -f %z "$icons/tray/macos-template.png") bytes"
