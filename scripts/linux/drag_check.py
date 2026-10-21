#!/usr/bin/env python3
"""Drag and drop between Firefox and dm's window, with real X11 input.

Run by scripts/linux-drag-check.sh inside its container (Xvfb, openbox,
xdotool). Firefox and dm sit side by side, and xdotool presses, moves and
lets go as a hand does:

- a link dragged from a page onto dm is queued;
- a finished file dragged out of dm's list onto Firefox is opened there,
  and dm's list still takes clicks afterwards (the button is not left held).
"""

import socket

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
NOTES = "notes-for-the-drag.txt"
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


def control(home):
    """A call on dm's control protocol."""
    end = time.time() + 30
    port_file = home / "control.port"
    while time.time() < end and not (port_file.exists() and port_file.read_text().strip()):
        time.sleep(0.3)
    conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
    replies = conn.makefile("r")

    def call(method, **params):
        conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(replies.readline())

    call("auth", token=(home / "control.token").read_text().strip())
    return call


def drag_out(home, dm_window, ff_window, url):
    """A finished file dragged from dm's list onto Firefox."""
    call = control(home)
    call("downloads.add", urls=[url])
    end = time.time() + 30
    while time.time() < end and not [r for r in rows(home) if r.get("url") == url
                                     and r.get("state", {}).get("kind") == "completed"]:
        time.sleep(0.5)
    time.sleep(1)
    inspected = call("ui.inspect").get("result", {})
    row = next((r for r in inspected.get("rows", []) if r.get("name") == NOTES and r.get("rect")), None)
    if not row:
        print("FAIL  the finished file's row was not found", inspected.get("rows"))
        return False
    geometry = dict(line.split("=", 1) for line in xdo("getwindowgeometry", "--shell", dm_window).splitlines())
    left, top = int(geometry["X"]), int(geometry["Y"])
    x0, y0, x1, y1 = row["rect"]
    start = (left + int(x0) + 60, top + int((y0 + y1) / 2))
    target = (320, 520)
    ok = False
    for attempt in range(3):
        xdo("windowactivate", "--sync", dm_window)
        xdo("mousemove", str(start[0]), str(start[1]))
        time.sleep(0.4)
        xdo("mousedown", "1")
        time.sleep(0.3)
        for step in range(1, 31):
            x = start[0] + (target[0] - start[0]) * step // 30
            y = start[1] + (target[1] - start[1]) * step // 30
            xdo("mousemove", str(x), str(y))
            time.sleep(0.05)
        time.sleep(0.5)
        subprocess.run(["import", "-window", "root", f"/shots/out-during-{attempt}.png"])
        xdo("mouseup", "1")
        # Firefox opens a dropped file in place of the page: the page's
        # title ("links") goes. (A text file's title is not its name.)
        end = time.time() + 6
        while time.time() < end and xdo("getwindowname", ff_window).startswith("links"):
            time.sleep(0.3)
        subprocess.run(["import", "-window", "root", f"/shots/out-after-{attempt}.png"])
        if not xdo("getwindowname", ff_window).startswith("links"):
            ok = True
            break
        print(f"  attempt {attempt + 1}: Firefox shows {xdo('getwindowname', ff_window)!r}")
    print(("ok    " if ok else "FAIL  ") + "a finished file dragged out of dm is opened by Firefox")
    # The list still takes a click: the release went to the drag, and dm
    # was told of it.
    call("ui.select", ids=[])
    xdo("mousemove", str(start[0]), str(start[1]))
    xdo("click", "1")
    time.sleep(1)
    selected = call("ui.inspect").get("result", {}).get("selected", [])
    clicks = row["id"] in selected
    print(("ok    " if clicks else "FAIL  ") + "and dm's list still takes a click after it", selected)
    return ok and clicks


def main():
    served = tempfile.mkdtemp(prefix="dm-drag-page-")
    pathlib.Path(served, "index.html").write_text(PAGE)
    pathlib.Path(served, NOTES).write_text("dragged out of dm\n")
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=served, **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    page = f"http://127.0.0.1:{server.server_address[1]}/index.html"

    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-drag-home-"))
    # No progress or done windows over the list the drags start from.
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(home / "out"),
        "show_progress": False,
        "show_complete": False,
    }))
    env = {**os.environ, "DM_HOME": str(home)}
    dm = subprocess.Popen([BINARY, "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=open("/shots/dm.log", "w"))
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
        out_ok = drag_out(home, dm_window, ff_window, page.rsplit("/", 1)[0] + "/" + NOTES)
        return 0 if ok and out_ok else 1
    finally:
        firefox.terminate()
        dm.terminate()
        server.shutdown()


if __name__ == "__main__":
    sys.exit(main())
