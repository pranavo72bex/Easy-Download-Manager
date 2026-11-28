# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Makes dm's ZIP fixtures with a password (gaps row F10): real archives by libarchive's ZIP
writer (bsdtar), which encrypts with the system's own crypto: AES-256 and AES-128 (WinZip's
AE-2, Deflate under it) and PKWARE's older cipher. The password is `dm secret`. The bytes come
from the generator `extract/password_tests.rs` repeats, so the tests compare byte for byte.
The salts are random, so each run makes other bytes holding the same files.

Made on Windows 11 with its own C:\\Windows\\System32\\tar.exe (bsdtar 3.8.9, libarchive 3.8.9,
cng/2.0); a macOS or Linux bsdtar built with a crypto library makes the same kind:
`python -I make.py <scratch folder> <this folder>`."""
import os
import shutil
import subprocess
import sys

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

WORDS = [b"alpha ", b"beta ", b"gamma ", b"delta ", b"download ", b"manager ", b"range ", b"stream\n"]
PASSWORD = "dm secret"


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


def bsdtar():
    system = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32", "tar.exe")
    return system if os.path.exists(system) else shutil.which("bsdtar") or "bsdtar"


def main(src, out):
    os.makedirs(out, exist_ok=True)
    os.makedirs(os.path.join(src, "docs", "empty"), exist_ok=True)
    open(os.path.join(src, "docs", "a.txt"), "wb").write(b"hello, zip\n")
    open(os.path.join(src, "docs", "b.txt"), "wb").write(text(20000, 1))
    open(os.path.join(src, "c.bin"), "wb").write(noise(3000, 2))
    open(os.path.join(src, "zero.txt"), "wb").write(b"")
    names = ["docs", "c.bin", "zero.txt"]
    for cipher, name in [("aes256", "aes256.zip"), ("aes128", "aes128.zip"),
                         ("zipcrypt", "pkware.zip")]:
        target = os.path.join(out, name)
        if os.path.exists(target):
            os.remove(target)
        subprocess.run([bsdtar(), "--format", "zip", "--options", "zip:encryption=" + cipher,
                        "--passphrase", PASSWORD, "-cf", target] + names, cwd=src, check=True)


main(sys.argv[1], sys.argv[2])
