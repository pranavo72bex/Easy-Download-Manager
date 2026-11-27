#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The README's phone demo and picture, recorded from dm on an Android emulator.

    python3 scripts/readme-phone.py app-debug.apk            # into docs/
    python3 scripts/readme-phone.py app-debug.apk some/folder

The APK is installed fresh (its data cleared) on the one emulator `adb` reaches, in the dark
look, and its screen recorded (`screenrecord`) while five links are shared to dm, as another
app's share sheet hands them over, from a server on this computer (10.0.2.2 from inside the
emulator): two large ones downloading side by side, three small ones done at once. ffmpeg
makes the recording docs/demo-phone.gif; the screen once they are done is docs/phone.png.
The files are made up and only the emulator's screen is shown. Needs adb, ffmpeg and python3;
an emulator only, never a person's phone.
"""

import http.server
import importlib.util
import pathlib
import shutil
import subprocess
import sys
import tempfile
import threading
import time

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

ROOT = pathlib.Path(__file__).resolve().parent.parent
if len(sys.argv) < 2:
    sys.exit(__doc__)
APK = pathlib.Path(sys.argv[1])
OUT = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / "docs"
PKG = "com.dm.android"
ACTIVITY = f"{PKG}/{PKG}.DmActivity"
SECONDS = 16
FPS = 10
# The GIF's width and the picture's, in pixels.
GIF_WIDTH = 320
PICTURE_WIDTH = 540

# The desktop demo's server: two large files at a steady pace, three small ones at once.
_spec = importlib.util.spec_from_file_location("readme_demo", ROOT / "scripts" / "readme-demo.py")
_demo = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_demo)
Files = _demo.Files


def adb(*args, check=True, capture=False):
    return subprocess.run(["adb", *args], check=check, capture_output=capture)


def share(url):
    """`url` handed to dm as a share sheet hands it, dm already open."""
    adb("shell", "am", "start", "-a", "android.intent.action.SEND", "-t", "text/plain",
        "--es", "android.intent.extra.TEXT", url, "-n", ACTIVITY)


def main():
    if not APK.exists():
        sys.exit(f"no APK at {APK}: ./gradlew assembleDebug in crates/dm-android/android")
    devices = adb("devices", capture=True).stdout.decode()
    if "emulator-" not in devices:
        sys.exit("no emulator: start one (emulator -avd <name>); never a person's phone")
    if not shutil.which("ffmpeg"):
        sys.exit("no ffmpeg: brew install ffmpeg")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Files)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://10.0.2.2:{server.server_address[1]}/files"
    OUT.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-phone-", dir="/tmp"))
    try:
        adb("install", "-r", "-g", str(APK))
        adb("shell", "pm", "clear", PKG)
        adb("shell", "pm", "grant", PKG, "android.permission.POST_NOTIFICATIONS", check=False)
        adb("shell", "cmd", "uimode", "night", "yes")
        # An earlier run's files taken away, so each arrives under its own name.
        adb("shell", "rm", "-rf", "/sdcard/Download/dm", check=False)
        adb("shell", "am", "start", "-W", "-n", ACTIVITY)
        time.sleep(6)
        recording = subprocess.Popen(
            ["adb", "shell", "screenrecord", "--time-limit", str(SECONDS), "/sdcard/dm-demo.mp4"])
        time.sleep(1.5)
        for name in ("ubuntu-24.04-desktop-amd64.iso", "report.pdf", "holiday-4k.mp4",
                     "photos.zip", "song.mp3"):
            share(f"{base}/{name}")
            time.sleep(1.2)
        recording.wait(SECONDS + 20)
        time.sleep(1)
        movie = work / "demo.mp4"
        adb("pull", "/sdcard/dm-demo.mp4", str(movie))
        palette = work / "palette.png"
        scale = f"fps={FPS},scale={GIF_WIDTH}:-1:flags=lanczos"
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(movie), "-vf",
                        f"{scale},palettegen=stats_mode=diff", str(palette)], check=True)
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(movie), "-i", str(palette),
                        "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
                        "-loop", "0", str(OUT / "demo-phone.gif")], check=True)
        # The list once the large ones have run a while longer.
        time.sleep(6)
        screen = work / "screen.png"
        with open(screen, "wb") as f:
            subprocess.run(["adb", "exec-out", "screencap", "-p"], stdout=f, check=True)
        subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(screen), "-vf",
                        f"scale={PICTURE_WIDTH}:-1:flags=lanczos", str(OUT / "phone.png")], check=True)
    finally:
        adb("shell", "am", "force-stop", PKG, check=False)
        adb("shell", "rm", "-f", "/sdcard/dm-demo.mp4", check=False)
        server.shutdown()
        shutil.rmtree(work, ignore_errors=True)
    print(OUT / "demo-phone.gif")
    print(OUT / "phone.png")


if __name__ == "__main__":
    main()
