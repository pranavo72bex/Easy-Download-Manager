#!/usr/bin/env python3
"""A link dragged from Firefox onto dm's window, with real X11 input.

Run by scripts/linux-drag-check.sh inside its container (Xvfb, openbox,
xdotool). Firefox shows a page whose top is one large link; dm's window sits
beside it; xdotool presses on the link, moves across onto dm and lets go, as
a hand does. dm must queue the link. Firefox is the one firefox-check.py
fetches.
"""

import http.server
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import threading
import time

CACHE = pathlib.Path.home() / ".cache/dm-firefox"


def fetch_firefox():
    """Firefox fetched into a cache, as firefox-check.py fetches it."""
    binary = CACHE / "firefox/firefox"
    if not binary.exists():
        CACHE.mkdir(parents=True, exist_ok=True)
        arch = "linux64-aarch64" if os.uname().machine in ("aarch64", "arm64") else "linux64"
        subprocess.run(f"curl -sSLf 'https://download.mozilla.org/?product=firefox-latest-ssl&os={arch}&lang=en-US'"
                       f" | tar -xJ -C '{CACHE}'", shell=True, check=True)
    return str(binary)


def profile():
    """A new profile with no first-run pages over the one under test."""
    d = pathlib.Path(tempfile.mkdtemp(prefix="dm-ffprofile-"))
    (d / "user.js").write_text("".join(f'user_pref("{k}", {v});\n' for k, v in [
        ("browser.shell.checkDefaultBrowser", "false"),
        ("datareporting.policy.dataSubmissionEnabled", "false"),
        ("termsofuse.bypassNotification", "true"),
        ("termsofuse.acceptedVersion", "999"),
        ("termsofuse.acceptedDate", '"1700000000000"'),
        ("datareporting.policy.dataSubmissionPolicyBypassNotification", "true"),
        ("browser.aboutwelcome.enabled", "false"),
        ("trailhead.firstrun.didSeeAboutWelcome", "true"),
        ("browser.startup.homepage_override.mstone", '"ignore"'),
    ]))
    return d

BINARY = os.environ.get("DM_BINARY", "/target/debug/dm-desktop")
LINK = "https://downloads.example.com/files/dragged.zip"
PAGE = f"""<!doctype html><title>links</title>
<style>body{{margin:0}} a{{display:block;width:560px;height:420px;font:40px sans-serif;
background:#cde;color:#036}}</style>
<a id=l href="{LINK}">Drag this link onto dm</a>"""


def xdo(*args):
    return subprocess.run(["xdotool", *args], capture_output=True, text=True, timeout=20).stdout.strip()


def window(name, seconds=30):
    end = time.time() + seconds
    while time.time() < end:
        found = xdo("search", "--onlyvisible", "--name", name).split()
        if found:
            return found[-1]
        time.sleep(0.5)
    return None


def rows(home):
    try:
        return json.loads((home / "queue.json").read_text() or "[]")
    except (OSError, ValueError):
        return []


def main():
    served = tempfile.mkdtemp(prefix="dm-drag-page-")
    pathlib.Path(served, "index.html").write_text(PAGE)
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=served, **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    page = f"http://127.0.0.1:{server.server_address[1]}/index.html"

    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-drag-home-"))
    (home / "settings.json").write_text(json.dumps({"download_dir": str(home / "out")}))
    env = {**os.environ, "DM_HOME": str(home)}
    dm = subprocess.Popen([BINARY], env=env, stdout=subprocess.DEVNULL, stderr=open("/shots/dm.log", "w"))
    firefox = subprocess.Popen(
        [fetch_firefox(), "--no-remote", "--width", "640", "--height", "760", "--profile", str(profile()), page],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    ok = False
    try:
        dm_window = window("^dm$")
        ff_window = window("links")
        if not dm_window or not ff_window:
            print("FAIL  the windows never appeared", dm_window, ff_window)
            return 1
        # Side by side, overlapping nowhere: a press on Firefox raises it,
        # and over dm it would take the drop itself.
        for name in ("links", "^dm$"):
            subprocess.run(["wmctrl", "-r", name.strip("^$"), "-b", "remove,maximized_vert,maximized_horz"])
        xdo("windowmove", ff_window, "0", "0")
        xdo("windowsize", ff_window, "640", "780")
        xdo("windowmove", dm_window, "660", "0")
        xdo("windowsize", dm_window, "620", "780")
        time.sleep(3)
        # Firefox's own toolbars sit over the page's top; the link fills
        # the 420 points below them.
        start = (200, 320)
        print(f"  windows: dm {dm_window}, Firefox {ff_window}")
        for attempt in range(3):
            xdo("windowactivate", "--sync", ff_window)
            xdo("windowmove", dm_window, "660", "0")
            time.sleep(0.5)
            xdo("mousemove", str(start[0]), str(start[1]))
            time.sleep(0.4)
            xdo("mousedown", "1")
            time.sleep(0.3)
            # Across in steps, as a hand moves: a drag starts only once the
            # pointer has travelled, and the target is told it is entered.
            for step in range(1, 31):
                x = start[0] + (950 - start[0]) * step // 30
                y = start[1] + (400 - start[1]) * step // 30
                xdo("mousemove", str(x), str(y))
                time.sleep(0.05)
            time.sleep(0.5)
            subprocess.run(["import", "-window", "root", f"/shots/during-{attempt}.png"])
            xdo("mouseup", "1")
            time.sleep(2)
            subprocess.run(["import", "-window", "root", f"/shots/after-{attempt}.png"])
            queued = [r for r in rows(home) if r.get("url") == LINK]
            if queued:
                ok = True
                break
            print(f"  attempt {attempt + 1}: nothing queued yet")
        print(("ok    " if ok else "FAIL  ") + "a link dragged from Firefox onto dm's window is queued")
        return 0 if ok else 1
    finally:
        firefox.terminate()
        dm.terminate()
        server.shutdown()


if __name__ == "__main__":
    sys.exit(main())
