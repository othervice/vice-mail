#!/usr/bin/env bash
# Regenerates every app icon in this directory:
#
#   app/src-tauri/icons/generate.sh
#
# 1. docs/brand/make_svgs.py writes the SVG sources (source*.svg here, the brand files in docs/brand).
# 2. `tauri icon` (resvg) rasterises each SVG at the sizes it is meant for:
#    - source.svg            full-detail master on the macOS icon grid (824 px body in 1024, shadow):
#                            macOS 64 px and up, and the PNG files tauri.conf.json lists
#    - source-macos-16.svg, source-macos-32.svg
#                            simplified icon for the small macOS sizes (Finder lists, menus)
#    - source-windows.svg    the master framed for Windows (body fills 94 %, no shadow): 60 px and up
#    - source-16/24/32/48.svg
#                            simplified icon, pixel-snapped per size: Windows title bar, taskbar,
#                            Explorer up to 48 px; 20, 30, 36 and 40 px come from the nearest of them
# 3. icon.icns is assembled with iconutil, icon.ico (PNG entries, Windows Vista and later) by the
#    Python below.
# Needs macOS (iconutil), pnpm with the app's node_modules installed, and python3 (standard library).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
app="$(cd "$here/../.." && pwd)"
root="$(cd "$app/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

python3 -I "$root/docs/brand/make_svgs.py" >/dev/null

# render <svg> <name> <sizes>: writes $tmp/<name>/<size>x<size>.png. `tauri icon` lists every file
# it writes; its output is shown only when it fails.
render() {
  local log
  if ! log="$(pnpm --dir "$app" tauri icon "$here/$1" -o "$tmp/$2" -p "$3" 2>&1)"; then
    printf '%s\n' "$log" >&2
    exit 1
  fi
}

render source.svg mac 64,128,256,512,1024
render source-macos-16.svg mac16 16
render source-macos-32.svg mac32 32
render source-windows.svg win 60,64,72,80,96,128,256
render source-16.svg w16 16
render source-24.svg w24 20,24
render source-32.svg w32 30,32
render source-48.svg w48 36,40,48

# The PNG files tauri.conf.json bundles (Linux, default window icon outside Windows and macOS dev).
cp "$tmp/w32/32x32.png" "$here/32x32.png"
cp "$tmp/mac/128x128.png" "$here/128x128.png"
cp "$tmp/mac/256x256.png" "$here/128x128@2x.png"
cp "$tmp/mac/512x512.png" "$here/icon.png"

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
iconutil -c icns "$iconset" -o "$here/icon.icns"

# icon.ico: every size Windows asks for at 100 to 250 % scaling, each from its own source.
python3 -I - "$tmp" "$here/icon.ico" <<'PY'
import struct
import sys
from pathlib import Path

tmp, out = Path(sys.argv[1]), Path(sys.argv[2])
sources = {16: "w16", 20: "w24", 24: "w24", 30: "w32", 32: "w32", 36: "w48", 40: "w48", 48: "w48",
           60: "win", 64: "win", 72: "win", 80: "win", 96: "win", 128: "win", 256: "win"}
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
echo "icon.icns: $(stat -f %z "$here/icon.icns") bytes"
