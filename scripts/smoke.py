#!/usr/bin/env python3
"""Smoke test of an installed dm, the same on Linux, Windows and macOS:

    python3 scripts/smoke.py /path/to/installed/dm-desktop

What a release's installer has to get right before anything else: the
installed binary starts, answers a browser as its native host, opens its
window (where there is a display), runs with no window, and downloads a
file from a local server byte-exact over several connections. Standard library only, so it runs on a bare CI machine right
after the installer. Exits non-zero, saying what failed, on the first
failure.
"""

import http.server
import json
import os
import pathlib
import shutil
import socket
import socketserver
import struct
import subprocess
import sys
import tempfile
import threading
import time

BINARY = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else None
BODY = bytes((i * 7 + 3) & 0xFF for i in range(4 * 1024 * 1024))


def fail(what):
    print(f"FAIL  {what}", flush=True)
    sys.exit(1)


def ok(what):
    print(f"ok    {what}", flush=True)


class Files(http.server.BaseHTTPRequestHandler):
    """One file, with ranges, so dm can split it across connections."""

    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        self.do_GET(head=True)

    def do_GET(self, head=False):
        start, end = 0, len(BODY) - 1
        asked = self.headers.get("Range")
        if asked and asked.startswith("bytes="):
            first, _, last = asked[6:].partition("-")
            start = int(first)
            end = min(int(last), end) if last else end
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(BODY)}")
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("ETag", '"smoke"')
        self.end_headers()
        if not head:
            try:
                self.wfile.write(BODY[start:end + 1])
            except OSError:
                pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def native_host(env):
    """A browser's first message to its native host, and dm's answer."""
    hello = json.dumps({"protocolVersion": 1, "type": "engine.hello"}).encode()
    # Pipes named, not capture_output: Python 3.6 (openSUSE Leap's) has none.
    done = subprocess.run([str(BINARY), "--native-host"], input=struct.pack("<I", len(hello)) + hello,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, timeout=30)
    if len(done.stdout) < 4:
        fail(f"no answer as a native host (exit {done.returncode}): {done.stderr.decode(errors='replace')[:300]}")
    size = struct.unpack("<I", done.stdout[:4])[0]
    answer = json.loads(done.stdout[4:4 + size])
    if not isinstance(answer, dict):
        fail(f"the native host's answer is not an object: {answer!r}")
    ok(f"answers a browser as its native host ({answer.get('type', '?')})")


def window(env):
    """The window opens: what the libraries a package must name are for.
    Wherever there is a display — always on macOS and Windows, on Linux
    when DISPLAY or WAYLAND_DISPLAY is set (xvfb-run in CI)."""
    if sys.platform.startswith("linux") and not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        print("--    the window: no display here, not checked", flush=True)
        return
    app = launch([str(BINARY)], env)
    try:
        time.sleep(6)
        if app.poll() is not None:
            fail(f"the window did not open (exit {app.returncode}): "
                 f"{app.stderr.read().decode(errors='replace')[:500]}")
        ok("opens its window")
    finally:
        stop(app)


def launch(argv, env):
    """dm started in a process group of its own, so stopping it stops what
    it started: an AppImage is a wrapper that runs dm as its child, and a
    signal to the wrapper alone left that dm running, which the next launch
    then handed over to and quit (0.2.0's CI)."""
    extra = {"start_new_session": True} if os.name == "posix" else {}
    return subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, **extra)


def stop(app):
    """Ends dm and everything in its group, then waits for it."""
    if app.poll() is not None:
        return
    if os.name == "posix":
        import signal
        try:
            os.killpg(app.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
    else:
        app.terminate()
    try:
        app.wait(15)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            import signal
            os.killpg(app.pid, signal.SIGKILL)
        else:
            app.kill()
    # The group's other members (the AppImage's dm) a moment to go.
    time.sleep(1)


def main():
    if BINARY is None or not BINARY.exists():
        fail(f"no dm at {BINARY}")
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-smoke-"))
    home, out = work / "home", work / "downloads"
    home.mkdir()
    (home / "settings.json").write_text(json.dumps({"download_dir": str(out), "connections": 4}))
    env = dict(os.environ, DM_HOME=str(home))
    server = Server(("127.0.0.1", 0), Files)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    app = None
    try:
        native_host(env)
        window(env)

        app = launch([str(BINARY), "--headless", "--control", "0"], env)
        port_file = home / "control.port"
        deadline = time.time() + 60
        while not (port_file.exists() and port_file.read_text().strip()):
            if app.poll() is not None:
                fail(f"dm exited at start ({app.returncode}): {app.stderr.read().decode(errors='replace')[:500]}")
            if time.time() > deadline:
                fail("dm never opened its control port")
            time.sleep(0.1)
        ok("starts with no window")

        control = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = control.makefile("r")

        def call(method, **params):
            control.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())

        call("auth", token=(home / "control.token").read_text().strip())
        url = f"http://127.0.0.1:{server.server_address[1]}/smoke.bin"
        call("downloads.add", urls=[url])
        deadline = time.time() + 120
        while True:
            rows = call("queue.list").get("result") or []
            state = (rows[0].get("state") or {}).get("kind") if rows else None
            if state == "completed":
                break
            if state == "failed" or time.time() > deadline:
                fail(f"the download did not complete: {rows[:1]}")
            time.sleep(0.2)
        path = pathlib.Path(rows[0]["path"])
        if path.read_bytes() != BODY:
            fail(f"{path} is not byte-exact")
        ok(f"downloads a file byte-exact ({len(BODY) // (1024 * 1024)} MiB, ranges)")
        control.close()
    finally:
        if app is not None:
            stop(app)
        server.shutdown()
        shutil.rmtree(work, ignore_errors=True)
    print("the installed dm works")


main()
