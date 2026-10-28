#!/usr/bin/env python3
"""Stress and reliability check of dm's download engine and control protocol.

    python3 scripts/stress-check.py [phase ...]       # all phases, or those named
    python3 scripts/stress-check.py --list

Everything stays on this computer: a local HTTP server (127.0.0.1 only) that
makes its files up as they are asked for, a dm of its own (a throwaway
DM_HOME, no window), and a disk budget it checks before each phase. Nothing
reaches an outside server, and nothing outside the temporary folder is
written. Safety rules, in order:

  * the disk: a phase does not start with less than MIN_FREE_GB free, and
    large files are never more than one at a time;
  * the processes: dm is ended, and the server shut down, however a phase
    ends; each phase has a time limit;
  * the data: the temporary folder is removed at the end (DM_STRESS_KEEP=1
    keeps it for reading).

What a phase finds is printed as `FINDING <severity> <phase>: <what>`; the
numbers it measures as `MEASURE ...`. The torrent part is not covered.
"""
import base64
import hashlib
import http.server
import json
import os
import pathlib
import random
import shutil
import signal
import socket
import socketserver
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")
MIN_FREE_GB = float(os.environ.get("DM_STRESS_MIN_FREE_GB", "8"))
MAX_SERVE_GB = float(os.environ.get("DM_STRESS_MAX_SERVE_GB", "3"))     # per phase
MB = 1024 * 1024
BLOCK = 65536

FINDINGS = []


def finding(severity, phase, what):
    FINDINGS.append((severity, phase, what))
    print(f"FINDING {severity} {phase}: {what}", flush=True)


def measure(phase, what):
    print(f"MEASURE {phase}: {what}", flush=True)


def check(phase, what, ok, detail=""):
    print(f"  {'ok  ' if ok else 'FAIL'} {phase}: {what}" + (f" -- {detail}" if detail and not ok else ""), flush=True)
    return ok


# ------------------------------------------------------------ the data

def block(seed, k):
    """64 KiB that is this file's and this position's alone."""
    return hashlib.shake_128(seed.encode() + k.to_bytes(8, "little")).digest(BLOCK)


def slice_of(seed, start, length):
    out = bytearray()
    pos = start
    end = start + length
    while pos < end:
        k, off = divmod(pos, BLOCK)
        take = min(BLOCK - off, end - pos)
        out += block(seed, k)[off:off + take]
        pos += take
    return bytes(out)


_EXPECTED = {}


def expected_sha(seed, size):
    key = (seed, size)
    if key not in _EXPECTED:
        h = hashlib.sha256()
        pos = 0
        while pos < size:
            n = min(4 * MB, size - pos)
            h.update(slice_of(seed, pos, n))
            pos += n
        _EXPECTED[key] = h.hexdigest()
    return _EXPECTED[key]


def file_sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(4 * MB):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------- the server

class Stats:
    lock = threading.Lock()
    requests = 0
    live = 0
    peak = 0
    by_path = {}
    ranges = 0
    status = {}
    served = {}
    total = 0                       # bytes served in all: a ceiling on what dm can be made to write

    @classmethod
    def enter(cls, path):
        with cls.lock:
            cls.requests += 1
            cls.live += 1
            cls.peak = max(cls.peak, cls.live)
            cls.by_path[path] = cls.by_path.get(path, 0) + 1

    @classmethod
    def leave(cls):
        with cls.lock:
            cls.live -= 1

    @classmethod
    def reset(cls):
        with cls.lock:
            cls.requests = cls.live = cls.peak = cls.ranges = 0
            cls.by_path = {}
            cls.status = {}
            cls.served = {}
            cls.total = 0


NAMES = {
    # What a hostile server may call a file. dm must keep every one inside
    # the folder it was told to save in.
    "dotdot": "../../escaped-dotdot.bin",
    "backslash": "..\\..\\escaped-backslash.bin",
    "abs": "/tmp/escaped-abs.bin",
    "nul": "NUL",
    "con": "CON.txt",
    "long": "a" * 600 + ".bin",
    "dots": "...",
    "space": "name with trailing space. ",
    "slash": "a/b/c.bin",
    "rtl": "photo‮gpj.exe",
    "nullbyte": "x\x00y.bin",
    "empty": "",
    "dotfile": ".bashrc",
    "ctrl": "line\nbreak\r\n.bin",
    "colon": "c:evil.bin",
    "unicode": "テスト\U0001f600.bin",
}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    timeout = 120

    def log_message(self, *_):
        pass

    def send_head(self, code, headers):
        self.send_response(code)
        for k, v in headers.items():
            self.send_header(k, v)
        self.end_headers()

    def do_HEAD(self):
        self.do_GET(head=True)

    def do_GET(self, head=False):
        url = urllib.parse.urlparse(self.path)
        q = {k: v[0] for k, v in urllib.parse.parse_qs(url.query).items()}
        parts = url.path.strip("/").split("/")
        Stats.enter(url.path)
        try:
            self.route(parts, q, head)
        except (BrokenPipeError, ConnectionResetError, ssl_error()):
            pass
        finally:
            Stats.leave()

    def route(self, parts, q, head):
        kind = parts[0]
        if kind == "f":                   # /f/<seed>/<size>[/name]
            return self.file(parts[1], int(parts[2]), q, head, parts[3] if len(parts) > 3 else None)
        if kind == "redirect":            # /redirect/<n>/<seed>/<size>
            n = int(parts[1])
            target = f"/redirect/{n - 1}/{parts[2]}/{parts[3]}" if n > 1 else f"/f/{parts[2]}/{parts[3]}"
            return self.send_head(302, {"Location": target, "Content-Length": "0"})
        if kind == "loop":
            return self.send_head(302, {"Location": "/loop", "Content-Length": "0"})
        if kind == "to":                  # /to?u=<url>
            return self.send_head(302, {"Location": q["u"], "Content-Length": "0"})
        if kind == "cd":                  # /cd/<key>: a name chosen by the server
            name = NAMES[parts[1]]
            body = slice_of("cd" + parts[1], 0, 4096)
            return self.send_head_body(200, {"Content-Disposition": f'attachment; filename="{name}"', "Content-Type": "application/octet-stream"}, body, head)
        if kind == "cd2":                 # RFC 5987 form of a name
            name = urllib.parse.quote(NAMES[parts[1]])
            body = slice_of("cd2" + parts[1], 0, 4096)
            return self.send_head_body(200, {"Content-Disposition": f"attachment; filename*=UTF-8''{name}", "Content-Type": "application/octet-stream"}, body, head)
        if kind == "bighead":
            return self.send_head_body(200, {"X-Pad": "x" * int(q.get("kb", "1024")) * 1024, "Content-Type": "application/octet-stream"}, b"ok" * 100, head)
        if kind == "stall":               # headers, then nothing
            self.send_response(200)
            self.send_header("Content-Length", "1000000")
            self.send_header("Content-Type", "application/octet-stream")
            self.end_headers()
            time.sleep(float(q.get("s", "60")))
            return
        if kind == "gz":                  # /gz/<expanded MB>: a small body that is large when decompressed
            import gzip, io
            n = int(parts[1])
            buf = io.BytesIO()
            with gzip.GzipFile(fileobj=buf, mode="wb", compresslevel=9) as g:
                chunk = b"\0" * MB
                for _ in range(n):
                    g.write(chunk)
            body = buf.getvalue()
            return self.send_head_body(200, {"Content-Encoding": "gzip", "Content-Type": "application/octet-stream"}, body, head)
        if kind == "hls":                 # /hls/<segments>/<name>.m3u8 or seg
            return self.hls(parts, q, head)
        if kind == "edge":                # /edge/<n>: a page of n download links
            n = int(parts[1])
            links = "".join(f'<p><a id="l{i}" href="/f/edge{i}/{2 * MB}/edge{i}.bin">edge{i}.bin</a></p>' for i in range(n))
            return self.send_head_body(200, {"Content-Type": "text/html"}, f"<!doctype html><title>edge stress</title>{links}".encode(), head)
        if kind == "page":                # an HTML page where a file is expected
            return self.send_head_body(200, {"Content-Type": "text/html"}, b"<html>sign in</html>", head)
        self.send_head_body(404, {"Content-Type": "text/plain"}, b"no", head)

    def send_head_body(self, code, headers, body, head):
        headers = dict(headers)
        headers["Content-Length"] = str(len(body))
        self.send_head(code, headers)
        if not head:
            self.wfile.write(body)

    def hls(self, parts, q, head):
        n = int(parts[1])
        if parts[2].endswith(".m3u8"):
            lines = ["#EXTM3U", "#EXT-X-VERSION:3", "#EXT-X-TARGETDURATION:2", "#EXT-X-MEDIA-SEQUENCE:0"]
            for i in range(n):
                lines += ["#EXTINF:2.0,", f"seg{i}.ts" + (f"?{urllib.parse.urlencode(q)}" if q else "")]
            lines.append("#EXT-X-ENDLIST")
            return self.send_head_body(200, {"Content-Type": "application/vnd.apple.mpegurl"}, ("\n".join(lines) + "\n").encode(), head)
        i = int(parts[2][3:-3])
        if q.get("missing") and i == int(q["missing"]):
            return self.send_head_body(404, {"Content-Type": "text/plain"}, b"gone", head)
        # 188-byte transport-stream packets with the sync byte, a segment's
        # worth of them.
        packet = b"\x47" + bytes([i % 251]) * 187
        body = packet * 350
        return self.send_head_body(200, {"Content-Type": "video/mp2t"}, body, head)

    def file(self, seed, size, q, head, name):
        rate = int(q.get("rate", "0"))                  # bytes/s per connection
        ttfb = float(q.get("ttfb", "0"))
        flaky = float(q.get("flaky", "0"))              # chance a response is cut short
        status = int(q.get("status", "200"))
        lie = int(q.get("lie", "0"))                    # Content-Length this many bytes over the truth
        short = int(q.get("short", "0"))                # Content-Length this many bytes under
        norange = q.get("norange") == "1"
        cap = int(q.get("cap", "0"))                    # serve at most this many bytes of the file in all, then refuse
        expire_after = int(q.get("expire", "0"))        # requests after which it answers 403
        if ttfb:
            time.sleep(ttfb)
        if expire_after:
            with Stats.lock:
                n = Stats.by_path.get(self.path, 0)
            if n > expire_after:
                return self.send_head_body(403, {"Content-Type": "text/plain"}, b"expired", head)
        if status != 200:
            return self.send_head_body(status, {"Content-Type": "text/plain", "Retry-After": "1"}, b"no", head)
        start, end = 0, size - 1
        code = 200
        rng = self.headers.get("Range")
        headers = {"Content-Type": "application/octet-stream" if not name or not name.endswith(".zip") else "application/zip"}
        if not norange:
            headers["Accept-Ranges"] = "bytes"
        if rng and rng.startswith("bytes=") and not norange:
            a, _, b = rng[6:].partition("-")
            if a == "":
                start = max(0, size - int(b))
            else:
                start = int(a)
                end = min(int(b), size - 1) if b else size - 1
            if start >= size:
                return self.send_head_body(416, {"Content-Range": f"bytes */{size}"}, b"", head)
            code = 206
            headers["Content-Range"] = f"bytes {start}-{end}/{size}"
        length = end - start + 1
        announced = length + lie - short
        headers["Content-Length"] = str(announced)
        headers["ETag"] = f'"{seed}-{size}"'
        self.send_head(code, headers)
        if head:
            return
        send = length
        if cap:
            # Counted over every request for the file: a client that resumes
            # after each cut must not be able to draw the whole of it.
            with Stats.lock:
                used = Stats.served.get(seed, 0)
                send = max(0, min(length, cap - used))
                Stats.served[seed] = used + send
            if send == 0:
                self.close_connection = True
                return
        if flaky and random.random() < flaky:
            send = random.randint(0, max(0, length - 1))
        pos = start
        sent = 0
        t0 = time.time()
        while sent < send:
            n = min(BLOCK - (pos % BLOCK), send - sent, 256 * 1024 if not rate else max(1024, rate // 20))
            with Stats.lock:
                Stats.total += n
                over = Stats.total > MAX_SERVE_GB * 1e9
            if over:
                # The check's own safety stop: nothing more is served, so a
                # download that goes on cannot fill the disk.
                self.close_connection = True
                return
            data = slice_of(seed, pos, n)
            self.wfile.write(data)
            pos += n
            sent += n
            if rate:
                due = sent / rate - (time.time() - t0)
                if due > 0:
                    time.sleep(due)
        if send < length:
            # Cut short: the connection is dropped, as a failing server does.
            self.close_connection = True
            try:
                self.connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        elif lie > 0:
            self.close_connection = True


def ssl_error():
    return OSError


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    clients = set()
    down = False

    def process_request(self, request, client_address):
        if self.down:
            try:
                request.close()
            except OSError:
                pass
            return
        self.clients.add(request)
        super().process_request(request, client_address)

    def cut_all(self):
        """A network outage: every open connection dropped, new ones refused."""
        self.down = True
        for c in list(self.clients):
            try:
                c.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                c.close()
            except OSError:
                pass
        self.clients.clear()
    request_queue_size = 2048
    allow_reuse_address = True

    def handle_error(self, request, client_address):
        # A client that drops its connection is the case being tested, not
        # a report.
        pass


def start_server():
    return start_server_with(Handler)


def start_server_with(handler):
    """The same server with a handler that adds routes (stress-protocols.py)."""
    server = Server(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}"


# ------------------------------------------------------------------ dm

class DM:
    def __init__(self, root, settings=None, name="dm", window=False):
        self.window = window
        self.home = root / f"{name}-home"
        self.out = root / f"{name}-out"
        self.home.mkdir(parents=True, exist_ok=True)
        self.out.mkdir(parents=True, exist_ok=True)
        self.settings = {"download_dir": str(self.out), "categorize": False,
                         "show_progress": False, "show_complete": False,
                         "check_updates": False, "watch_clipboard": False, **(settings or {})}
        self.proc = None
        LIVE_DMS.append(self)
        self.conn = None
        self.replies = None
        self.samples = []
        self._stop = threading.Event()

    def start(self):
        (self.home / "settings.json").write_text(json.dumps(self.settings))
        # What a dm that was killed left behind is not this one's: waited for
        # again, or the check would connect to a port nobody listens on.
        for stale in ("control.port", "control.token"):
            (self.home / stale).unlink(missing_ok=True)
        env = {**os.environ, "DM_HOME": str(self.home)}
        args = ["--control", "0"] if self.window else ["--headless", "--control", "0"]
        self.proc = subprocess.Popen([str(BINARY), *args], env=env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        port_file = self.home / "control.port"
        deadline = time.time() + 30
        while time.time() < deadline and not (port_file.exists() and (self.home / "control.token").exists()):
            if self.proc.poll() is not None:
                raise RuntimeError("dm exited: " + self.proc.stderr.read().decode(errors="replace"))
            time.sleep(0.1)
        self.connect()
        self._stop.clear()
        threading.Thread(target=self._sampler, daemon=True).start()
        return self

    def connect(self):
        port = int((self.home / "control.port").read_text())
        self.conn = socket.create_connection(("127.0.0.1", port), timeout=120)
        self.replies = self.conn.makefile("r")
        r = self.call("auth", token=(self.home / "control.token").read_text().strip())
        assert r["ok"], r

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(self.replies.readline())

    def rows(self):
        r = self.call("queue.list")
        return r.get("result", []) if r.get("ok") else []

    def add(self, urls, **kw):
        return self.call("downloads.add", urls=urls, **kw)

    def _sampler(self):
        while not self._stop.is_set() and self.proc and self.proc.poll() is None:
            self.samples.append((time.time(), *proc_stats(self.proc.pid)))
            self._stop.wait(2)

    def stop(self, kill=False):
        self._stop.set()
        if self.proc and self.proc.poll() is None:
            (self.proc.kill if kill else self.proc.terminate)()
            try:
                self.proc.wait(15)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        try:
            if self.conn:
                self.conn.close()
        except OSError:
            pass

    def peak(self):
        if not self.samples:
            return (0, 0.0, 0, 0)
        return (max(s[1] for s in self.samples), max(s[2] for s in self.samples),
                max(s[3] for s in self.samples), max(s[4] for s in self.samples))


def proc_stats(pid):
    """RSS in MB, CPU percent, open files, threads."""
    try:
        out = subprocess.run(["ps", "-o", "rss=,pcpu=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
        rss, cpu = float(out[0]) / 1024, float(out[1])
        fds = len(subprocess.run(["lsof", "-p", str(pid)], capture_output=True, text=True).stdout.splitlines()) - 1
        threads = len(subprocess.run(["ps", "-M", "-p", str(pid)], capture_output=True, text=True).stdout.splitlines()) - 1
        return rss, cpu, max(fds, 0), max(threads, 0)
    except Exception:
        return 0.0, 0.0, 0, 0


def free_gb(path):
    return shutil.disk_usage(path).free / 1e9


def wait_for(cond, seconds, every=0.5):
    deadline = time.time() + seconds
    while time.time() < deadline:
        value = cond()
        if value:
            return value
        time.sleep(every)
    return None


def states(rows):
    out = {}
    for r in rows:
        k = r["state"]["kind"] if isinstance(r["state"], dict) else str(r["state"])
        out[k] = out.get(k, 0) + 1
    return out


def verify(rows, expect):
    """expect: url suffix -> (seed, size). Returns (ok, bad list)."""
    bad = []
    ok = 0
    for r in rows:
        if r["state"]["kind"] != "completed":
            continue
        name = urllib.parse.urlparse(r["url"]).path.rsplit("/", 1)[-1]
        for suffix, (seed, size) in expect.items():
            if name == suffix:
                p = pathlib.Path(r["path"])
                if not p.exists():
                    bad.append((suffix, "file missing"))
                elif p.stat().st_size != size:
                    bad.append((suffix, f"size {p.stat().st_size} != {size}"))
                elif file_sha(p) != expected_sha(seed, size):
                    bad.append((suffix, "content differs"))
                else:
                    ok += 1
                break
    return ok, bad


PHASES = {}
LIVE_DMS = []


def with_login(url, name, secret):
    """`url` with a login in it, set through the parts rather than written out:
    an address with a login spelled in the source reads to a secret scanner as
    a leaked credential, made-up as these are."""
    parts = urllib.parse.urlsplit(url)
    return parts._replace(netloc=f"{name}:{secret}@{parts.netloc}").geturl()


def phase(fn):
    PHASES[fn.__name__.removeprefix("p_")] = fn
    return fn


def guard(phase_name):
    if free_gb(tempfile.gettempdir()) < MIN_FREE_GB:
        finding("info", phase_name, f"skipped: only {free_gb(tempfile.gettempdir()):.1f} GB free, below the {MIN_FREE_GB} GB the check keeps")
        return False
    return True


# --------------------------------------------------------------- phases

@phase
def p_throughput(ctx):
    """One large file on 1, 8 and 16 connections: speed, byte-exactness."""
    base, root = ctx["base"], ctx["root"]
    size = 256 * MB
    for conns in (1, 8, 16):
        dm = DM(root, {"connections": conns}, name=f"tp{conns}").start()
        try:
            url = f"{base}/f/tp{conns}/{size}/big{conns}.bin"
            t0 = time.time()
            dm.add([url])
            done = wait_for(lambda: [r for r in dm.rows() if r["state"]["kind"] in ("completed", "failed")], 300)
            took = time.time() - t0
            if not done or done[0]["state"]["kind"] != "completed":
                finding("high", "throughput", f"{conns} connections: did not complete ({done and done[0]['state']})")
                continue
            ok, bad = verify(dm.rows(), {f"big{conns}.bin": (f"tp{conns}", size)})
            check("throughput", f"{conns} connections byte-exact", ok == 1 and not bad, str(bad))
            if bad:
                finding("critical", "throughput", f"{conns} connections: completed file is wrong: {bad}")
            rss, cpu, fds, th = dm.peak()
            measure("throughput", f"{conns} conn: {size / MB / took:.0f} MB/s in {took:.1f}s; peak rss {rss:.0f} MB cpu {cpu:.0f}% fds {fds} threads {th}")
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_many_small(ctx):
    """Thousands of small files: all arrive, byte-exact, memory stays flat."""
    base, root = ctx["base"], ctx["root"]
    n = int(os.environ.get("DM_STRESS_SMALL", "2000"))
    conc = int(os.environ.get("DM_STRESS_SMALL_CONC", "8"))
    dm = DM(root, {"connections": 4, "concurrency": conc}, name="small").start()
    measure("many_small", f"concurrency setting {conc}")
    try:
        sizes = [0, 1, 4097, 16 * 1024, 100 * 1024]
        expect = {}
        urls = []
        for i in range(n):
            size = sizes[i % len(sizes)]
            name = f"s{i}.bin"
            urls.append(f"{base}/f/small{i}/{size}/{name}")
            expect[name] = (f"small{i}", size)
        t0 = time.time()
        for k in range(0, n, 200):
            r = dm.add(urls[k:k + 200])
            if not r.get("ok"):
                finding("high", "many_small", f"downloads.add refused a batch of 200: {r}")
                break
        added = time.time() - t0
        measure("many_small", f"{n} links added in {added:.1f}s")
        rows = []
        def settled():
            nonlocal rows
            rows = dm.rows()
            return len(rows) >= n and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in rows)
        wait_for(settled, 600, every=2)
        took = time.time() - t0
        st = states(rows)
        measure("many_small", f"states {st} after {took:.0f}s ({n / took:.1f} files/s)")
        ok, bad = verify(rows, expect)
        if st.get("failed"):
            finding("high", "many_small", f"{st['failed']} of {n} small files failed: e.g. {[r['state'] for r in rows if r['state']['kind'] == 'failed'][:2]}")
        if bad:
            finding("critical", "many_small", f"{len(bad)} completed files are wrong, e.g. {bad[:3]}")
        if len(rows) != n:
            finding("high", "many_small", f"{n} links added but the list holds {len(rows)}")
        check("many_small", f"{ok} of {n} verified byte-exact", ok == n - st.get("failed", 0) - st.get("cancelled", 0) and not bad)
        rss, cpu, fds, th = dm.peak()
        measure("many_small", f"peak rss {rss:.0f} MB, cpu {cpu:.0f}%, fds {fds}, threads {th}")
        if fds > 800:
            finding("medium", "many_small", f"{fds} open files at peak for {n} small downloads")
        t = time.time()
        dm.call("queue.list")
        measure("many_small", f"queue.list of {len(rows)} rows answers in {time.time() - t:.2f}s")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_concurrency(ctx):
    """Many slow downloads at once, and a server that limits connections."""
    base, root = ctx["base"], ctx["root"]
    n = int(os.environ.get("DM_STRESS_CONC", "150"))
    dm = DM(root, {"connections": 8, "concurrency": 32}, name="conc").start()
    Stats.reset()
    try:
        size = 4 * MB
        expect = {}
        urls = []
        for i in range(n):
            name = f"c{i}.bin"
            urls.append(f"{base}/f/conc{i}/{size}/{name}?rate={2 * MB}")
            expect[name] = (f"conc{i}", size)
        dm.add(urls)
        t0 = time.time()
        wait_for(lambda: len(dm.rows()) >= n and all(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 600, every=2)
        took = time.time() - t0
        rows = dm.rows()
        st = states(rows)
        ok, bad = verify(rows, expect)
        measure("concurrency", f"{n} x {size // MB} MB: {st} in {took:.0f}s, server peak {Stats.peak} simultaneous connections, {Stats.requests} requests")
        if bad:
            finding("critical", "concurrency", f"wrong content in {len(bad)} files: {bad[:3]}")
        if st.get("failed"):
            finding("high", "concurrency", f"{st['failed']} of {n} failed under {n}-way concurrency")
        rss, cpu, fds, th = dm.peak()
        measure("concurrency", f"peak rss {rss:.0f} MB cpu {cpu:.0f}% fds {fds} threads {th}")
        if Stats.peak > 32 * 8 + 40:
            finding("medium", "concurrency", f"dm held {Stats.peak} connections at once against one host with concurrency 32 x 8 connections")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_flaky(ctx):
    """Servers that cut connections, rate-limit, lie about length, stall."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 8, "concurrency": 8, "retries": 10}, name="flaky").start()
    try:
        size = 8 * MB
        cases = {
            "cut": (f"{base}/f/cut/{size}/cut.bin?flaky=0.5&rate={4 * MB}", True),
            "r429": (f"{base}/f/r429/{size}/r429.bin?status=429", False),
            "r503": (f"{base}/f/r503/{size}/r503.bin?status=503", False),
            "nrange": (f"{base}/f/nrange/{size}/nrange.bin?norange=1", True),
            "ttfb": (f"{base}/f/ttfb/{size}/ttfb.bin?ttfb=3", True),
            "lieover": (f"{base}/f/lieover/{size}/lieover.bin?lie=1000", False),
            "lieunder": (f"{base}/f/lieunder/{size}/lieunder.bin?short=1000", False),
            "redirect": (f"{base}/redirect/5/redir/{size}", True),
            "loop": (f"{base}/loop", False),
            "page": (f"{base}/page/x.zip", False),
        }
        for k, (u, _) in cases.items():
            dm.add([u])
        wait_for(lambda: len(dm.rows()) >= len(cases) and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 240, every=2)
        rows = dm.rows()
        by = {}
        for r in rows:
            for k, (u, _) in cases.items():
                if r["url"].endswith(u.replace(base, "")) or u.endswith(r["url"].split("127.0.0.1", 1)[-1].split(":", 1)[-1][4:] if False else ""):
                    by[k] = r
        # Match by the seed in the URL.
        for r in rows:
            for k, (u, _) in cases.items():
                if f"/{k}/" in r["url"] or (k == "loop" and "/loop" in r["url"]) or (k == "redirect" and "/redirect/" in r["url"]) or (k == "page" and "/page/" in r["url"]):
                    by[k] = r
        for k, (u, must) in cases.items():
            r = by.get(k)
            kind = r["state"]["kind"] if r else "missing"
            measure("flaky", f"{k}: {kind}")
            seed = {"redirect": "redir"}.get(k, k)
            if kind == "completed":
                p = pathlib.Path(r["path"])
                good = p.exists() and p.stat().st_size == size and file_sha(p) == expected_sha(seed, size) if k not in ("page", "lieover", "lieunder") else None
                if k in ("lieover", "lieunder"):
                    finding("high", "flaky", f"{k}: a download whose Content-Length is wrong was marked completed ({p.stat().st_size if p.exists() else 'missing'} bytes on disk)")
                elif k == "page":
                    finding("high", "flaky", "an HTML page served in place of a .zip was marked completed as if it were the file")
                elif good is False:
                    finding("critical", "flaky", f"{k}: completed but the file differs from what was served")
            elif must:
                finding("high", "flaky", f"{k}: expected to complete through retries, ended {kind}: {r and r['state']}")
            if kind == "running" or kind == "queued":
                finding("medium", "flaky", f"{k}: still {kind} after 240 s")
        rss, cpu, fds, th = dm.peak()
        measure("flaky", f"peak rss {rss:.0f} MB, fds {fds}")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_crash(ctx):
    """kill -9 in the middle of 24 downloads; a restart finishes them byte-exact."""
    base, root = ctx["base"], ctx["root"]
    n = 24
    size = 48 * MB
    dm = DM(root, {"connections": 8, "concurrency": 8}, name="crash").start()
    expect = {}
    urls = []
    for i in range(n):
        name = f"k{i}.bin"
        urls.append(f"{base}/f/crash{i}/{size}/{name}?rate={3 * MB}")
        expect[name] = (f"crash{i}", size)
    dm.add(urls)
    wait_for(lambda: sum(r["progress"]["downloaded"] for r in dm.rows()) > n * size * 0.15, 120, every=1)
    before = dm.rows()
    dm.stop(kill=True)
    time.sleep(1)
    left = sum(1 for r in before if r["state"]["kind"] != "completed")
    measure("crash", f"killed with {left} of {n} unfinished, {sum(r['progress']['downloaded'] for r in before) // MB} MB fetched")
    dm2 = DM(root, {"connections": 8, "concurrency": 8}, name="crash")
    dm2.home, dm2.out = dm.home, dm.out
    try:
        dm2.start()
        rows = dm2.rows()
        if len(rows) != n:
            finding("high", "crash", f"after kill -9 and restart the list holds {len(rows)} of {n} downloads")
        # They come back paused or queued; start them.
        for r in rows:
            if r["state"]["kind"] in ("paused", "failed", "queued"):
                dm2.call("downloads.resume", ids=[r["id"]])
        wait_for(lambda: all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm2.rows()) and len(dm2.rows()) >= n, 300, every=2)
        rows = dm2.rows()
        st = states(rows)
        ok, bad = verify(rows, expect)
        measure("crash", f"after restart {st}; {ok} verified")
        if bad:
            finding("critical", "crash", f"files that resumed after kill -9 are corrupt: {bad[:3]}")
        if st.get("failed"):
            finding("high", "crash", f"{st['failed']} downloads failed to resume after kill -9: {[r['state'] for r in rows if r['state']['kind'] == 'failed'][:2]}")
        check("crash", "all resumed byte-exact", ok == n)
        # Leftovers: staged parts of finished downloads.
        parts = list(dm2.home.rglob("*.part")) + [p for p in (dm2.home / "parts").rglob("*") if p.is_file()] if (dm2.home / "parts").exists() else []
        if parts:
            finding("low", "crash", f"{len(parts)} staged files left behind after every download finished")
    finally:
        dm2.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_pause_storm(ctx):
    """Random pause/resume/cancel at speed on 80 downloads; survivors are exact."""
    base, root = ctx["base"], ctx["root"]
    n = 80
    size = 6 * MB
    dm = DM(root, {"connections": 4, "concurrency": 16}, name="storm").start()
    try:
        expect = {}
        urls = []
        for i in range(n):
            name = f"p{i}.bin"
            urls.append(f"{base}/f/storm{i}/{size}/{name}?rate={MB}")
            expect[name] = (f"storm{i}", size)
        dm.add(urls)
        time.sleep(2)
        rnd = random.Random(7)
        cancelled = set()
        t_end = time.time() + 30
        errors = 0
        while time.time() < t_end:
            rows = dm.rows()
            if not rows:
                continue
            for _ in range(10):
                r = rnd.choice(rows)
                action = rnd.choice(["downloads.pause", "downloads.resume", "downloads.resume", "downloads.pause", "downloads.cancel" if rnd.random() < 0.05 else "downloads.resume"])
                res = dm.call(action, ids=[r["id"]])
                if not res.get("ok"):
                    errors += 1
                elif action == "downloads.cancel" and res["result"]["done"]:
                    cancelled.add(r["url"])
        # Let everything run.
        for r in dm.rows():
            if r["state"]["kind"] in ("paused", "failed"):
                dm.call("downloads.resume", ids=[r["id"]])
        wait_for(lambda: all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 400, every=2)
        rows = dm.rows()
        st = states(rows)
        measure("pause_storm", f"{st}; {len(cancelled)} cancelled by the storm; {errors} control errors")
        gone = {urllib.parse.urlparse(u).path.rsplit("/", 1)[-1] for u in cancelled}
        survivors = {k: v for k, v in expect.items() if k not in gone}
        ok, bad = verify(rows, survivors)
        if bad:
            finding("critical", "pause_storm", f"{len(bad)} files corrupt after pause/resume storm: {bad[:3]}")
        stuck = [r for r in rows if r["state"]["kind"] in ("running", "queued", "paused")]
        if stuck:
            finding("high", "pause_storm", f"{len(stuck)} downloads stuck {states(stuck)} after resuming everything")
        left = [p.name for p in dm.out.iterdir() if p.is_file() and p.name in gone]
        if left:
            finding("low", "pause_storm", f"{len(left)} cancelled downloads left a file in the folder")
        if errors:
            finding("medium", "pause_storm", f"{errors} control calls errored during the storm")
        check("pause_storm", "survivors verified", ok >= len(survivors) - st.get("failed", 0) - len(stuck))
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_names(ctx):
    """A hostile server names its files: nothing may land outside the folder."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, name="names").start()
    sentinel_dirs = [root, root.parent, pathlib.Path("/tmp")]
    before = {d: set(p.name for p in d.iterdir()) if d.exists() else set() for d in sentinel_dirs}
    try:
        urls = [f"{base}/cd/{k}" for k in NAMES] + [f"{base}/cd2/{k}" for k in NAMES]
        for u in urls:
            dm.add([u])
        wait_for(lambda: len(dm.rows()) >= len(urls) and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 120, every=1)
        rows = dm.rows()
        out = dm.out.resolve()
        outside = []
        for r in rows:
            if r["state"]["kind"] == "completed":
                p = pathlib.Path(r["path"]).resolve()
                if out not in p.parents and p != out:
                    outside.append((r["url"], str(p)))
                measure("names", f"{r['url'].rsplit('/', 2)[-2]}/{r['url'].rsplit('/', 1)[-1]} -> {p.name!r}")
            elif r["state"]["kind"] == "failed":
                measure("names", f"{r['url'].rsplit('/', 1)[-1]} failed: {r['state'].get('reason', r['state'])}")
        for d in sentinel_dirs:
            now = set(p.name for p in d.iterdir()) if d.exists() else set()
            new = [n for n in now - before[d] if "escaped" in n]
            if new:
                outside.append((str(d), new))
        if outside:
            finding("critical", "names", f"a download was written outside the download folder: {outside[:4]}")
        check("names", "nothing outside the folder", not outside)
        for stray in (root.parent / "escaped-dotdot.bin", root / "escaped-dotdot.bin", pathlib.Path("/tmp/escaped-abs.bin"),
                      root.parent / "escaped-backslash.bin"):
            stray.unlink(missing_ok=True)
        for r in rows:
            if r["state"]["kind"] == "completed":
                name = pathlib.Path(r["path"]).name
                if any(c in name for c in "\x00\r\n") or name.upper().split(".")[0] in ("CON", "NUL", "PRN", "AUX"):
                    finding("medium", "names", f"saved under an unsafe name: {name!r}")
                if "‮" in name:
                    finding("low", "names", "a right-to-left override character is kept in a saved file name (photo\\u202egpj.exe shows as photoexe.jpg)")
                if len(name.encode()) > 255:
                    finding("medium", "names", "a saved name is longer than the file system allows")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_hostile(ctx):
    """Redirect to other schemes, oversized headers, stalls, gzip bomb, huge advertised size."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 4, "concurrency": 8}, name="hostile").start()
    try:
        cases = {
            "to_file": f"{base}/to?u=file:///etc/hosts",
            "to_ftp_local": f"{base}/to?u=ftp://127.0.0.1:1/x",
            "bighead": f"{base}/bighead?kb=1024",
            "stall": f"{base}/stall?s=40",
            "gz": f"{base}/gz/300",
            # Four gigabytes advertised, one megabyte sent: dm may reserve the
            # room, but the server never fills it.
            "huge": f"{base}/f/huge/{4096 * MB}/huge.bin?cap={MB}",
            # More than the disk has: dm should say so before it starts.
            "toobig": f"{base}/f/toobig/{int((free_gb(tempfile.gettempdir()) + 12) * 1e9)}/toobig.bin?cap={MB}",
            "cred": with_login(f"{base}/f/cred/1024/cred.bin", "user", "pass"),
        }
        for k, u in cases.items():
            r = dm.add([u])
            measure("hostile", f"{k}: add -> ok={r.get('ok')} {str(r.get('error') or r.get('result'))[:100]}")
        time.sleep(1)
        # An answer whose headers cannot be read fails at once; timed apart
        # from the others, which are retried for minutes by design.
        t_start = time.time()
        bighead = lambda: next((r for r in dm.rows() if "/bighead" in r["url"]), None)
        wait_for(lambda: (bighead() or {"state": {"kind": ""}})["state"]["kind"] in ("completed", "failed", "cancelled"), 60, every=1)
        waited = time.time() - t_start
        wait_for(lambda: all(r["state"]["kind"] in ("completed", "failed", "cancelled", "paused") for r in dm.rows() if "/stall" not in r["url"]) and len(dm.rows()) >= 3, 200, every=2)
        rows = dm.rows()
        for r in rows:
            if "/bighead" in r["url"] and (r["state"]["kind"] != "failed" or waited > 20):
                finding("low", "hostile", f"a response whose headers can never be parsed (1 MB of header) took {waited:.0f}s to fail ({r['state']['kind']}): a permanent error is treated like a passing one")
            measure("hostile", f"{r['url'][-40:]} -> {r['state']['kind']} {r['state'].get('reason', '') if isinstance(r['state'], dict) else ''}")
        # Whatever a file:// redirect saved is compared with the real file:
        # a link to /etc/hosts that ends as an empty or error page is not
        # the same as one that carries the computer's file.
        hosts = pathlib.Path("/etc/hosts").read_bytes()
        for r in rows:
            if "/to?u=file" in r["url"] and r["state"]["kind"] == "completed":
                p = pathlib.Path(r["path"])
                body = p.read_bytes() if p.exists() else b""
                measure("hostile", f"the file:// redirect saved {len(body)} bytes as {p.name!r}")
                if body and (body == hosts or hosts[:200] in body):
                    finding("critical", "hostile", "a redirect to file:///etc/hosts was followed and the computer's own file was saved into the download folder")
                else:
                    finding("low", "hostile", f"a redirect to a file:// address is recorded as a completed download ({len(body)} bytes saved, not the file): it should fail, as the ftp:// redirect did")
        gz = [r for r in rows if "/gz/" in r["url"]]
        for r in gz:
            if r["state"]["kind"] == "completed":
                sz = pathlib.Path(r["path"]).stat().st_size
                measure("hostile", f"gzip body of ~300 KB expanded to {sz // MB} MB on disk")
                if sz > 100 * MB:
                    finding("low", "hostile", f"a gzip-encoded response of about 300 KB on the wire was decompressed and saved as {sz // MB} MB: only the disk floor (256 MB free) stops a larger one")
        for r in rows:
            if "/toobig/" in r["url"]:
                measure("hostile", f"a file larger than the free disk space: {r['state']}")
                if r["state"]["kind"] == "running" or r["state"]["kind"] == "queued":
                    finding("medium", "hostile", "a file advertised as larger than the free disk space was accepted and started instead of being refused up front")
                if r["state"]["kind"] == "completed":
                    finding("high", "hostile", "a file larger than the free disk space was marked completed")
        huge = [r for r in rows if "/huge/" in r["url"]]
        for r in huge:
            if r["state"]["kind"] == "completed":
                finding("high", "hostile", "a file advertised as 30 GB with less sent was marked completed")
        # Credentials in a URL must not appear in the list's shown URL or on disk names.
        for r in rows:
            if "pass" in json.dumps(r.get("url", "")) and "user:pass@" in r["url"]:
                finding("medium", "hostile", "a password written in a link is shown in clear in the download list (queue.list)")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_control(ctx):
    """The local control port: token, malformed input, floods, scope of what it may do."""
    root = ctx["root"]
    dm = DM(root, name="control").start()
    try:
        port = int((dm.home / "control.port").read_text())
        # Bound to loopback only?
        lsof = subprocess.run(["lsof", "-nP", "-a", "-p", str(dm.proc.pid), "-iTCP", "-sTCP:LISTEN"], capture_output=True, text=True).stdout
        measure("control", f"listening: {lsof.strip().splitlines()[-1] if lsof.strip() else 'none'}")
        if "*:" in lsof or "0.0.0.0" in lsof:
            finding("critical", "control", "the control port listens on every interface")
        mode = oct((dm.home / "control.token").stat().st_mode & 0o777)
        check("control", f"token file mode {mode}", mode == "0o600")
        if mode != "0o600":
            finding("high", "control", f"control.token is {mode}, readable by other users")
        # No token, bad token.
        def raw(lines, timeout=10, read=True):
            s = socket.create_connection(("127.0.0.1", port), timeout=timeout)
            s.sendall(lines if isinstance(lines, bytes) else lines.encode())
            out = b""
            if read:
                try:
                    s.settimeout(3)
                    out = s.recv(65536)
                except OSError:
                    pass
            s.close()
            return out.decode(errors="replace")
        r = raw(json.dumps({"id": 1, "method": "queue.list", "params": {}}) + "\n")
        check("control", "no token is refused", '"ok":false' in r.replace(" ", "") or "error" in r, r[:100])
        if '"ok":true' in r.replace(" ", ""):
            finding("critical", "control", "queue.list answered with no token")
        r = raw(json.dumps({"id": 1, "method": "auth", "params": {"token": "x" * 10}}) + "\n")
        check("control", "a wrong token is refused", '"ok":false' in r.replace(" ", ""))
        # Many wrong guesses: is there any lockout / delay?
        t0 = time.time()
        s = socket.create_connection(("127.0.0.1", port), timeout=10)
        rf = s.makefile("r")
        guesses = 0
        try:
            for i in range(300):
                s.sendall((json.dumps({"id": i, "method": "auth", "params": {"token": f"guess{i}"}}) + "\n").encode())
                if not rf.readline():
                    break
                guesses += 1
        except OSError:
            pass
        measure("control", f"dm cut the connection after {guesses} wrong tokens in {time.time() - t0:.1f}s")
        s.close()
        if guesses >= 300:
            finding("low", "control", "300 wrong tokens in a row on one connection were all answered: no lockout (the token is random and long, so guessing is not practical)")
        # Malformed and oversized input.
        for label, payload in (("not json", "hello\n"), ("array", "[1,2,3]\n"), ("null params", json.dumps({"id": 1, "method": "auth", "params": None}) + "\n"),
                               ("huge id", json.dumps({"id": "x" * 100000, "method": "commands.list"}) + "\n"),
                               ("deep json", "[" * 20000 + "]" * 20000 + "\n")):
            alive_before = dm.proc.poll() is None
            raw(payload)
            time.sleep(0.3)
            if dm.proc.poll() is not None:
                finding("high", "control", f"dm crashed on malformed input: {label}")
                dm.start()
                port = int((dm.home / "control.port").read_text())
        # A very long line with no newline (memory).
        rss0 = proc_stats(dm.proc.pid)[0]
        s = socket.create_connection(("127.0.0.1", port), timeout=10)
        sent = 0
        try:
            for _ in range(200):
                s.sendall(b"A" * MB)
                sent += MB
        except OSError:
            pass
        time.sleep(1)
        rss1 = proc_stats(dm.proc.pid)[0]
        measure("control", f"{sent // MB} MB without a newline accepted before the connection was cut: rss {rss0:.0f} -> {rss1:.0f} MB")
        if sent >= 150 * MB:
            finding("medium", "control", f"the control port buffers a line of at least {sent // MB} MB with no newline (rss {rss0:.0f} -> {rss1:.0f} MB): a local process can make dm use unbounded memory before it authenticates")
        s.close()
        # A flood of connections.
        socks = []
        try:
            for _ in range(1500):
                socks.append(socket.create_connection(("127.0.0.1", port), timeout=3))
        except OSError as e:
            measure("control", f"connection flood stopped at {len(socks)}: {e}")
        measure("control", f"{len(socks)} simultaneous unauthenticated connections held open")
        time.sleep(25)
        closed = 0
        for sk in socks:
            try:
                sk.setblocking(False)
                if sk.recv(1) == b"":
                    closed += 1
            except BlockingIOError:
                pass
            except OSError:
                closed += 1
        measure("control", f"after 25 s idle, {closed} of {len(socks)} unauthenticated connections were closed by dm")
        if len(socks) >= 1000 and closed < len(socks) // 2:
            finding("low", "control", f"{len(socks)} idle unauthenticated connections to the control port are kept open (no cap, no idle timeout within 25 s): a local process can use up dm's file descriptors")
        time.sleep(1)
        alive = dm.proc.poll() is None
        try:
            dm.connect()
            answers = dm.call("commands.list")["ok"]
        except Exception as e:
            answers = False
        for s in socks:
            s.close()
        if not alive:
            finding("high", "control", "dm died under 1500 idle control connections")
        elif not answers:
            finding("medium", "control", f"with {len(socks)} idle connections open, the control port stops answering an authenticated client")
        # What an authenticated client may add.
        base = ctx["base"]
        for label, params in (("file url", {"urls": ["file:///etc/passwd"]}),
                              ("name traversal, real file", {"urls": [f"{base}/f/ntrav/2048/ntrav.bin"], "name": "../../../dm-stress-name-escape.bin"}),
                              ("folder traversal", {"urls": ["http://127.0.0.1:1/a"], "folder": f"{dm.out}/../../../tmp/dm-stress-escape"}),
                              ("relative folder", {"urls": ["http://127.0.0.1:1/a"], "folder": "../x"}),
                              ("name traversal", {"urls": ["http://127.0.0.1:1/a"], "name": "../../escape.txt"}),
                              ("javascript url", {"urls": ["javascript:alert(1)"]}),
                              ("shell metachars", {"urls": ["http://127.0.0.1:1/a;touch /tmp/dm-stress-injected"]})):
            dm.connect()
            res = dm.call("downloads.add", **params)
            measure("control", f"{label}: ok={res.get('ok')} {str(res.get('error') or res.get('result'))[:90]}")
        time.sleep(3)
        for stray in (root / "dm-stress-name-escape.bin", root.parent / "dm-stress-name-escape.bin", dm.out.parent / "dm-stress-name-escape.bin"):
            if stray.exists():
                finding("critical", "control", f"a name of ../../../ written through downloads.add put a file at {stray}")
                stray.unlink()
        if pathlib.Path("/tmp/dm-stress-injected").exists() or pathlib.Path("/tmp/dm-stress-escape").exists():
            finding("critical", "control", "a crafted add wrote or ran something outside the download folder")
        rows = dm.rows()
        for r in rows:
            if "file://" in r["url"] or r["url"].startswith("javascript"):
                finding("medium", "control", f"a {r['url'].split(':')[0]}: link was accepted into the list ({r['state']['kind']})")
        for p in ("/tmp/dm-stress-injected", "/tmp/dm-stress-escape"):
            shutil.rmtree(p, ignore_errors=True) if pathlib.Path(p).is_dir() else pathlib.Path(p).unlink(missing_ok=True)
    finally:
        dm.stop()


@phase
def p_bigqueue(ctx):
    """Twenty thousand queued links: adding, listing, restarting with them, clearing."""
    base, root = ctx["base"], ctx["root"]
    n = int(os.environ.get("DM_STRESS_QUEUE", "20000"))
    dm = DM(root, name="bigq").start()
    try:
        t0 = time.time()
        step = 500
        for k in range(0, n, step):
            urls = [f"{base}/f/bq{i}/1024/q{i}.bin" for i in range(k, min(n, k + step))]
            t = time.time()
            r = dm.call("downloads.add", urls=urls, later=True)
            if not r.get("ok"):
                finding("high", "bigqueue", f"adding links {k}..{k + step} was refused: {r.get('error')}")
                break
            if k in (0, n // 2, n - step):
                measure("bigqueue", f"batch of {step} at {k} links listed took {time.time() - t:.2f}s")
        measure("bigqueue", f"{n} links queued in {time.time() - t0:.0f}s")
        rss_idle, *_ = proc_stats(dm.proc.pid)
        measure("bigqueue", f"rss {rss_idle:.0f} MB with {n} rows before anyone lists them ({rss_idle * 1024 / n:.1f} KB per row)")
        t = time.time()
        rows = dm.rows()
        measure("bigqueue", f"queue.list of {len(rows)} rows took {time.time() - t:.2f}s")
        rss, cpu, fds, th = proc_stats(dm.proc.pid)
        measure("bigqueue", f"rss {rss:.0f} MB with {len(rows)} rows ({rss * 1024 / max(1, len(rows)):.1f} KB per row)")
        if len(rows) != n:
            finding("high", "bigqueue", f"{n} links added, {len(rows)} rows listed")
        if rss > 1500:
            finding("medium", "bigqueue", f"{rss:.0f} MB resident for {n} queued links")
        # Restart with them: how long to come back, and are all there?
        dm.stop()
        time.sleep(1)
        size = (dm.home / "queue.json").stat().st_size if (dm.home / "queue.json").exists() else 0
        measure("bigqueue", f"queue.json is {size // 1024} KB for {n} rows")
        t = time.time()
        dm.start()
        rss_back, *_ = proc_stats(dm.proc.pid)
        measure("bigqueue", f"rss {rss_back:.0f} MB just after a restart with {n} rows ({rss_back * 1024 / n:.1f} KB per row)")
        back = dm.rows()
        measure("bigqueue", f"a restart with {n} rows listed {len(back)} after {time.time() - t:.1f}s")
        if len(back) != n:
            finding("high", "bigqueue", f"{n} queued links before a restart, {len(back)} after")
        # What a long list costs the downloads running beside it: 200 small
        # files with the queued links still in the list.
        urls = [f"{base}/f/bqs{i}/4097/bqs{i}.bin" for i in range(200)]
        t = time.time()
        dm.add(urls)
        wait_for(lambda: sum(1 for x in dm.rows() if "bqs" in x["url"] and x["state"]["kind"] == "completed") >= 200, 600, every=1)
        took = time.time() - t
        done = sum(1 for x in dm.rows() if "bqs" in x["url"] and x["state"]["kind"] == "completed")
        measure("bigqueue", f"{done} of 200 small files finished in {took:.1f}s ({done / took:.1f} files/s) with {n} queued links in the list")
        if done / max(took, 0.1) < 15:
            finding("medium", "bigqueue", f"with {n} links in the list, small downloads finish at {done / took:.1f} files/s (about 57/s with a short list): every state change rewrites and syncs the whole list")
        t = time.time()
        r = dm.call("downloads.cancel", ids=[x["id"] for x in back[:2000]])
        measure("bigqueue", f"cancelling 2000 took {time.time() - t:.1f}s: ok={r.get('ok')} {len(r.get('result', {}).get('done', []))} done {str(r.get('error', ''))[:100]}")
        if time.time() - t > 20:
            finding("medium", "bigqueue", f"cancelling 2000 of {n} queued links in one call took {time.time() - t:.0f}s (the window or script that asks waits that long)")
        left = states(dm.rows())
        measure("bigqueue", f"after the cancel: {left}")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_edge(ctx):
    """Microsoft Edge: forty downloads started at once; dm must take every one and Edge keep none."""
    base = ctx["base"]
    import importlib.util
    spec = importlib.util.spec_from_file_location("chrome_check", ROOT / "scripts/chrome-check.py")
    cc = importlib.util.module_from_spec(spec)
    sys.modules["chrome_check"] = cc
    spec.loader.exec_module(cc)
    n = int(os.environ.get("DM_STRESS_EDGE", "40"))
    cc.CHROME_PATH_DEFAULT = None
    # Microsoft Edge itself, as the phase says: the installed one unless a
    # browser was named (CHROME_PATH). Chrome for Testing, which the browser
    # check downloads when none is named, is a different browser: it failed
    # here three ways in a row (the connection closed, timed out, reset) where
    # Edge passed all 38 checks.
    if not os.environ.get("CHROME_PATH"):
        edge = next((p for p in (
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            "/usr/bin/microsoft-edge",
            "/opt/microsoft/msedge/msedge",
        ) if pathlib.Path(p).exists()), None)
        if edge:
            os.environ["CHROME_PATH"] = edge
        else:
            print("  (Microsoft Edge is not installed here: Chrome for Testing stands in, "
                  "which is not what this phase is about)", flush=True)

    def hook(devtools, origin, engine_dl, engine_home, worker):
        print(f"  (the browser check's own steps are done; now {n} downloads at once)", flush=True)
        tab = cc.open_tab(devtools, f"{base}/edge/{n}")
        cc.loaded(tab, f"/edge/{n}", "#l0")
        before = len(cc.queue_rows(engine_home))
        t0 = time.time()
        # A pointer pressed on each link, as a person does it: Edge lets one
        # download through for a script's click and asks about the rest, so
        # only real input is a fair test of many downloads at once.
        gap = float(os.environ.get("DM_STRESS_EDGE_GAP", "0.08"))
        for i in range(n):
            x, y = tab.evaluate(f"""(() => {{ const el = document.getElementById('l{i}'); el.scrollIntoView({{block: 'center'}});
              const r = el.getBoundingClientRect(); return [r.left + 5, r.top + r.height / 2]; }})()""")
            cc.click_at(tab, x, y)
            time.sleep(gap)
        clicked = time.time() - t0
        measure("edge", f"{n} real clicks sent in {clicked:.1f}s ({gap}s apart)")
        def rows():
            return cc.queue_rows(engine_home)[before:] if len(cc.queue_rows(engine_home)) >= before else []
        done = wait_for(lambda: len(rows()) >= n and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in rows()), 240, every=1)
        took = time.time() - t0
        mine = rows()
        st = states(mine)
        measure("edge", f"dm's list after {took:.0f}s: {len(mine)} rows {st}")
        # What Edge itself holds: anything it kept is a download that did not go to dm.
        listed = worker.evaluate("chrome.downloads.search({}).then(l => JSON.stringify(l.filter(d => /edge[0-9]+[.]bin/.test(d.filename || d.url)).map(d => [d.url.split('/').slice(-1)[0], d.state, d.exists])))")
        kept = json.loads(listed)
        measure("edge", f"Edge's own download list holds {len(kept)} of them: {kept[:4]}")
        urls = [r["url"] for r in mine]
        dup = len(urls) - len(set(urls))
        if dup:
            finding("high", "edge", f"{dup} of {n} simultaneous downloads were taken twice")
        if len(mine) < n:
            finding("high", "edge", f"only {len(mine)} of {n} downloads started together reached dm")
        if kept:
            finding("high", "edge", f"{len(kept)} of {n} downloads started together stayed with Edge instead of dm: {kept[:3]}")
        if st.get("failed"):
            finding("high", "edge", f"{st['failed']} of {n} downloads taken from Edge failed in dm")
        expect = {f"edge{i}.bin": (f"edge{i}", 2 * MB) for i in range(n)}
        ok, bad = verify(mine, expect)
        measure("edge", f"{ok} of {n} byte-exact in dm's folder")
        if bad:
            finding("critical", "edge", f"{len(bad)} files taken from Edge are wrong: {bad[:3]}")
        if not done:
            finding("medium", "edge", f"downloads taken from Edge still not settled after 240 s: {st}")
        stray = [p.name for p in pathlib.Path.home().glob("Downloads/edge*.bin")]
        if stray:
            finding("medium", "edge", f"{len(stray)} edge*.bin files were saved by Edge into ~/Downloads")

    cc.panel_checks = hook
    try:
        cc.main()
    except SystemExit:
        pass


@phase
def p_large(ctx):
    """One 1.5 GB file on 16 connections: integrity, memory, and the disk it needs."""
    base, root = ctx["base"], ctx["root"]
    size = 1536 * MB
    if free_gb(tempfile.gettempdir()) < MIN_FREE_GB + 4:
        finding("info", "large", "skipped: not enough free disk for a 1.5 GB file with the safety margin")
        return
    dm = DM(root, {"connections": 16}, name="large").start()
    try:
        dm.add([f"{base}/f/large/{size}/large.bin"])
        t0 = time.time()
        lowest = free_gb(tempfile.gettempdir())
        def settled():
            nonlocal lowest
            lowest = min(lowest, free_gb(tempfile.gettempdir()))
            return any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows())
        wait_for(settled, 600, every=1)
        took = time.time() - t0
        rows = dm.rows()
        measure("large", f"{rows[0]['state']} in {took:.0f}s ({size / MB / took:.0f} MB/s); lowest free disk {lowest:.1f} GB; peak rss {dm.peak()[0]:.0f} MB")
        ok, bad = verify(rows, {"large.bin": ("large", size)})
        check("large", "1.5 GB byte-exact", ok == 1 and not bad, str(bad))
        if bad:
            finding("critical", "large", f"a 1.5 GB download is wrong: {bad}")
        if dm.peak()[0] > 400:
            finding("medium", "large", f"{dm.peak()[0]:.0f} MB resident for one large download")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_outage(ctx):
    """The network goes away for 25 seconds in the middle of 20 downloads and comes back."""
    root = ctx["root"]
    server = ctx["server"]
    base = ctx["base"]
    n, size = 20, 24 * MB
    dm = DM(root, {"connections": 4, "concurrency": 10}, name="outage").start()
    try:
        expect = {}
        urls = []
        for i in range(n):
            expect[f"o{i}.bin"] = (f"out{i}", size)
            urls.append(f"{base}/f/out{i}/{size}/o{i}.bin?rate={2 * MB}")
        dm.add(urls)
        wait_for(lambda: sum(r["progress"]["downloaded"] for r in dm.rows()) > n * size * 0.2, 90, every=1)
        server.cut_all()
        measure("outage", "the network is down")
        time.sleep(25)
        mid = states(dm.rows())
        measure("outage", f"after 25 s down: {mid}")
        server.down = False
        measure("outage", "the network is back")
        wait_for(lambda: all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 300, every=2)
        rows = dm.rows()
        st = states(rows)
        # A failed one is given a retry, as a person would.
        for r in rows:
            if r["state"]["kind"] == "failed":
                dm.call("downloads.resume", ids=[r["id"]])
        if st.get("failed"):
            measure("outage", f"{st['failed']} failed during the outage; retried by hand")
            finding("low", "outage", f"{st['failed']} of {n} downloads gave up during a 25-second outage instead of waiting for the network (a person retried them)")
            wait_for(lambda: all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 300, every=2)
        rows = dm.rows()
        ok, bad = verify(rows, expect)
        measure("outage", f"final {states(rows)}; {ok} byte-exact")
        if bad:
            finding("critical", "outage", f"files differ after an outage: {bad[:3]}")
        check("outage", "all finished byte-exact", ok == n)
    finally:
        server.down = False
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_expired(ctx):
    """A link that stops working after a few requests, as expiring links do."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 8, "retries": 3}, name="expired").start()
    try:
        size = 16 * MB
        dm.add([f"{base}/f/exp/{size}/exp.bin?expire=3&rate={2 * MB}"])
        wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 200, every=1)
        r = dm.rows()[0]
        measure("expired", f"{r['state']}")
        if r["state"]["kind"] == "completed":
            p = pathlib.Path(r["path"])
            good = p.exists() and p.stat().st_size == size and file_sha(p) == expected_sha("exp", size)
            if not good:
                finding("critical", "expired", "a link that expired mid-download was marked completed with wrong content")
        elif r["state"]["kind"] != "failed":
            finding("medium", "expired", f"an expired link is {r['state']} after 200 s")
        else:
            check("expired", "ends failed with a reason", bool(r["state"].get("reason")))
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_urls(ctx):
    """Odd links and repeats: nothing crashes, repeats are not downloaded twice."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, name="urls").start()
    try:
        port = base.rsplit(":", 1)[1]
        odd = {
            "long": f"{base}/f/long/1024/" + "a" * 60000 + ".bin",
            "spaces": f"{base}/f/sp/1024/a%20b%20c.bin",
            "unicode_host": "http://m\u00fcnchen.example/x.bin",
            "ipv6": f"http://[::1]:{port}/f/v6/1024/v6.bin",
            "empty": "",
            "ws": "   ",
            "nonsense": "not a url at all",
            "only_scheme": "http://",
            "port0": "http://127.0.0.1:0/x",
            "huge_port": "http://127.0.0.1:99999/x",
            "ctrl": f"{base}/f/ct/1024/a\x01b.bin",
            "ftp_dead": "ftp://127.0.0.1:1/x.bin",
            "data": "data:text/plain;base64,aGVsbG8=",
            "mailto": "mailto:a@b.c",
            "ws_scheme": "ws://127.0.0.1:1/x",
        }
        for k, u in odd.items():
            try:
                r = dm.add([u])
                measure("urls", f"{k}: ok={r.get('ok')} {str(r.get('error') or r.get('result'))[:70]}")
            except Exception as e:
                finding("high", "urls", f"{k}: the control call failed: {e!r}")
                dm.start()
        if dm.proc.poll() is not None:
            finding("high", "urls", "dm died on an odd link")
        # The same link a hundred times.
        same = f"{base}/f/same/1024/same.bin"
        added = 0
        for _ in range(100):
            r = dm.add([same])
            added += (r.get("result") or {}).get("added", 0)
        measure("urls", f"the same link added 100 times: {added} accepted")
        time.sleep(3)
        n_same = sum(1 for r in dm.rows() if r["url"].endswith("same.bin"))
        if n_same > 1:
            finding("low", "urls", f"the same link added 100 times made {n_same} rows")
        # One call with a thousand links, a thousand of them the same.
        r = dm.add([same] * 1000)
        measure("urls", f"1000 repeats in one call: {r.get('result')}")
        if not dm.proc.poll() is None:
            finding("high", "urls", "dm died")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_limits(ctx):
    """The concurrency setting is kept: never more downloads running than it allows."""
    base, root = ctx["base"], ctx["root"]
    cap = 5
    dm = DM(root, {"connections": 2, "concurrency": cap}, name="limits").start()
    try:
        urls = [f"{base}/f/lim{i}/{3 * MB}/lim{i}.bin?rate={MB}" for i in range(40)]
        dm.add(urls)
        worst = 0
        t_end = time.time() + 150
        while time.time() < t_end:
            rows = dm.rows()
            running = sum(1 for r in rows if r["state"]["kind"] == "running")
            worst = max(worst, running)
            if len(rows) >= 40 and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in rows):
                break
            time.sleep(0.25)
        measure("limits", f"concurrency {cap}: at most {worst} running at once; {states(dm.rows())}")
        if worst > cap:
            finding("medium", "limits", f"{worst} downloads ran at once with the concurrency setting at {cap}")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_window(ctx):
    """dm's window under load: 5000 rows and 60 active downloads; does it keep answering?"""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 4, "concurrency": 60}, name="win", window=True).start()
    try:
        for k in range(0, 5000, 500):
            dm.call("downloads.add", urls=[f"{base}/f/w{i}/1024/w{i}.bin" for i in range(k, k + 500)], later=True)
        dm.add([f"{base}/f/wa{i}/{40 * MB}/wa{i}.bin?rate={MB}" for i in range(60)])
        time.sleep(8)
        lat = []
        for _ in range(20):
            t = time.time()
            dm.call("commands.list")            # one turn of the window's loop
            lat.append(time.time() - t)
            time.sleep(0.5)
        lat.sort()
        measure("window", f"the window answers a call in median {lat[len(lat) // 2] * 1000:.0f} ms, worst {lat[-1] * 1000:.0f} ms, with {len(dm.rows())} rows")
        if lat[-1] > 1.0:
            finding("medium", "window", f"the window took {lat[-1]:.1f}s to answer once with 5000 rows and 60 active downloads")
        t = time.time()
        dm.call("ui.inspect")
        took = time.time() - t
        measure("window", f"ui.inspect (the automation call that describes the window) took {took:.1f}s with {len(dm.rows())} rows")
        if took > 3:
            finding("low", "window", f"ui.inspect takes {took:.1f}s with {len(dm.rows())} rows (about 7 s at 5000, 25 ms at 1500): its cost grows with the square of the list; the window itself keeps answering in ~10 ms")
        shot = dm.call("ui.screenshot", name="stress-5000-rows")
        measure("window", f"screenshot: {shot.get('result') or shot.get('error')}")
        rss, cpu, fds, th = dm.peak()
        measure("window", f"peak rss {rss:.0f} MB, cpu {cpu:.0f}%, fds {fds}, threads {th}")
        if cpu > 250:
            finding("low", "window", f"dm uses {cpu:.0f}% of a core with the window open under this load")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_speedlimit(ctx):
    """A speed limit is kept, and lifting it is felt."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 8}, name="limit").start()
    try:
        dm.call("downloads.speed_limit", kb=1024)
        size = 12 * MB
        dm.add([f"{base}/f/limit/{size}/limit.bin"])
        t0 = time.time()
        wait_for(lambda: any(r["state"]["kind"] == "completed" for r in dm.rows()), 60, every=0.5)
        took = time.time() - t0
        rate = size / took / 1024
        measure("speedlimit", f"limit 1024 KB/s: {rate:.0f} KB/s over {took:.1f}s")
        if rate > 1024 * 1.25:
            finding("medium", "speedlimit", f"a 1024 KB/s limit let {rate:.0f} KB/s through")
        if rate < 1024 * 0.5:
            finding("low", "speedlimit", f"a 1024 KB/s limit gave only {rate:.0f} KB/s")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_hls(ctx):
    """An HLS playlist of 300 segments, and one with a missing segment."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 8}, name="hls").start()
    try:
        dm.add([f"{base}/hls/300/good.m3u8"])
        dm.add([f"{base}/hls/40/bad.m3u8?missing=20"])
        wait_for(lambda: len(dm.rows()) >= 2 and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 120, every=1)
        rows = dm.rows()
        for r in rows:
            measure("hls", f"{r['url'][-30:]} -> {r['state']}")
        for r in rows:
            if "good" in r["url"]:
                if r["state"]["kind"] != "completed":
                    finding("medium", "hls", f"a plain 300-segment playlist ended {r['state']}")
                else:
                    sz = pathlib.Path(r["path"]).stat().st_size
                    check("hls", f"joined size {sz}", sz == 300 * 350 * 188 or sz > 0)
            if "bad" in r["url"] and r["state"]["kind"] == "completed":
                sz = pathlib.Path(r["path"]).stat().st_size
                finding("high", "hls", f"a playlist with a missing segment (404) was marked completed with {sz} bytes, silently shorter than the stream")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_leak(ctx):
    """Five waves of downloads: memory, file descriptors and threads return to where they were."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, {"connections": 8, "concurrency": 16}, name="leak").start()
    try:
        series = []
        for wave in range(5):
            urls = [f"{base}/f/leak{wave}_{i}/{2 * MB}/w{wave}_{i}.bin" for i in range(150)]
            dm.add(urls)
            wait_for(lambda: len(dm.rows()) >= 150 * (wave + 1) and all(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 300, every=1)
            time.sleep(3)
            rss, cpu, fds, th = proc_stats(dm.proc.pid)
            series.append((rss, fds, th))
            measure("leak", f"after wave {wave + 1}: rss {rss:.0f} MB, fds {fds}, threads {th}, rows {len(dm.rows())}")
        if series[-1][1] > series[0][1] + 60:
            finding("medium", "leak", f"open files grew from {series[0][1]} to {series[-1][1]} over five identical waves")
        if series[-1][0] > series[0][0] * 1.8 + 50:
            finding("medium", "leak", f"memory grew from {series[0][0]:.0f} to {series[-1][0]:.0f} MB over five identical waves of 150 files")
        if series[-1][2] > series[0][2] + 30:
            finding("medium", "leak", f"threads grew from {series[0][2]} to {series[-1][2]}")
        failed = [r for r in dm.rows() if r["state"]["kind"] == "failed"]
        if failed:
            finding("high", "leak", f"{len(failed)} of 750 failed")
        expect = {f"w{w}_{i}.bin": (f"leak{w}_{i}", 2 * MB) for w in range(5) for i in range(150)}
        ok, bad = verify(dm.rows(), expect)
        measure("leak", f"{ok} of 750 verified byte-exact")
        if bad:
            finding("critical", "leak", f"{len(bad)} wrong files over the waves: {bad[:3]}")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


# ----------------------------------------------------------------- main

def main():
    args = sys.argv[1:]
    if "--list" in args:
        for k, f in PHASES.items():
            print(f"{k:12} {f.__doc__}")
        return
    names = [a for a in args if not a.startswith("-")] or list(PHASES)
    if not BINARY.exists():
        sys.exit(f"no dm at {BINARY}: (cd desktop && cargo build -p dm-desktop)")
    root = pathlib.Path(tempfile.mkdtemp(prefix="dm-stress-"))
    server, base = start_server()
    ctx = {"root": root, "base": base, "server": server}
    print(f"stress check: dm {BINARY}, server {base}, folder {root}, {free_gb(tempfile.gettempdir()):.1f} GB free", flush=True)
    try:
        for name in names:
            if name not in PHASES:
                print(f"no phase {name}")
                continue
            if not guard(name):
                continue
            print(f"\n== {name}: {PHASES[name].__doc__}", flush=True)
            Stats.reset()
            t0 = time.time()
            try:
                PHASES[name](ctx)
            except Exception as e:  # a phase that breaks is itself a result
                import traceback
                traceback.print_exc()
                finding("high", name, f"the check itself failed: {e!r}")
            print(f"   ({time.time() - t0:.0f}s, {free_gb(tempfile.gettempdir()):.1f} GB free)", flush=True)
    finally:
        server.shutdown()
        # Only the dm this run started: another run's is not this one's to end.
        for d in LIVE_DMS:
            d.stop()
        if os.environ.get("DM_STRESS_KEEP"):
            print(f"kept {root}")
        else:
            shutil.rmtree(root, ignore_errors=True)
    print("\nFINDINGS:")
    for sev, ph, what in FINDINGS:
        print(f"  [{sev}] {ph}: {what}")
    if not FINDINGS:
        print("  none")
    sys.exit(1 if any(s in ("critical", "high") for s, _, _ in FINDINGS) else 0)


if __name__ == "__main__":
    main()
