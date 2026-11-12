# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Makes dm's 7z test fixtures (gaps row F10): raw LZMA and LZMA2 streams (Python's own lzma
module) and real 7z archives (the system's bsdtar, whose libarchive writes 7z). The bytes come
from a small generator that `extract/sevenz/tests.rs` repeats, so the tests compare byte for byte.
Nothing is installed: run it on a Mac as `python3 -I make.py <scratch folder> <this folder>`."""
import lzma
import os
import subprocess
import sys

WORDS = [b"alpha ", b"beta ", b"gamma ", b"delta ", b"download ", b"manager ", b"range ", b"stream\n"]


def text(n, seed):
    x = seed
    out = bytearray()
    while len(out) < n:
        x = (x * 1103515245 + 12345) & 0x7FFFFFFF
        out += WORDS[(x >> 16) & 7]
    return bytes(out[:n])


def noise(n, seed):
    x = seed
    out = bytearray(n)
    for i in range(n):
        x = (x * 1103515245 + 12345) & 0x7FFFFFFF
        out[i] = (x >> 16) & 0xFF
    return bytes(out)


def pattern(n):
    unit = bytes(range(256)) * 3 + b"dm repeats itself\n"
    return (unit * (n // len(unit) + 1))[:n]


def main(src, out):
    os.makedirs(out, exist_ok=True)
    # Raw LZMA (with its end marker, as liblzma writes a raw LZMA1 stream).
    data = text(50000, 11)
    raw = lzma.compress(data, format=lzma.FORMAT_RAW,
                        filters=[{"id": lzma.FILTER_LZMA1, "dict_size": 1 << 16}])
    open(os.path.join(out, "text.lzma"), "wb").write(raw)
    # Unusual properties: lc=1, lp=2, pb=1, on bytes where position matters.
    data = noise(3000, 5) + pattern(20000) + text(8000, 6)
    raw = lzma.compress(data, format=lzma.FORMAT_RAW,
                        filters=[{"id": lzma.FILTER_LZMA1, "dict_size": 1 << 12,
                                  "lc": 1, "lp": 2, "pb": 1}])
    open(os.path.join(out, "props.lzma"), "wb").write(raw)
    # LZMA2: a stored chunk (noise), compressed chunks, several of them without a dictionary
    # reset, and a 64 KiB dictionary that wraps many times.
    data = noise(65600, 7) + text(60000, 8) + pattern(4_500_000)
    raw = lzma.compress(data, format=lzma.FORMAT_RAW,
                        filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 1 << 16}])
    open(os.path.join(out, "mixed.lzma2"), "wb").write(raw)

    # Real 7z archives by libarchive.
    os.makedirs(os.path.join(src, "docs", "empty"), exist_ok=True)
    open(os.path.join(src, "docs", "a.txt"), "wb").write(b"hello, 7z\n")
    open(os.path.join(src, "docs", "b.txt"), "wb").write(text(20000, 1))
    open(os.path.join(src, "c.bin"), "wb").write(noise(3000, 2))
    open(os.path.join(src, "zero.txt"), "wb").write(b"")
    open(os.path.join(src, "docs", "naïve 日本.txt"), "wb").write(b"unicode\n")
    names = ["docs", "c.bin", "zero.txt"]
    for method in ["store", "lzma1", "lzma2", "bzip2", "ppmd"]:
        target = os.path.join(out, method + ".7z")
        if os.path.exists(target):
            os.remove(target)
        subprocess.run(["/usr/bin/bsdtar", "--format", "7zip", "--options",
                        "7zip:compression=" + method, "-cf", target] + names,
                       cwd=src, check=True)
    link_dir = os.path.join(src, "..", "linksrc")
    os.makedirs(link_dir, exist_ok=True)
    if not os.path.lexists(os.path.join(link_dir, "link")):
        os.symlink("/etc/passwd", os.path.join(link_dir, "link"))
    target = os.path.join(out, "link.7z")
    if os.path.exists(target):
        os.remove(target)
    subprocess.run(["/usr/bin/bsdtar", "--format", "7zip", "-cf", target, "link"],
                   cwd=link_dir, check=True)


main(sys.argv[1], sys.argv[2])
