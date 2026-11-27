#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The README's desktop demo, recorded from the real window.

    python3 scripts/readme-demo.py             # docs/demo-desktop.gif
    python3 scripts/readme-demo.py some/folder

dm's GPUI window (`target/debug/dm-gpui`, or DM_BINARY) is started with a home
of its own, in the dark look, and recorded from the screen over its window
(`screencapture -v`) while five downloads arrive from a server on 127.0.0.1:
two large ones run side by side, three small ones finish at once, then one
download's own window shows its speed, time left and parts. ffmpeg makes the
recording a GIF. Nothing of the computer that made it is shown: the files are
made up, the window is dm's alone. macOS; needs ffmpeg and the screen
recording permission for the terminal.
"""

import http.server
import importlib.util
import json
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
OUT = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "docs"
MiB = 1024 * 1024
# How long the recording runs, in seconds, and the GIF's frames a second and width in pixels.
SECONDS = 16
FPS = 10
WIDTH = 960

# The README's pictures' own helpers: dm started on a home, asked over its control port.
_spec = importlib.util.spec_from_file_location("readme_shots", ROOT / "scripts" / "readme-shots.py")
shots = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(shots)


class Files(http.server.BaseHTTPRequestHandler):
    """Two large files sent at a steady pace on each connection, so they run side by side for
    the recording; three small ones whole at once. Ranges answered, as a server that lets a
    download resume does."""

    protocol_version = "HTTP/1.1"
    sizes = {
        "ubuntu-24.04-desktop-amd64.iso": 400 * MiB,
        "holiday-4k.mp4": 300 * MiB,
        "report.pdf": 2 * MiB,
        "photos.zip": 6 * MiB,
        "song.mp3": 4 * MiB,
    }
    large = {"ubuntu-24.04-desktop-amd64.iso", "holiday-4k.mp4"}

    def log_message(self, *_):
        pass

    def do_GET(self):
        name = self.path.rsplit("/", 1)[-1]
        size = self.sizes.get(name)
        if size is None:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        start, end = 0, size - 1
        asked = self.headers.get("Range", "")
        if asked.startswith("bytes="):
            first, _, last = asked[6:].partition("-")
            start = int(first or 0)
            end = min(int(last), size - 1) if last else size - 1
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        self.end_headers()
        left = end - start + 1
        chunk = 64 * 1024
        try:
            while left > 0:
                n = min(chunk, left)
                self.wfile.write(b"\0" * n)
                left -= n
                # A large file at about 1.3 MB/s on each of its connections.
                if name in self.large:
                    time.sleep(0.05)
        except OSError:
            pass


def record(box, path):
    """The screen over `box` (points: left, top, right, bottom) for SECONDS, in the
    background."""
    x0, y0, x1, y1 = (round(v) for v in box)
    return subprocess.Popen(
        ["screencapture", "-x", "-v", "-V", str(SECONDS), f"-R{x0},{y0},{x1 - x0},{y1 - y0}", str(path)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def to_gif(movie, gif):
    """`movie` as a GIF of FPS frames a second, WIDTH wide, with a palette of its own."""
    palette = movie.with_suffix(".png")
    scale = f"fps={FPS},scale={WIDTH}:-1:flags=lanczos"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(movie), "-vf",
                    f"{scale},palettegen=stats_mode=diff", str(palette)], check=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(movie), "-i", str(palette),
                    "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
                    "-loop", "0", str(gif)], check=True)


def main():
    if sys.platform != "darwin":
        sys.exit("the demo is recorded on macOS, with screencapture")
    if not shots.BINARY.exists():
        sys.exit(f"no dm at {shots.BINARY}: cargo build -p dm-gpui")
    if not shutil.which("ffmpeg"):
        sys.exit("no ffmpeg: brew install ffmpeg")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Files)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_address[1]}/files"
    OUT.mkdir(parents=True, exist_ok=True)
    # Under /tmp, not the system's per-user temporary folder, whose path is unique to the
    # computer and its user.
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-demo-", dir="/tmp"))
    home = work / "home"
    downloads = home / "downloads"
    downloads.mkdir(parents=True)
    # No pop-up on each download (the settings' edition saved with them, or dm takes its
    # defaults, every pop-up on), and no bar asking to add dm to a browser (its Not now).
    (home / "settings.json").write_text(json.dumps({
        "theme": "dark", "download_dir": str(downloads), "show_progress": False,
        "show_complete": False, "popups_version": 2,
    }), encoding="utf-8")
    (home / "browser-hint.hidden").write_text("", encoding="utf-8")
    dm = shots.Dm(home)
    try:
        time.sleep(1.5)
        shots.in_front(dm)
        box = dm.call("ui.windows")["main"]
        movie = work / "demo.mov"
        recording = record(box, movie)
        time.sleep(1.5)
        for name in ("ubuntu-24.04-desktop-amd64.iso", "report.pdf", "holiday-4k.mp4",
                     "photos.zip", "song.mp3"):
            dm.call("downloads.add", urls=[f"{base}/{name}"])
            time.sleep(0.7)
        time.sleep(5)
        # One large download's own window: its speed, time left and parts.
        large = dm.wait("ubuntu-24.04-desktop-amd64.iso", {"running"})
        dm.call("ui.select", ids=[large["id"]])
        dm.call("ui.click", text="ubuntu-24.04-desktop-amd64.iso", double=True)
        recording.wait(SECONDS + 10)
        if not movie.exists():
            sys.exit("nothing was recorded: give the terminal the screen recording permission")
        to_gif(movie, OUT / "demo-desktop.gif")
    finally:
        dm.quit()
        server.shutdown()
        shutil.rmtree(work, ignore_errors=True)
    print(OUT / "demo-desktop.gif")


if __name__ == "__main__":
    main()
