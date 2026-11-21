# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Makes dm's 7z fixtures with a password (gaps row F10): `aes.7z`, its files encrypted
(LZMA2 under 7-Zip's AES-256 coder, 2^19 rounds of SHA-256 for the key) and its list of files
not; and `aes-header.7z`, its list of files encrypted too. The password, `pässwort 7z`, is not
all ASCII, so the key's UTF-16 is tested. The files are `make.py`'s, from the same generator.

libarchive writes no encrypted 7z, so these are made by py7zr (1.1.4 here), a 7z writer of
its own in Python, in a virtual environment of their own so nothing is installed for the
system: `python -m venv v && v/bin/pip install py7zr==1.1.4`, then
`v/bin/python -I make_aes.py <scratch folder> <this folder>`. Its IVs are random, so each run
makes other bytes holding the same files."""
import os
import sys

import py7zr

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

WORDS = [b"alpha ", b"beta ", b"gamma ", b"delta ", b"download ", b"manager ", b"range ", b"stream\n"]
PASSWORD = "pässwort 7z"


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


def main(src, out):
    os.makedirs(out, exist_ok=True)
    os.makedirs(os.path.join(src, "docs", "empty"), exist_ok=True)
    open(os.path.join(src, "docs", "a.txt"), "wb").write(b"hello, 7z\n")
    open(os.path.join(src, "docs", "b.txt"), "wb").write(text(20000, 1))
    open(os.path.join(src, "c.bin"), "wb").write(noise(3000, 2))
    open(os.path.join(src, "zero.txt"), "wb").write(b"")
    open(os.path.join(src, "docs", "naïve 日本.txt"), "wb").write(b"unicode\n")
    for name, header in [("aes.7z", False), ("aes-header.7z", True)]:
        target = os.path.join(out, name)
        if os.path.exists(target):
            os.remove(target)
        with py7zr.SevenZipFile(target, "w", password=PASSWORD, header_encryption=header) as z:
            for entry in ["docs", "c.bin", "zero.txt"]:
                z.writeall(os.path.join(src, entry), entry) if os.path.isdir(
                    os.path.join(src, entry)) else z.write(os.path.join(src, entry), entry)


main(sys.argv[1], sys.argv[2])
