#!/usr/bin/env python3
"""Closing dm's window under Wayland with a tray present, run by
scripts/linux-wayland-close-check.sh inside the Linux container under sway.

On Wayland a program cannot hide its own window (winit's set_visible does
nothing), so dm used to mark the window hidden and stop drawing it while
the compositor went on showing it: a frozen ghost (KDE Plasma 6, AppImage).
This closes the window as a compositor does (xdg_toplevel close, here with
`swaymsg kill`) and checks that dm lives, that the window is not marked
hidden, and that it still answers and draws.
"""
import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import time

BINARY = os.environ.get("DM_BINARY", "/target/debug/dm-desktop")
HERE = pathlib.Path(__file__).parent
failed = 0


def check(what, ok):
    global failed
    print(("ok   " if ok else "FAIL ") + what, flush=True)
    failed += 0 if ok else 1


def wait(probe, seconds):
    end = time.time() + seconds
    while time.time() < end:
        got = probe()
        if got:
            return got
        time.sleep(0.3)
    return None


def windows(pid):
    out = subprocess.run(["swaymsg", "-t", "get_tree"], capture_output=True, text=True).stdout
    found = []

    def walk(node):
        if node.get("pid") == pid and node.get("type") in ("con", "floating_con") and node.get("name"):
            found.append(node)
        for child in node.get("nodes", []) + node.get("floating_nodes", []):
            walk(child)

    try:
        walk(json.loads(out))
    except ValueError:
        pass
    return found


class Control:
    def __init__(self, home):
        port = int(wait(lambda: (home / "control.port").exists() and (home / "control.port").read_text().strip(), 30))
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=10)
        self.file = self.sock.makefile("rw")
        self.call("auth", {"token": (home / "control.token").read_text().strip()})

    def call(self, method, params=None):
        self.file.write(json.dumps({"id": 1, "method": method, "params": params or {}}) + "\n")
        self.file.flush()
        reply = json.loads(self.file.readline())
        if not reply.get("ok"):
            raise RuntimeError(reply.get("error"))
        return reply.get("result")


def main():
    home = pathlib.Path(tempfile.mkdtemp())
    env = dict(os.environ, DM_HOME=str(home), HOME=str(home), XDG_SESSION_TYPE="wayland")
    env.pop("DISPLAY", None)
    watcher = subprocess.Popen([sys.executable, str(HERE / "tray_watcher.py")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    host = subprocess.Popen([sys.executable, str(HERE / "tray_host.py")],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    app = subprocess.Popen([BINARY, "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        check("dm opens a window under Wayland", bool(wait(lambda: windows(app.pid), 40)))
        control = Control(home)
        check("the window starts shown, not hidden", control.call("ui.inspect")["hidden"] is False)
        time.sleep(3)  # the tray item registers with the watcher
        subprocess.run(["swaymsg", f"[pid={app.pid}] kill"], capture_output=True)
        time.sleep(3)
        if app.poll() is not None:
            print("dm exited", app.returncode, app.stderr.read().decode()[-600:], flush=True)
            return failed + 1
        check("closing the window keeps dm running in the tray", True)
        state = control.call("ui.inspect")
        check("dm still answers", isinstance(state, dict))
        check("the window is not parked as hidden while the compositor still shows it",
              state["hidden"] is False)
        check("the compositor still has the window", bool(windows(app.pid)))
        # It keeps drawing: a change made over the control port is reflected.
        control.call("downloads.add", {"urls": ["http://127.0.0.1:9/never.bin"], "later": True})
        time.sleep(1)
        rows = control.call("ui.inspect")["rows"]
        check("and keeps up with changes", any("never.bin" in r["name"] for r in rows))
    finally:
        for p in (app, host, watcher):
            p.terminate()
        print(f"{failed} failed", flush=True)
    return failed


sys.exit(1 if main() else 0)
