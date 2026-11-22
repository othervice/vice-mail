#!/usr/bin/env bash
# Regenerates every app icon in this directory from source.svg:
# - the files tauri.conf.json bundles (32x32.png, 128x128.png, 128x128@2x.png, icon.icns,
#   icon.png), rendered by `tauri icon`, which rasterises the SVG for each size;
# - icon.ico with every size Windows asks for at 100 to 250 % scaling (16 to 256 px), instead of
#   the six sizes `tauri icon` puts into it. PNG-compressed entries (Windows Vista and later).
# Needs pnpm (with the app's node_modules installed) and python3 (standard library only).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
app="$(cd "$here/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# `tauri icon` lists every file it writes; show its output only when it fails.
icon() {
  local log
  if ! log="$(pnpm --dir "$app" tauri icon "$here/source.svg" "$@" 2>&1)"; then
    printf '%s\n' "$log" >&2
    exit 1
  fi
}

icon -o "$tmp/all"
for file in 32x32.png 128x128.png 128x128@2x.png icon.icns icon.png; do
  cp "$tmp/all/$file" "$here/$file"
done

ico_sizes="16,20,24,30,32,36,40,48,60,64,72,80,96,128,256"
icon -o "$tmp/ico" -p "$ico_sizes"
python3 - "$tmp/ico" "$here/icon.ico" "$ico_sizes" <<'PY'
import struct
import sys
from pathlib import Path

src, out, sizes = Path(sys.argv[1]), Path(sys.argv[2]), [int(s) for s in sys.argv[3].split(",")]
images = [(size, (src / f"{size}x{size}.png").read_bytes()) for size in sizes]
# ICONDIR, then one ICONDIRENTRY (16 bytes) per image, then the PNG data. 256 is written as 0.
data = struct.pack("<HHH", 0, 1, len(images))
offset = 6 + 16 * len(images)
for size, png in images:
    side = 0 if size >= 256 else size
    data += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
data += b"".join(png for _, png in images)
out.write_bytes(data)
print(f"{out.name}: {len(images)} sizes ({', '.join(str(s) for s in sizes)}), {len(data)} bytes")
PY
