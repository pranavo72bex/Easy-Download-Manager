#!/usr/bin/env python3
"""A picture of dm's window, for checking a visual change by eye.

It starts the real binary on a throwaway home, with a
local server and a few downloads in the states a window has to draw (done,
running, paused, failed), drives it over the control protocol, and saves
PNGs:

    python3 scripts/ui-shot.py out-dir [command-id ...]

With command ids (as `commands.list` names them), each is run and the window
is photographed after it as well, so a dialog can be looked at.
Nothing outside the temporary home and the given folder is touched.
"""
import http.server
import json
import os
import pathlib
import shutil
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")
SIZE = 2 * 1024 * 1024


class Files(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        name = self.path.rsplit("/", 1)[-1]
        if name.startswith("missing"):
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(SIZE))
        self.end_headers()
        # A slow file stays running for the picture; the rest arrive at once.
        slow = name.startswith("slow")
        chunk = b"x" * 16384
        try:
            for _ in range(SIZE // len(chunk)):
                self.wfile.write(chunk)
                if slow:
                    time.sleep(0.05)
        except OSError:
            pass


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[1]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    commands = sys.argv[2:]

    server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Files)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_address[1]}"

    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-ui-shot-"))
    (home / "downloads").mkdir()
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(home / "downloads"), "categorize": True, "show_progress": False,
        "show_complete": False,
        # DM_SHOT_THEME=light|dark|classic|system picks the look to photograph.
        "theme": os.environ.get("DM_SHOT_THEME", "system"),
        # DM_SHOT_LANG=de|fr|... photographs it in that language.
        "language": os.environ.get("DM_SHOT_LANG", ""),
    }))
    env = dict(os.environ, DM_HOME=str(home))
    app = subprocess.Popen([BINARY, "--control", "0"], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        port_file = home / "control.port"
        for _ in range(100):
            if port_file.exists() and port_file.read_text().strip():
                break
            time.sleep(0.1)
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")

        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            started = time.time()
            reply = json.loads(replies.readline())
            if os.environ.get("DM_SHOT_TRACE"):
                print(method, round(time.time() - started, 2), str(reply)[:100])
            return reply

        call("auth", token=(home / "control.token").read_text().strip())
        for name in ("Ubuntu-24.04-desktop.iso", "Annual report.pdf", "holiday-video.mp4",
                     "song.mp3", "slow-archive.zip", "missing-installer.dmg"):
            call("downloads.add", urls=[f"{origin}/files/{name}"])
        time.sleep(3)
        # One paused, as a person leaves one.
        rows = call("queue.list").get("result") or []
        slow = [r["id"] for r in rows if "slow" in r.get("url", "")]
        if slow:
            call("ui.select", ids=slow)

        def shoot(name):
            shot = call("ui.screenshot", name=name)
            if shot.get("ok"):
                target = out / f"{name}.png"
                shutil.copy(shot["result"]["path"], target)
                print(target)
            else:
                print(f"{name}: {shot.get('error')}")

        shoot("window")
        for command in commands:
            reply = call("command.run", id=command)
            if not reply.get("ok"):
                print(f"{command}: {reply.get('error')}")
            time.sleep(0.6)
            shoot(command.replace(".", "-"))
    finally:
        app.terminate()
        try:
            app.wait(10)
        except subprocess.TimeoutExpired:
            app.kill()
        server.shutdown()
        shutil.rmtree(home, ignore_errors=True)


main()
