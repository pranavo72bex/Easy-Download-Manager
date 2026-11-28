# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Draws every icon EDM ships from its one source, assets/app-icons/icon.svg, so the
desktop program, the browser extension and the Android app show the same icon.

    python3 scripts/app-icons.py

Writes assets/app-icons/ (the PNGs, icon.ico for Windows, icon.icns for macOS) and
extension/icons/. Android's launcher icon is the same drawing as two vector layers in
crates/edm-android/android/app/src/main/res/drawable/ic_launcher_{background,foreground}.xml,
written by hand in the same units: change them with the SVG.

Needs rsvg-convert and ImageMagick (`magick`); icon.icns needs macOS's iconutil (skipped,
with a note, elsewhere). A tool for the person who changes the icon: nothing builds with it.
"""

import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "assets" / "app-icons" / "icon.svg"
APP = ROOT / "assets" / "app-icons"
EXTENSION = ROOT / "extension" / "icons"


def inset(svg: str, canvas: int, margin: int, shadow: bool = False) -> str:
    """The icon drawn `margin` pixels inside a `canvas`-wide transparent square (macOS's
    grid keeps a tile 824 of 1024 with a soft shadow; a browser store's 128 keeps 8 round it)."""
    body = re.search(r"<svg[^>]*>(.*)</svg>", svg, re.S).group(1)
    size = canvas - 2 * margin
    defs = ""
    filt = ""
    if shadow:
        defs = (
            '<defs><filter id="drop" x="-20%" y="-20%" width="140%" height="140%">'
            '<feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" '
            'flood-opacity="0.3"/></filter></defs>'
        )
        filt = ' filter="url(#drop)"'
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{canvas}" height="{canvas}" '
        f'viewBox="0 0 {canvas} {canvas}">{defs}<g{filt}>'
        f'<svg x="{margin}" y="{margin}" width="{size}" height="{size}" viewBox="18 18 72 72">'
        f"{body}</svg></g></svg>"
    )


def render(svg: str, size: int, out: pathlib.Path, work: pathlib.Path) -> None:
    src = work / "src.svg"
    src.write_text(svg)
    subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), str(src), "-o", str(out)],
        check=True,
    )


def main() -> int:
    for tool in ("rsvg-convert", "magick"):
        if not shutil.which(tool):
            print(f"app-icons: {tool} is needed (brew install librsvg imagemagick)")
            return 1
    svg = SOURCE.read_text()
    with tempfile.TemporaryDirectory() as tmp:
        work = pathlib.Path(tmp)

        # The desktop program: Linux's and the tray's PNGs, the bundle's, Windows' .ico.
        for size, name in ((32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")):
            render(svg, size, APP / name, work)
        ico_parts = []
        for size in (16, 24, 32, 48, 64, 128, 256):
            part = work / f"ico{size}.png"
            render(svg, size, part, work)
            ico_parts.append(str(part))
        subprocess.run(["magick", *ico_parts, str(APP / "icon.ico")], check=True)

        # macOS: the tile on Apple's icon grid, every size of an iconset.
        if shutil.which("iconutil"):
            mac = inset(svg, 1024, 100, shadow=True)
            iconset = work / "icon.iconset"
            iconset.mkdir()
            for base in (16, 32, 128, 256, 512):
                render(mac, base, iconset / f"icon_{base}x{base}.png", work)
                render(mac, base * 2, iconset / f"icon_{base}x{base}@2x.png", work)
            subprocess.run(
                ["iconutil", "-c", "icns", str(iconset), "-o", str(APP / "icon.icns")],
                check=True,
            )
        else:
            print("app-icons: no iconutil (macOS only); icon.icns left as it was")

        # The browser extension: the toolbar's 16 and 32 fill their square; the store's
        # 48 and 128 keep a margin, as the stores ask.
        render(svg, 16, EXTENSION / "icon16.png", work)
        render(svg, 32, EXTENSION / "icon32.png", work)
        render(inset(svg, 48, 2), 48, EXTENSION / "icon48.png", work)
        render(inset(svg, 128, 8), 128, EXTENSION / "icon128.png", work)
    print("app-icons: written from", SOURCE.relative_to(ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
