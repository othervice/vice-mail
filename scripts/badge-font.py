#!/usr/bin/env python3
"""Writes the font of the unread count on Windows (decision 0080) from Selawik.

  python3 -m venv /tmp/ft && /tmp/ft/bin/pip install fonttools
  /tmp/ft/bin/python scripts/badge-font.py path/to/selawksb.ttf

Selawik is Microsoft's open-source fallback for Segoe UI (https://github.com/microsoft/Selawik,
release 1.01, SIL Open Font License 1.1, reserved font name "Selawik"). The count is drawn from
Selawik Semibold (selawksb.ttf of the release zip).

Output: app/src-tauri/fonts/badge-digits.ttf, a subset with
- the glyphs of the count: the digits (the "+" of "99+" is drawn as two bars, see badge.rs);
- the capitals FreeType's auto-hinter (skrifa's port) measures for Latin: the alignment zones
  at the top ("T H E Z O C Q S") and at the baseline ("H E Z L O C U S"), and the standard stem
  width ("O"). Selawik has no hinting instructions of its own; without these letters the hinter
  would not know the zones, and the digits (as high as the capitals) would not snap to the pixel
  grid. No lowercase: from the x-height zone ("u v x z o e s c") the hinter would round the x-height
  and scale everything with it, so the digits would no longer be exactly as high as asked.

A subset is a modified version under the OFL, so it carries another name ("Badge Digits"; the
reserved name Selawik is not used) and keeps Microsoft's copyright and the licence
(app/src-tauri/fonts/OFL.txt). No layout tables (GSUB, GPOS, GDEF), no signature, no glyph names.
"""
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "app" / "src-tauri" / "fonts" / "badge-digits.ttf"

TEXT = "0123456789" + "THEZOCQS" + "HEZLOCUS"
FAMILY = "Badge Digits"
STYLE = "Semibold"
DESCRIPTION = (
    "Subset of Selawik Semibold 1.01 (Microsoft, SIL Open Font License 1.1): the digits and the "
    "capitals the auto-hinter measures. Renamed, as the OFL asks of modified versions."
)


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("usage: badge-font.py path/to/selawksb.ttf")
    font = TTFont(sys.argv[1])
    if font["name"].getDebugName(1) != "Selawik Semibold":
        sys.exit(f"{sys.argv[1]} is not Selawik Semibold")

    options = subset.Options()
    options.layout_features = []
    options.drop_tables += ["GSUB", "GPOS", "GDEF", "DSIG", "kern"]
    options.glyph_names = False
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 10, 13, 14]
    options.notdef_outline = True
    options.recalc_bounds = True
    subsetter = subset.Subsetter(options)
    subsetter.populate(text=TEXT)
    subsetter.subset(font)

    names = font["name"]
    for record in list(names.names):
        if record.nameID in (1, 3, 4, 6, 16, 17, 10):
            names.removeNames(nameID=record.nameID)
    postscript = FAMILY.replace(" ", "") + "-" + STYLE
    for name_id, value in (
        (1, f"{FAMILY} {STYLE}"),
        (3, f"{postscript};1.01"),
        (4, f"{FAMILY} {STYLE}"),
        (6, postscript),
        (10, DESCRIPTION),
    ):
        names.setName(value, name_id, 3, 1, 0x409)
    # Only the Windows (3, 1, 0x409) records stay, so every name is set once.
    names.names = [n for n in names.names if (n.platformID, n.platEncID, n.langID) == (3, 1, 0x409)]

    OUT.parent.mkdir(parents=True, exist_ok=True)
    font.save(OUT)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
