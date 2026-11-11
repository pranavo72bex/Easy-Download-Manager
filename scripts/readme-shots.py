#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The README's pictures of the window dm ships, taken from the real window.

    python3 scripts/readme-shots.py             # into docs/
    python3 scripts/readme-shots.py some/folder

dm's GPUI window (`desktop/target/debug/dm-gpui`, or DM_BINARY) is started
with a home of its own, in the light look and then the dark one, and given
four downloads from a server on 127.0.0.1, as a person would have them: one
finished, one running (its server sends it slowly), one added for later
and one whose link no longer works. Then it is pictured by the system, as it
stands on screen: the window (screenshot.png), a download's progress window
(progress.png) and Options (settings.png), each with its -dark twin. The
download folder is shown as ~/Downloads, nothing of the computer that made
them. macOS, as its window is pictured with `screencapture`; standard
library only.
"""
import http.server
import json
import os
import pathlib
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-gpui")
OUT = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "docs"
KiB = 1024


class Files(http.server.BaseHTTPRequestHandler):
    """file.zip and later.mkv whole; large.iso in ranges, as a server that
    lets a download resume answers, slowly on every connection, so it is
    still running while it is pictured; gone.iso not there."""

    protocol_version = "HTTP/1.1"
    sizes = {"file.zip": 256 * KiB, "large.iso": 48 * 1024 * KiB, "later.mkv": 2048 * KiB}

    def log_message(self, *_):
        pass

    def do_GET(self):
        name = self.path.rsplit("/", 1)[-1]
        if name not in self.sizes:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        size = self.sizes[name]
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
        if name == "large.iso":
            # About 48 KB/s on each connection, for longer than the pictures take.
            try:
                for _ in range((end - start + 1) // (4 * KiB)):
                    self.wfile.write(b"\0" * (4 * KiB))
                    self.wfile.flush()
                    time.sleep(0.08)
            except OSError:
                pass
            return
        self.wfile.write(b"\0" * (end - start + 1))


class Dm:
    def __init__(self, home):
        self.home = home
        self.proc = subprocess.Popen([str(BINARY), "--control", "0"], env=dict(os.environ, DM_HOME=str(home)),
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        port = home / "control.port"
        for _ in range(200):
            if port.exists() and port.read_text().strip():
                break
            time.sleep(0.1)
        self.conn = socket.create_connection(("127.0.0.1", int(port.read_text())), timeout=60)
        self.replies = self.conn.makefile("r", encoding="utf-8")
        self.call("auth", token=(home / "control.token").read_text().strip())

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        reply = json.loads(self.replies.readline())
        if not reply.get("ok"):
            raise RuntimeError(f"{method}: {reply.get('error')}")
        return reply.get("result")

    def rows(self):
        return self.call("queue.list") or []

    def wait(self, name, kinds, seconds=30):
        end = time.time() + seconds
        while time.time() < end:
            row = next((r for r in self.rows() if r["url"].endswith("/" + name)), None)
            if row and row["state"]["kind"] in kinds:
                return row
            time.sleep(0.2)
        raise RuntimeError(f"{name} never became {kinds}")

    def quit(self):
        self.proc.terminate()
        self.proc.wait(20)


def in_front(dm):
    """dm's windows in front, as a person looking at them has them: a window
    behind another is drawn as one not in use."""
    dm.call("ui.window", to="shown")
    subprocess.run(["osascript", "-e", 'tell application "System Events" to set frontmost of '
                    f"(first process whose unix id is {dm.proc.pid}) to true"], capture_output=True)
    time.sleep(1.0)


def picture_dialog(dm, title, path):
    """A dialog's own window, pictured from the screen over the rectangle dm
    says it stands on (`screencapture -R`, in points), once it has stopped
    fitting itself to what it holds."""
    last, steady = None, 0
    for _ in range(60):
        shown = [d for d in dm.call("ui.windows").get("dialogs", []) if title in d.get("title", "")]
        box = shown[0].get("outer") if shown else None
        steady = steady + 1 if box and box == last else 0
        last = box
        if steady >= 4:
            x0, y0, x1, y1 = box
            subprocess.run(["screencapture", "-x", f"-R{x0},{y0},{x1 - x0},{y1 - y0}", str(path)], check=True)
            return
        time.sleep(0.25)
    raise RuntimeError(f"no {title} window to picture")


def take(look, base, work):
    home = work / look
    downloads = home / "downloads"
    downloads.mkdir(parents=True)
    # The look, and the pop-ups a new install would open on every download off,
    # so the window is pictured as a person leaves it.
    (home / "settings.json").write_text(json.dumps({
        "theme": look, "download_dir": str(downloads), "show_progress": False, "show_complete": False,
    }))
    dm = Dm(home)
    try:
        suffix = "" if look == "light" else "-dark"
        dm.call("downloads.add", urls=[f"{base}/file.zip"])
        dm.wait("file.zip", {"completed"})
        dm.call("downloads.add", urls=[f"{base}/large.iso"])
        running = dm.wait("large.iso", {"running"})
        dm.call("downloads.add", urls=[f"{base}/later.mkv"], later=True)
        dm.call("downloads.add", urls=[f"{base}/gone.iso"])
        dm.wait("gone.iso", {"failed"})
        time.sleep(1.0)
        # A link that answers 404 says so in a window of its own: closed, as a
        # person closes it.
        for _ in range(10):
            if "OK" not in [t["text"] for t in dm.call("ui.texts").get("texts", []) if t.get("window") == "dialog"]:
                break
            dm.call("ui.click", text="OK")
            time.sleep(0.5)
        # A few seconds in, so its pieces and speed show.
        time.sleep(4)
        dm.call("ui.select", ids=[running["id"]])
        in_front(dm)
        shot = dm.call("ui.screenshot", name=f"readme{suffix}")
        shutil.copy(shot["path"], OUT / f"screenshot{suffix}.png")
        dm.call("ui.click", text="large.iso", double=True)
        in_front(dm)
        picture_dialog(dm, "large.iso", OUT / f"progress{suffix}.png")
        dm.call("ui.key", key="Escape")
        time.sleep(0.6)
        dm.call("command.run", id="downloads.options")
        in_front(dm)
        picture_dialog(dm, "Options", OUT / f"settings{suffix}.png")
        dm.call("ui.key", key="Escape")
    finally:
        dm.quit()


def main():
    if sys.platform != "darwin":
        sys.exit("the README's pictures are taken on macOS, where dm's window is pictured by the system")
    if not BINARY.exists():
        sys.exit(f"no dm at {BINARY}: cd desktop && cargo build -p dm-gpui")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Files)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_address[1]}/files"
    OUT.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-readme-"))
    try:
        for look in ("light", "dark"):
            take(look, base, work)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    for name in ("screenshot", "progress", "settings"):
        print(f"{OUT / name}.png and {name}-dark.png")


main()
