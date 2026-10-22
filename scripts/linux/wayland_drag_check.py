#!/usr/bin/env python3
"""A link dragged onto dm's window under Wayland, with the compositor's own
pointer: run by scripts/linux-wayland-drag-check.sh inside the Linux
container, under sway (headless). A virtual pointer (`vpointer`, wlroots'
virtual-pointer protocol: an input device to the compositor) moves,
presses and releases as a hand does, so the drag is a real Wayland drag
between two clients, not something dm is told about.
"""
import http.server
import json
import os
import socket
import threading
import pathlib
import subprocess
import sys
import tempfile
import time

BINARY = os.environ.get("DM_BINARY", "/target/debug/dm-desktop")
LINK = "https://downloads.example.com/files/dragged-under-wayland.zip"
HERE = pathlib.Path(__file__).parent


def sway(*args):
    return subprocess.run(["swaymsg", *args], capture_output=True, text=True, timeout=20)


def tree():
    return json.loads(sway("-t", "get_tree").stdout or "{}")


def window(app):
    """The rectangle of the window whose title is `app`."""
    def walk(node):
        if node.get("name") == app and node.get("pid"):
            return node.get("rect")
        for child in node.get("nodes", []) + node.get("floating_nodes", []):
            found = walk(child)
            if found:
                return found
        return None
    return walk(tree())


def wait(probe, seconds):
    end = time.time() + seconds
    while time.time() < end:
        got = probe()
        if got:
            return got
        time.sleep(0.3)
    return None


class Pointer:
    """The virtual pointer, driven a line at a time."""

    def __init__(self):
        self.proc = subprocess.Popen(["/tmp/vpointer", "1280", "800"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, text=True)
        assert self.proc.stdout.readline().strip() == "ready", "no virtual pointer"

    def __call__(self, command):
        self.proc.stdin.write(command + "\n")
        self.proc.stdin.flush()
        self.proc.stdout.readline()


def control(home):
    """A call on dm's control protocol."""
    port = home / "control.port"
    wait(lambda: port.exists() and port.read_text().strip(), 30)
    conn = socket.create_connection(("127.0.0.1", int(port.read_text())), timeout=30)
    replies = conn.makefile("r")

    def call(method, **params):
        conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(replies.readline())

    call("auth", token=(home / "control.token").read_text().strip())
    return call


def drag(pointer, start, end):
    """Press at `start`, move as a hand does, and let go at `end`."""
    pointer(f"move {start[0]} {start[1]}")
    time.sleep(0.4)
    pointer("press")
    time.sleep(0.3)
    for step in range(1, 31):
        x = start[0] + (end[0] - start[0]) * step // 30
        y = start[1] + (end[1] - start[1]) * step // 30
        pointer(f"move {x} {y}")
        time.sleep(0.04)
    time.sleep(0.5)


def rows(home):
    try:
        return json.loads((home / "queue.json").read_text() or "[]")
    except (OSError, ValueError):
        return []


def drag_out(home, pointer, dm_rect, src_rect):
    """A finished file dragged from dm's list onto the other window."""
    served = tempfile.mkdtemp(prefix="dm-wl-files-")
    pathlib.Path(served, "notes.txt").write_text("dragged out of dm under Wayland\n")
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=served, **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/notes.txt"
    call = control(home)
    call("downloads.add", urls=[url])
    wait(lambda: any(r.get("url") == url and r.get("state", {}).get("kind") == "completed"
                     for r in rows(home)), 30)
    time.sleep(1)
    row = next((r for r in call("ui.inspect").get("result", {}).get("rows", [])
                if r.get("name") == "notes.txt" and r.get("rect")), None)
    if not row:
        print("FAIL  the finished row was not found")
        return False
    x0, y0, x1, y1 = row["rect"]
    start = (dm_rect["x"] + int(x0) + 40, dm_rect["y"] + int((y0 + y1) / 2))
    end = (src_rect["x"] + src_rect["width"] // 2, src_rect["y"] + src_rect["height"] // 3)
    received = pathlib.Path("/shots/received.txt")
    ok = False
    for attempt in range(3):
        drag(pointer, start, end)
        subprocess.run(["grim", f"/shots/out-during-{attempt}.png"], capture_output=True)
        pointer("release")
        time.sleep(2)
        got = received.read_text() if received.exists() else ""
        if "notes.txt" in got:
            ok = True
            break
        print(f"  attempt {attempt + 1}: nothing received", flush=True)
    print(("ok    " if ok else "FAIL  ") + "a finished file dragged out of dm under Wayland reaches the other window")
    # The list still takes a click: dm was told the button is up.
    call("ui.select", ids=[])
    pointer(f"move {start[0]} {start[1]}")
    pointer("press")
    pointer("release")
    time.sleep(1)
    selected = call("ui.inspect").get("result", {}).get("selected", [])
    clicks = row["id"] in selected
    print(("ok    " if clicks else "FAIL  ") + "and dm's list still takes a click after it", selected)
    return ok and clicks


def main():
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-wl-"))
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(home / "out"), "show_progress": False, "show_complete": False}))
    env = {**os.environ, "DM_HOME": str(home)}
    env.pop("DISPLAY", None)  # Wayland, not XWayland
    debug = {"WAYLAND_DEBUG": "1"} if os.environ.get("DM_WAYLAND_DEBUG") else {}
    dm = subprocess.Popen([BINARY, "--control", "0"], env={**env, **debug}, stdout=subprocess.DEVNULL,
                          stderr=open("/shots/dm.log", "w"))
    source = subprocess.Popen([sys.executable, str(HERE / "wayland_drag_source.py"), LINK],
                              env={**env, **debug, "GDK_BACKEND": "wayland"},
                              stdout=subprocess.DEVNULL, stderr=open("/shots/source.log", "w"))
    ok = False
    try:
        dm_rect = wait(lambda: window("dm"), 30)
        src_rect = wait(lambda: window("drag source"), 30)
        print(f"  dm at {dm_rect}, the source at {src_rect}", flush=True)
        if not dm_rect or not src_rect:
            print("FAIL  the windows never appeared")
            return 1
        start = (src_rect["x"] + src_rect["width"] // 2, src_rect["y"] + src_rect["height"] // 2)
        end = (dm_rect["x"] + dm_rect["width"] // 2, dm_rect["y"] + dm_rect["height"] // 2)
        pointer = Pointer()
        for attempt in range(3):
            drag(pointer, start, end)
            subprocess.run(["grim", f"/shots/during-{attempt}.png"], capture_output=True)
            pointer("release")
            time.sleep(2)
            subprocess.run(["grim", f"/shots/after-{attempt}.png"], capture_output=True)
            if any(r.get("url") == LINK for r in rows(home)):
                ok = True
                break
            print(f"  attempt {attempt + 1}: nothing queued", flush=True)
        print(("ok    " if ok else "FAIL  ") + "a link dragged onto dm's window under Wayland is queued")
        out_ok = drag_out(home, pointer, dm_rect, src_rect)
        return 0 if ok and out_ok else 1
    finally:
        source.terminate()
        dm.terminate()


if __name__ == "__main__":
    sys.exit(main())
