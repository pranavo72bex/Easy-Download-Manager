"""Makes desktop/crates/dm-desktop/fonts/Inter-TabularDigits.ttf.

The list's sizes, speeds and times are drawn with digits of one width, so
a column of them lines up and a changing speed does not jitter. egui does
not apply a font's optional features, so this takes Inter's own tabular
figures (its `tnum` feature) and makes them the digits that are drawn,
keeping only the characters numbers use; letters come from Inter itself,
behind it in the family. Inter is under the SIL OFL 1.1 with no reserved
name; the result keeps that licence (fonts/LICENSE-Inter.txt).

    pip install fonttools
    python3 scripts/tabular-digits.py
"""
import pathlib
import sys

from fontTools import subset
from fontTools.ttLib import TTFont

FONTS = pathlib.Path(__file__).resolve().parent.parent / "desktop/crates/dm-desktop/fonts"
# Digits and what stands between and around them in a size, rate or time.
CHARS = [ord(c) for c in "0123456789.,:%/ +-−–  "]


def tabular(font):
    """Inter's glyph for each digit's tabular form."""
    gsub = font["GSUB"].table
    mapping = {}
    for record in gsub.FeatureList.FeatureRecord:
        if record.FeatureTag != "tnum":
            continue
        for index in record.Feature.LookupListIndex:
            lookup = gsub.LookupList.Lookup[index]
            for table in lookup.SubTable:
                if lookup.LookupType == 7:
                    table = table.ExtSubTable
                mapping.update(getattr(table, "mapping", {}))
    return mapping


def main():
    font = TTFont(FONTS / "Inter-Regular.ttf")
    forms = tabular(font)
    if not forms:
        sys.exit("Inter-Regular.ttf has no tabular figures")
    for table in font["cmap"].tables:
        for code in CHARS:
            glyph = table.cmap.get(code)
            if glyph in forms:
                table.cmap[code] = forms[glyph]
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = ["*"]
    options.notdef_outline = True
    cut = subset.Subsetter(options)
    cut.populate(unicodes=CHARS)
    cut.subset(font)
    # A name of its own, so it is never taken for Inter.
    for record in font["name"].names:
        if record.nameID in (1, 4, 16):
            record.string = record.toUnicode().replace("Inter", "Inter Tabular Digits")
        elif record.nameID == 6:  # a PostScript name has no spaces
            record.string = record.toUnicode().replace("Inter", "InterTabularDigits")
    font.save(FONTS / "Inter-TabularDigits.ttf")


if __name__ == "__main__":
    main()
