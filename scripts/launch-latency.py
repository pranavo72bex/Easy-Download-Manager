#!/usr/bin/env python3
"""How long from a click in the browser to dm's first request for the file.

Plays the browser: starts the dm binary as a native messaging host, sends the
`download.start` the extension sends for a click, and times how long until a
local file server sees dm ask for the file. Twice over:

  cold  no dm running, so the host starts one (`--desktop --headless`)
  warm  that dm still running, so the hand-off is only filed and read

Each case is run several times and the median is printed. Needs a built dm
and a display (Xvfb will do):

    cargo build --release -p dm-desktop
    xvfb-run -a python3 scripts/launch-latency.py desktop/target/release/dm-desktop
"""

import http.server
import json
import os
import shutil
import statistics
import struct
import subprocess
import sys
import tempfile
import threading
import time

SIZE = 4 * 1024 * 1024
BODY = bytes((i * 13 + 7) & 0xFF for i in range(SIZE))


class Server(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    seen = []

    def log_message(self, *args):
        pass

    def do_GET(self):
        Server.seen.append((time.monotonic(), self.headers.get("Range", "")))
        rng = self.headers.get("Range", "")
        a, b = 0, SIZE - 1
        if rng.startswith("bytes="):
            first, _, last = rng[6:].partition("-")
            a = int(first)
            b = min(int(last), SIZE - 1) if last else SIZE - 1
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {a}-{b}/{SIZE}")
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(b - a + 1))
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("ETag", '"latency"')
        self.end_headers()
        try:
            self.wfile.write(BODY[a : b + 1])
        except (BrokenPipeError, ConnectionResetError):
            pass


def frame(message):
    data = json.dumps(message).encode()
    return struct.pack("<I", len(data)) + data


def read_frame(pipe):
    head = pipe.read(4)
    if len(head) < 4:
        return None
    (n,) = struct.unpack("<I", head)
    return json.loads(pipe.read(n))


def click(dm, env, url, n):
    """Times one hand-off: host started, request sent, answer read."""
    before = len(Server.seen)
    clicked = time.monotonic()
    host = subprocess.Popen(
        [dm, "--native-host"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        env=env,
    )
    host.stdin.write(
        frame(
            {
                "protocolVersion": 1,
                "type": "download.start",
                "id": f"latency{n}",
                "url": url,
                "referrer": "",
                "caught": False,
            }
        )
    )
    host.stdin.flush()
    answer = read_frame(host.stdout)
    answered = time.monotonic()
    host.stdin.close()
    host.wait(timeout=10)
    deadline = time.monotonic() + 20
    while len(Server.seen) == before and time.monotonic() < deadline:
        time.sleep(0.001)
    if len(Server.seen) == before:
        raise SystemExit(f"dm never asked for the file (host said {answer})")
    first = Server.seen[before][0]
    return (answered - clicked) * 1000, (first - clicked) * 1000


def main():
    dm = os.path.abspath(sys.argv[1])
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 7
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Server)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_address[1]}"

    cold, warm = [], []
    for n in range(runs):
        home = tempfile.mkdtemp(prefix="dm-latency-")
        env = dict(os.environ, DM_HOME=home)
        os.makedirs(os.path.join(home), exist_ok=True)
        with open(os.path.join(home, "settings.json"), "w") as f:
            json.dump({"download_dir": os.path.join(home, "out")}, f)
        try:
            cold.append(click(dm, env, f"{base}/cold{n}.bin", n * 2))
            time.sleep(1.5)  # the first download finishes; dm stays
            warm.append(click(dm, env, f"{base}/warm{n}.bin", n * 2 + 1))
        finally:
            # Asked to end through its inbox, as an uninstall asks it.
            inbox = os.path.join(home, "inbox")
            os.makedirs(inbox, exist_ok=True)
            open(os.path.join(inbox, "quit"), "w").close()
            time.sleep(1.5)
            subprocess.run(["pkill", "-f", f"{dm} --desktop"], check=False)
            shutil.rmtree(home, ignore_errors=True)

    for name, rows in (("cold", cold), ("warm", warm)):
        host = statistics.median(r[0] for r in rows)
        first = statistics.median(r[1] for r in rows)
        worst = max(r[1] for r in rows)
        print(
            f"{name}: host answered {host:6.1f} ms, first request {first:6.1f} ms "
            f"(median of {len(rows)}; slowest {worst:.1f} ms)"
        )
    server.shutdown()


if __name__ == "__main__":
    main()
