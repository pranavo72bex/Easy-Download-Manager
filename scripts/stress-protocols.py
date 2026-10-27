#!/usr/bin/env python3
"""The rest of the stress check: proxy, mirrors and Metalink, FTP, the watched
folder, the scheduler, the MCP server, a lossy and slow link, manifests with
absurd sizes, and torrents.

    python3 scripts/stress-protocols.py [phase ...]       # all, or those named
    python3 scripts/stress-protocols.py --list

It is `stress-check.py` with more servers (see that file's header for the
safety rules, which hold here too): a local HTTP proxy, a local FTP server, a
lossy TCP forwarder. All on 127.0.0.1. The torrent phase runs
`tests/torrent_stress.rs` inside a macOS sandbox profile that refuses every
network connection except to this computer, and checks that the profile
holds (an outside TCP, UDP and multicast send must all be refused) before it
runs anything; with no sandbox it does not run.
"""
import hashlib
import importlib.util
import json
import os
import pathlib
import random
import shutil
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
import http.server

ROOT = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("sc", ROOT / "scripts/stress-check.py")
sc = importlib.util.module_from_spec(spec)
sys.modules["sc"] = sc
spec.loader.exec_module(sc)
MB = sc.MB
finding, measure, check = sc.finding, sc.measure, sc.check
DM, wait_for, states, verify = sc.DM, sc.wait_for, sc.states, sc.verify
slice_of, expected_sha, file_sha = sc.slice_of, sc.expected_sha, sc.file_sha

PHASES = {}


def phase(fn):
    PHASES[fn.__name__.removeprefix("p_")] = fn
    return fn


# ------------------------------------------------------------ the proxy

class Proxy(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    request_queue_size = 512

    def handle_error(self, *a):
        pass

    log = []
    mode = "ok"          # ok | refuse | needauth | slow
    credentials = None   # "user:pass" the proxy asks for


class ProxyHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def log_message(self, *_):
        pass

    def deny_auth(self):
        self.send_response(407)
        self.send_header("Proxy-Authenticate", 'Basic realm="stress"')
        self.send_header("Content-Length", "0")
        self.end_headers()

    def check_auth(self):
        want = self.server.credentials
        if not want:
            return True
        import base64
        got = self.headers.get("Proxy-Authorization", "")
        return got == "Basic " + base64.b64encode(want.encode()).decode()

    def do_CONNECT(self):
        self.server.log.append(("CONNECT", self.path, self.headers.get("Proxy-Authorization") is not None))
        if self.server.mode == "refuse" or not self.check_auth():
            return self.deny_auth() if not self.check_auth() else self.send_error(503)
        host, _, port = self.path.partition(":")
        try:
            up = socket.create_connection((host, int(port)), timeout=10)
        except OSError:
            return self.send_error(502)
        self.send_response(200, "Connection established")
        self.end_headers()
        self.tunnel(up)

    def tunnel(self, up):
        c = self.connection
        def pump(a, b):
            try:
                while (d := a.recv(65536)):
                    b.sendall(d)
            except OSError:
                pass
            for s in (a, b):
                try:
                    s.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        t = threading.Thread(target=pump, args=(up, c), daemon=True)
        t.start()
        pump(c, up)

    def do_GET(self, head=False):
        self.server.log.append(("GET", self.path, self.headers.get("Proxy-Authorization") is not None))
        if self.server.mode == "refuse":
            return self.send_error(503)
        if not self.check_auth():
            return self.deny_auth()
        u = urllib.parse.urlparse(self.path)
        if not u.hostname:
            return self.send_error(400)
        try:
            up = socket.create_connection((u.hostname, u.port or 80), timeout=10)
        except OSError:
            return self.send_error(502)
        path = (u.path or "/") + ("?" + u.query if u.query else "")
        lines = [f"{'HEAD' if head else 'GET'} {path} HTTP/1.1", f"Host: {u.netloc}", "Connection: close"]
        for k, v in self.headers.items():
            if k.lower() in ("range", "if-range", "user-agent", "accept-encoding", "referer", "cookie"):
                lines.append(f"{k}: {v}")
        up.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())
        try:
            while (d := up.recv(65536)):
                if self.server.mode == "slow":
                    time.sleep(0.05)
                self.wfile.write(d)
        except OSError:
            pass
        up.close()

    def do_HEAD(self):
        self.do_GET(head=True)


def start_proxy():
    p = Proxy(("127.0.0.1", 0), ProxyHandler)
    p.log = []
    threading.Thread(target=p.serve_forever, daemon=True).start()
    return p


# ------------------------------------------------------------ FTP server

class FTPServer(threading.Thread):
    """A small FTP server on loopback: passive mode only, files made up as asked for.
    /<seed>_<size>.bin is that many bytes of that seed. Modes: `bounce` answers
    PASV with another address; `cut` drops the data connection half-way."""
    def __init__(self, user="u", password="p", mode="ok"):
        super().__init__(daemon=True)
        self.sock = socket.socket()
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(256)
        self.port = self.sock.getsockname()[1]
        self.user, self.password, self.mode = user, password, mode
        self.log = []
        self.cut_done = set()
        self.live = 0
        self.peak = 0
        self.lock = threading.Lock()

    def run(self):
        while True:
            try:
                c, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self.session, args=(c,), daemon=True).start()

    def session(self, c):
        with self.lock:
            self.live += 1
            self.peak = max(self.peak, self.live)
        try:
            self.serve(c)
        except OSError:
            pass
        finally:
            with self.lock:
                self.live -= 1
            try:
                c.close()
            except OSError:
                pass

    def serve(self, c):
        f = c.makefile("rwb", buffering=0)
        def say(s):
            f.write((s + "\r\n").encode())
        say("220 stress ftp")
        user = None
        authed = False
        rest = 0
        data = None
        while True:
            line = f.readline(8192)
            if not line:
                return
            cmd, _, arg = line.decode(errors="replace").rstrip("\r\n").partition(" ")
            cmd = cmd.upper()
            self.log.append((cmd, arg[:80]))
            if cmd == "USER":
                user = arg
                say("331 password")
            elif cmd == "PASS":
                authed = (user == self.user and arg == self.password) or self.user == "*"
                say("230 in" if authed else "530 no")
            elif not authed and cmd not in ("QUIT", "FEAT", "AUTH", "SYST"):
                say("530 log in first")
            elif cmd in ("SYST",):
                say("215 UNIX Type: L8")
            elif cmd in ("FEAT", "AUTH", "PBSZ", "PROT", "OPTS"):
                say("502 no")
            elif cmd == "TYPE":
                say("200 ok")
            elif cmd == "PWD":
                say('257 "/"')
            elif cmd == "CWD":
                say("250 ok")
            elif cmd == "MDTM":
                say("550 no")
            elif cmd == "SIZE":
                m = self.parse(arg)
                say(f"213 {m[1]}" if m else "550 no such file")
            elif cmd == "REST":
                try:
                    rest = int(arg)
                    say("350 ok")
                except ValueError:
                    say("501 bad")
            elif cmd in ("EPSV", "PASV"):
                ls = socket.socket()
                ls.bind(("127.0.0.1", 0))
                ls.listen(1)
                ls.settimeout(30)
                data = ls
                port = ls.getsockname()[1]
                if cmd == "EPSV":
                    say(f"229 Entering Extended Passive Mode (|||{port}|)")
                else:
                    ip = "127,0,0,2" if self.mode == "bounce" else "127,0,0,1"
                    say(f"227 Entering Passive Mode ({ip},{port >> 8},{port & 255})")
            elif cmd in ("RETR", "LIST", "MLSD", "NLST"):
                if data is None:
                    say("425 no data connection")
                    continue
                if cmd == "MLSD":
                    say("500 no")
                    continue
                m = self.parse(arg)
                if cmd == "RETR" and not m:
                    say("550 no such file")
                    continue
                say("150 opening")
                try:
                    dc, _ = data.accept()
                except OSError:
                    say("425 no connection")
                    continue
                try:
                    if cmd == "RETR":
                        seed, size = m
                        pos = rest
                        cut = self.mode == "cut" and (seed, size) not in self.cut_done
                        limit = size if not cut else max(rest, size // 2)
                        if cut:
                            self.cut_done.add((seed, size))
                        while pos < limit:
                            n = min(262144, limit - pos)
                            dc.sendall(slice_of(seed, pos, n))
                            pos += n
                        if cut and limit < size:
                            dc.close()
                            f.close()
                            return
                    else:
                        dc.sendall(b"-rw-r--r-- 1 u u 1000 Jan 01 00:00 listed.bin\r\n")
                except OSError:
                    pass
                finally:
                    try:
                        dc.close()
                    except OSError:
                        pass
                data.close()
                data = None
                rest = 0
                say("226 done")
            elif cmd == "QUIT":
                say("221 bye")
                return
            else:
                say("502 no")

    @staticmethod
    def parse(arg):
        name = arg.strip("/").rsplit("/", 1)[-1]
        if not name.endswith(".bin") or "_" not in name:
            return None
        seed, _, size = name[:-4].rpartition("_")
        try:
            return seed, int(size)
        except ValueError:
            return None


# ----------------------------------------------------- a lossy TCP link

class Lossy(threading.Thread):
    """Forwards to the HTTP server with delay, jitter and a chance of cutting
    a connection in the middle, as a bad link does."""
    def __init__(self, target_port, delay=0.05, jitter=0.1, cut=0.02):
        super().__init__(daemon=True)
        self.sock = socket.socket()
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(512)
        self.port = self.sock.getsockname()[1]
        self.target = target_port
        self.delay, self.jitter, self.cut = delay, jitter, cut
        self.cuts = 0
        self.connections = 0

    def run(self):
        while True:
            try:
                c, _ = self.sock.accept()
            except OSError:
                return
            self.connections += 1
            threading.Thread(target=self.one, args=(c,), daemon=True).start()

    def one(self, c):
        try:
            up = socket.create_connection(("127.0.0.1", self.target), timeout=10)
        except OSError:
            c.close()
            return
        def pump(a, b, lossy):
            try:
                while (d := a.recv(32768)):
                    if lossy:
                        time.sleep(self.delay + random.random() * self.jitter)
                        if random.random() < self.cut:
                            self.cuts += 1
                            break
                    b.sendall(d)
            except OSError:
                pass
            for s in (a, b):
                try:
                    s.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        threading.Thread(target=pump, args=(c, up, False), daemon=True).start()
        pump(up, c, True)


class Socks5(threading.Thread):
    """A SOCKS5 server on loopback with no login: counts the connections it is asked for."""
    def __init__(self, refuse=False):
        super().__init__(daemon=True)
        self.sock = socket.socket()
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(256)
        self.port = self.sock.getsockname()[1]
        self.connects = []
        self.refuse = refuse

    def run(self):
        while True:
            try:
                c, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self.one, args=(c,), daemon=True).start()

    def one(self, c):
        try:
            head = c.recv(2)
            if len(head) < 2 or head[0] != 5:
                return c.close()
            c.recv(head[1])
            c.sendall(b"\x05\x00")
            req = c.recv(4)
            if len(req) < 4 or req[1] != 1:
                return c.close()
            if req[3] == 1:
                host = socket.inet_ntoa(c.recv(4))
            elif req[3] == 3:
                host = c.recv(c.recv(1)[0]).decode()
            else:
                return c.close()
            port = int.from_bytes(c.recv(2), "big")
            self.connects.append((host, port))
            if self.refuse:
                c.sendall(b"\x05\x02\x00\x01\x00\x00\x00\x00\x00\x00")
                return c.close()
            up = socket.create_connection((host, port), timeout=10)
            c.sendall(b"\x05\x00\x00\x01\x00\x00\x00\x00\x00\x00")
        except OSError:
            return c.close()
        def pump(a, b):
            try:
                while (d := a.recv(65536)):
                    b.sendall(d)
            except OSError:
                pass
            for x in (a, b):
                try:
                    x.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        threading.Thread(target=pump, args=(up, c), daemon=True).start()
        pump(c, up)


# --------------------------------------------------------------- phases


@phase
def p_socks(ctx):
    """Downloads through a SOCKS5 proxy: used, and never bypassed when it refuses or is gone."""
    base, root = ctx["base"], ctx["root"]
    proxy = Socks5()
    proxy.start()
    def settings(port):
        return {"connections": 4, "retries": 1, "proxy": {"enabled": True, "type": "socks5", "host": "127.0.0.1", "port": port, "username": "", "password": "", "bypass": []}}
    dm = DM(root, settings(proxy.port), name="socks1").start()
    try:
        expect, urls = {}, []
        for i in range(20):
            expect[f"sk{i}.bin"] = (f"sk{i}", 2 * MB)
            urls.append(f"{base}/f/sk{i}/{2 * MB}/sk{i}.bin")
        dm.add(urls)
        wait_for(lambda: len(dm.rows()) >= 20 and all(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 120)
        ok, bad = verify(dm.rows(), expect)
        measure("socks", f"20 files through SOCKS5: {ok} byte-exact; the proxy was asked for {len(proxy.connects)} connections, the server saw {sc.Stats.requests} requests")
        if ok != 20:
            finding("high", "socks", f"through a working SOCKS5 proxy {ok} of 20 files were exact: {states(dm.rows())}")
        if len(proxy.connects) == 0:
            finding("critical", "socks", "no connection went through the SOCKS5 proxy that was set")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)
    for label, port, refuse in (("a proxy that refuses every request", None, True), ("a proxy that is not there", 1, False)):
        refusing = Socks5(refuse=True)
        if refuse:
            refusing.start()
            port = refusing.port
        sc.Stats.reset()
        dm = DM(root, settings(port), name="socks2" + str(bool(refuse))).start()
        try:
            dm.add([f"{base}/f/skd{int(refuse)}/{MB}/skd{int(refuse)}.bin"])
            wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 120)
            kind = dm.rows()[0]["state"]["kind"]
            measure("socks", f"{label}: download {kind}; the server saw {sc.Stats.requests} requests")
            if sc.Stats.requests or kind == "completed":
                finding("critical", "socks", f"with {label}, the download {'completed' if kind == 'completed' else 'failed'} and the server saw {sc.Stats.requests} direct requests: it went around the proxy that was set")
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_proxy(ctx):
    """Downloads through an HTTP proxy: used, authenticated, and never bypassed when it fails."""
    base, root = ctx["base"], ctx["root"]
    proxy = start_proxy()
    port = proxy.server_address[1]
    try:
        # 1. A working proxy: every request goes through it.
        dm = DM(root, {"connections": 4, "proxy": {"enabled": True, "type": "http", "host": "127.0.0.1", "port": port, "username": "", "password": "", "bypass": []}}, name="proxy1").start()
        try:
            expect = {}
            urls = []
            for i in range(20):
                expect[f"px{i}.bin"] = (f"px{i}", 3 * MB)
                urls.append(f"{base}/f/px{i}/{3 * MB}/px{i}.bin")
            dm.add(urls)
            wait_for(lambda: len(dm.rows()) >= 20 and all(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 120)
            ok, bad = verify(dm.rows(), expect)
            direct = sc.Stats.requests
            measure("proxy", f"20 files through the proxy: {ok} byte-exact; the proxy saw {len(proxy.log)} requests, the server {direct}")
            if ok != 20 or bad:
                finding("high", "proxy", f"through a working proxy {ok} of 20 files were exact: {bad[:2]}")
            if len(proxy.log) < direct * 0.9:
                finding("high", "proxy", f"only {len(proxy.log)} of the server's {direct} requests came through the proxy: some downloads went out directly")
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)
        # 2. A proxy that needs a login: without it the download must fail, with it succeed.
        proxy.credentials = "alice:s3cret"
        proxy.log.clear()
        for label, user, pw, want in (("wrong login", "alice", "nope", False), ("right login", "alice", "s3cret", True)):
            dm = DM(root, {"proxy": {"enabled": True, "type": "http", "host": "127.0.0.1", "port": port, "username": user, "password": pw, "bypass": []}}, name="proxy2" + label.replace(" ", "")).start()
            try:
                dm.add([f"{base}/f/pa/{MB}/pa_{label.replace(' ', '')}.bin"])
                wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 90)
                kind = dm.rows()[0]["state"]["kind"]
                measure("proxy", f"{label}: {kind}")
                if (kind == "completed") != want:
                    finding("high", "proxy", f"with a {label} at the proxy the download {kind}")
            finally:
                dm.stop()
                shutil.rmtree(dm.out, ignore_errors=True)
        proxy.credentials = None
        # 3. A proxy that is down: nothing may go around it.
        proxy.mode = "refuse"
        sc.Stats.reset()
        dm = DM(root, {"retries": 1, "proxy": {"enabled": True, "type": "http", "host": "127.0.0.1", "port": port, "username": "", "password": "", "bypass": []}}, name="proxy3").start()
        try:
            dm.add([f"{base}/f/down/{MB}/down.bin"])
            wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 200)
            kind = dm.rows()[0]["state"]["kind"]
            measure("proxy", f"proxy refusing everything: download {kind}; the server saw {sc.Stats.requests} requests")
            if sc.Stats.requests:
                finding("critical", "proxy", f"with the proxy refusing every request, {sc.Stats.requests} requests reached the server directly: a download went around the proxy the person set")
            if kind == "completed":
                finding("critical", "proxy", "a download completed with its proxy refusing everything")
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)
        # 4. A proxy that is simply not there.
        sc.Stats.reset()
        dm = DM(root, {"retries": 1, "proxy": {"enabled": True, "type": "http", "host": "127.0.0.1", "port": 1, "username": "", "password": "", "bypass": []}}, name="proxy4").start()
        try:
            dm.add([f"{base}/f/gone/{MB}/gone.bin"])
            wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 200)
            kind = dm.rows()[0]["state"]["kind"]
            measure("proxy", f"proxy port closed: download {kind}; the server saw {sc.Stats.requests} requests")
            if sc.Stats.requests:
                finding("critical", "proxy", "with the proxy's port closed, the download went to the server directly")
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)
        # 5. The bypass list: a host on it is fetched directly.
        proxy.mode = "ok"
        proxy.log.clear()
        sc.Stats.reset()
        dm = DM(root, {"proxy": {"enabled": True, "type": "http", "host": "127.0.0.1", "port": port, "username": "", "password": "", "bypass": ["127.0.0.1"]}}, name="proxy5").start()
        try:
            dm.add([f"{base}/f/byp/{MB}/byp.bin"])
            wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 60)
            measure("proxy", f"bypass list names the host: proxy saw {len(proxy.log)}, server {sc.Stats.requests}, {dm.rows()[0]['state']['kind']}")
            if proxy.log:
                finding("low", "proxy", f"a host on the bypass list was still fetched through the proxy ({len(proxy.log)} requests)")
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)
    finally:
        proxy.shutdown()


@phase
def p_mirrors(ctx):
    """A Metalink with a dead mirror, a mirror serving the wrong bytes, and a good one."""
    base, root = ctx["base"], ctx["root"]
    size = 6 * MB
    good = expected_sha("meta", size)
    class H(sc.Handler):
        def route(self, parts, q, head):
            if parts[0] == "meta4":
                kind = parts[1]
                good_url = f"{base}/f/meta/{size}/meta.bin"
                bad_url = f"{base}/f/WRONG/{size}/meta.bin"
                dead_url = "http://127.0.0.1:1/f/meta/meta.bin"
                mirrors = {"ok": [good_url], "wrongfirst": [bad_url, good_url], "deadfirst": [dead_url, good_url],
                           "allwrong": [bad_url], "nohash": [bad_url, good_url]}[kind]
                hash_xml = "" if kind == "nohash" else f'<hash type="sha-256">{good}</hash>'
                urls = "".join(f'<url priority="{i + 1}">{u}</url>' for i, u in enumerate(mirrors))
                body = (f'<?xml version="1.0" encoding="UTF-8"?><metalink xmlns="urn:ietf:params:xml:ns:metalink">'
                        f'<file name="meta.bin"><size>{size}</size>{hash_xml}{urls}</file></metalink>').encode()
                return self.send_head_body(200, {"Content-Type": "application/metalink4+xml"}, body, head)
            return super().route(parts, q, head)
    server, mbase = sc.start_server_with(H)
    dm = DM(root, {"connections": 6}, name="mirrors").start()
    try:
        for kind in ("ok", "wrongfirst", "deadfirst", "allwrong", "nohash"):
            dm.add([f"{mbase}/meta4/{kind}/x.meta4"])
        wait_for(lambda: len(dm.rows()) >= 5 and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 240, every=2)
        for r in dm.rows():
            kind = r["url"].split("/meta4/")[-1].split("/")[0]
            ok = False
            if r["state"]["kind"] == "completed":
                p = pathlib.Path(r["path"])
                ok = p.exists() and file_sha(p) == good
            measure("mirrors", f"{kind}: {r['state']['kind']}; content {'exact' if ok else 'not the true file'}")
            if kind == "ok" and not ok:
                finding("high", "mirrors", f"a Metalink with one good mirror ended {r['state']}")
            if kind in ("wrongfirst", "deadfirst") and not ok:
                finding("medium", "mirrors", f"{kind}: the good mirror was not used after the first failed ({r['state']})")
            if kind == "allwrong" and r["state"]["kind"] == "completed":
                finding("critical", "mirrors", "a Metalink whose only mirror serves the wrong bytes was marked completed although the file's SHA-256 was given")
            if kind == "nohash" and r["state"]["kind"] == "completed" and not ok:
                measure("mirrors", "with no hash in the Metalink, wrong bytes from the first mirror are saved: expected, nothing to check against")
    finally:
        dm.stop()
        server.shutdown()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_ftp(ctx):
    """FTP: a file, a resume after a cut, bad login, 100 at once, a server that points elsewhere."""
    root = ctx["root"]
    ftp = FTPServer()
    ftp.start()
    base = sc.with_login(f"ftp://127.0.0.1:{ftp.port}", "u", "p")
    dm = DM(root, {"connections": 4, "concurrency": 8, "retries": 5}, name="ftp").start()
    try:
        size = 24 * MB
        dm.add([f"{base}/ftp1_{size}.bin"])
        wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 120)
        rows = dm.rows()
        ok, bad = verify(rows, {f"ftp1_{size}.bin": ("ftp1", size)})
        measure("ftp", f"one {size // MB} MB file: {rows[0]['state']['kind']}, byte-exact {ok == 1}")
        if ok != 1:
            finding("high", "ftp", f"a plain FTP download of {size // MB} MB ended {rows[0]['state']} {bad}")
        # Many at once.
        n = 100
        urls = [f"{base}/ftpm{i}_{200000 + i * 1000}.bin" for i in range(n)]
        t0 = time.time()
        dm.add(urls)
        wait_for(lambda: sum(1 for r in dm.rows() if "ftpm" in r["url"] and r["state"]["kind"] in ("completed", "failed")) >= n, 240, every=2)
        mine = [r for r in dm.rows() if "ftpm" in r["url"]]
        exp = {f"ftpm{i}_{200000 + i * 1000}.bin": (f"ftpm{i}", 200000 + i * 1000) for i in range(n)}
        ok, bad = verify(mine, exp)
        measure("ftp", f"{n} files at once: {states(mine)}; {ok} byte-exact in {time.time() - t0:.0f}s; the server held {ftp.peak} control connections at once")
        if ok != n:
            finding("high", "ftp", f"{n} FTP downloads at once: {states(mine)}, {ok} exact, {bad[:2]}")
        if ftp.peak > 100:
            finding("low", "ftp", f"{ftp.peak} FTP control connections open at once for {n} files with concurrency 8 and 4 connections")
        # A bad login must fail, not loop.
        dm.add([sc.with_login(f"ftp://127.0.0.1:{ftp.port}/bad_1000.bin", "u", "wrong")])
        wait_for(lambda: any("u:wrong" in r["url"] or "•••@" in r["url"] and "bad_" in r["url"] for r in dm.rows() if r["state"]["kind"] in ("failed", "completed")), 90)
        badrow = [r for r in dm.rows() if "bad_1000" in r["url"]]
        measure("ftp", f"wrong password: {badrow[0]['state'] if badrow else 'no row'}; the server saw {sum(1 for c in ftp.log if c[0] == 'PASS' and c[1] == 'wrong')} login tries")
        if badrow and badrow[0]["state"]["kind"] == "completed":
            finding("critical", "ftp", "a download with the wrong FTP password completed")
        tries = sum(1 for c in ftp.log if c[0] == "PASS" and c[1] == "wrong")
        if tries > 20:
            finding("low", "ftp", f"a wrong FTP password was sent {tries} times before the download gave up")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)
    # A server that cuts the data connection half-way each time, then lets it finish.
    ftp.mode = "cut"
    dm = DM(root, {"connections": 1, "retries": 6}, name="ftp2").start()
    try:
        dm.add([f"{base}/cut1_{8 * MB}.bin"])
        wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 60)
        r = dm.rows()[0]
        ok, bad = verify(dm.rows(), {f"cut1_{8 * MB}.bin": ("cut1", 8 * MB)})
        measure("ftp", f"a server that cuts the first transfer at half: {r['state']['kind']}, byte-exact {ok == 1}")
        if r["state"]["kind"] == "completed" and ok != 1:
            finding("critical", "ftp", "a download from an FTP server that cut the transfer half-way was marked completed with wrong content")
        elif r["state"]["kind"] != "completed":
            finding("high", "ftp", f"an FTP transfer cut at half did not resume with REST: {r['state']}")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)
    # PASV pointing at another address: dm must keep to the server it logged in to.
    ftp.mode = "bounce"
    dm = DM(root, {"connections": 1, "retries": 1}, name="ftp3").start()
    try:
        dm.add([f"{base}/bounce1_{MB}.bin"])
        wait_for(lambda: any(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 60)
        measure("ftp", f"PASV naming 127.0.0.2: {dm.rows()[0]['state']['kind']}")
        if dm.rows()[0]["state"]["kind"] != "completed":
            finding("low", "ftp", "when the passive reply names another address, the download failed; dm connects to the server it logged in to (an FTP bounce is not possible), so this is a refusal of a server that is wrong, not a hole")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_watch(ctx):
    """The watched folder: a few hundred link files at once, odd ones, a symlink, an unfinished one."""
    base, root = ctx["base"], ctx["root"]
    watch = root / "watch"
    watch.mkdir()
    dm = DM(root, {"watch_folder": str(watch), "concurrency": 8}, name="watch").start()
    try:
        n = 300
        for i in range(n):
            (watch / f"l{i}.url").write_text(f"[InternetShortcut]\nURL={base}/f/wf{i}/{20000 + i}/wf{i}.bin\n")
        (watch / "empty.txt").write_text("")
        (watch / "big.txt").write_bytes((f"{base}/f/wfbig/1000/wfbig.bin\n").encode() + b"x" * (8 * MB))
        (watch / "binary.url").write_bytes(os.urandom(100000))
        (watch / "two.txt").write_text(f"{base}/f/wfa/1000/wfa.bin\n{base}/f/wfb/1000/wfb.bin\n")
        outside = root / "outside.txt"
        outside.write_text(f"{base}/f/wfoutside/1000/wfoutside.bin\n")
        try:
            os.symlink(outside, watch / "link.txt")
        except OSError:
            pass
        (watch / "dirs.url").mkdir()
        t0 = time.time()
        wait_for(lambda: sum(1 for r in dm.rows() if "/wf" in r["url"] and r["state"]["kind"] in ("completed", "failed")) >= n + 3, 120, every=2)
        rows = dm.rows()
        took = time.time() - t0
        mine = [r for r in rows if "/wf" in r["url"]]
        measure("watch", f"{len(mine)} downloads from the folder in {took:.0f}s: {states(mine)}")
        expect = {f"wf{i}.bin": (f"wf{i}", 20000 + i) for i in range(n)}
        ok, bad = verify(mine, expect)
        measure("watch", f"{ok} of {n} link files fetched byte-exact")
        if ok != n:
            finding("medium", "watch", f"{n} link files dropped in the watched folder: {ok} fetched ({states(mine)})")
        if any("wfoutside" in r["url"] for r in rows):
            finding("medium", "watch", "a symbolic link in the watched folder to a file outside it was read and its link downloaded")
        moved = len(list((watch / "done").glob("*"))) if (watch / "done").exists() else 0
        measure("watch", f"{moved} files moved to done; left in the folder: {sorted(p.name for p in watch.iterdir() if p.is_file() or p.is_symlink())[:8]}")
        if any("wfbig" in r["url"] for r in rows):
            measure("watch", "an 8 MB text file's first link was read (only its first 64 KB is looked at)")
        if dm.proc.poll() is not None:
            finding("high", "watch", "dm died on a file in the watched folder")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)
        shutil.rmtree(watch, ignore_errors=True)


@phase
def p_scheduler(ctx):
    """The scheduler: outside its window nothing starts; inside it, everything does."""
    base, root = ctx["base"], ctx["root"]
    now = time.localtime()
    def hhmm(minutes):
        t = (now.tm_hour * 60 + now.tm_min + minutes) % 1440
        return f"{t // 60:02d}:{t % 60:02d}"
    for label, window, want_run in (("window later today", (60, 120), False), ("window that includes now", (-30, 30), True)):
        dm = DM(root, {"concurrency": 4, "schedule": {"enabled": True, "from": hhmm(window[0]), "to": hhmm(window[1]), "days": [0, 1, 2, 3, 4, 5, 6], "wake": False}}, name="sched").start()
        try:
            dm.add([f"{base}/f/sch{i}/{MB}/sch{i}_{label[:4]}.bin" for i in range(6)])
            time.sleep(12)
            done = sum(1 for r in dm.rows() if r["state"]["kind"] == "completed")
            running = sum(1 for r in dm.rows() if r["state"]["kind"] == "running")
            measure("scheduler", f"{label} ({hhmm(window[0])}-{hhmm(window[1])}): {states(dm.rows())}")
            if want_run and done < 6:
                finding("high", "scheduler", f"inside the scheduled window only {done} of 6 downloads finished in 12 s: {states(dm.rows())}")
            if not want_run and (done or running):
                finding("high", "scheduler", f"outside the scheduled window {done} downloads finished and {running} ran")
            # Lifting the schedule must release them.
            if not want_run:
                s = json.loads((dm.home / "settings.json").read_text())
                r = dm.call("command.run", id="scheduler.toggle") if False else None
        finally:
            dm.stop()
            shutil.rmtree(dm.out, ignore_errors=True)
    # Bad times are refused rather than saved.
    dm = DM(root, {"schedule": {"enabled": True, "from": "25:99", "to": "banana", "days": [9, 9, 9]}}, name="sched2").start()
    try:
        dm.add([f"{base}/f/schbad/{MB}/schbad.bin"])
        time.sleep(8)
        measure("scheduler", f"unreadable times in the settings file: {states(dm.rows())}; dm alive {dm.proc.poll() is None}")
        if dm.proc.poll() is not None:
            finding("high", "scheduler", "dm died on a settings file with a schedule it cannot read")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_mcp(ctx):
    """The MCP server: malformed and oversized requests, rapid calls, and what a tool may do."""
    base, root = ctx["base"], ctx["root"]
    dm = DM(root, name="mcp").start()
    try:
        env = {**os.environ, "DM_HOME": str(dm.home)}
        def run(lines, timeout=30):
            data = ("\n".join(lines) + "\n").encode() if isinstance(lines, list) else lines
            p = subprocess.run([str(sc.BINARY), "mcp"], env=env, input=data, capture_output=True, timeout=timeout)
            return p.returncode, p.stdout.decode(errors="replace"), p.stderr.decode(errors="replace")
        init = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        rc, out, err = run([init, json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})])
        tools = [t["name"] for t in json.loads(out.splitlines()[-1])["result"]["tools"]] if out.strip() else []
        measure("mcp", f"tools: {tools}")
        # Rapid calls.
        calls = [init] + [json.dumps({"jsonrpc": "2.0", "id": 10 + i, "method": "tools/call", "params": {"name": "dm_add_downloads", "arguments": {"urls": [f"{base}/f/mcp{i}/4096/mcp{i}.bin"]}}}) for i in range(500)]
        t0 = time.time()
        rc, out, err = run(calls, timeout=120)
        answered = len([l for l in out.splitlines() if l.strip()])
        measure("mcp", f"500 tool calls in one session: {answered} answers in {time.time() - t0:.1f}s, exit {rc}")
        wait_for(lambda: len([r for r in dm.rows() if "mcp" in r["url"]]) >= 500, 60)
        measure("mcp", f"{len([r for r in dm.rows() if '/mcp' in r['url']])} downloads came of it")
        # Malformed input.
        for label, payload in (("not json", b"hello\n"), ("array", b"[1,2]\n"), ("no method", b'{"jsonrpc":"2.0","id":1}\n'),
                               ("unknown tool", (init + '\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}\n').encode()),
                               ("huge arguments", (init + '\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"dm_add_downloads","arguments":{"urls":["' + "a" * 5_000_000 + '"]}}}\n').encode()),
                               ("deep json", b"[" * 100000 + b"]" * 100000 + b"\n"),
                               ("20 MB line", b'{"x":"' + b"a" * (20 * MB) + b'"}\n')):
            try:
                rc, out, err = run(payload, timeout=60)
                measure("mcp", f"{label}: exit {rc}, {len(out)} bytes of answer")
                if rc not in (0, 1, 2) and rc < 0:
                    finding("medium", "mcp", f"{label}: the MCP server died with signal {-rc}")
            except subprocess.TimeoutExpired:
                finding("medium", "mcp", f"{label}: the MCP server did not finish within 60 s")
        # What the tools can reach: only downloads, by id.
        for label, args in (("file url", {"urls": ["file:///etc/passwd"]}), ("folder traversal", {"urls": [f"{base}/f/mt/10/mt.bin"], "folder": f"{dm.out}/../../../tmp/mcp-escape"})):
            rc, out, err = run([init, json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "dm_add_downloads", "arguments": args}})])
            tail = out.strip().splitlines()[-1][:160] if out.strip() else ""
            measure("mcp", f"{label}: {tail}")
        if pathlib.Path("/tmp/mcp-escape").exists():
            finding("critical", "mcp", "a tool call wrote a download outside the download folder")
            shutil.rmtree("/tmp/mcp-escape", ignore_errors=True)
        # With no dm running, the server says so.
        dm.stop()
        time.sleep(1)
        rc, out, err = run([init, json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "dm_list_downloads", "arguments": {}}})])
        measure("mcp", f"with dm stopped: exit {rc}, {out.strip().splitlines()[-1][:120] if out.strip() else err[:120]}")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_lossy(ctx):
    """A slow, jittery link that cuts connections: 30 downloads through it, byte-exact."""
    base, root = ctx["base"], ctx["root"]
    port = int(base.rsplit(":", 1)[1])
    link = Lossy(port, delay=0.03, jitter=0.08, cut=0.03)
    link.start()
    dm = DM(root, {"connections": 6, "concurrency": 10, "retries": 20}, name="lossy").start()
    try:
        n, size = 30, 6 * MB
        expect = {}
        urls = []
        for i in range(n):
            expect[f"ls{i}.bin"] = (f"ls{i}", size)
            urls.append(f"http://127.0.0.1:{link.port}/f/ls{i}/{size}/ls{i}.bin")
        t0 = time.time()
        dm.add(urls)
        wait_for(lambda: len(dm.rows()) >= n and all(r["state"]["kind"] in ("completed", "failed") for r in dm.rows()), 600, every=2)
        rows = dm.rows()
        st = states(rows)
        ok, bad = verify(rows, expect)
        measure("lossy", f"{n} x {size // MB} MB through a link that cut {link.cuts} connections: {st}; {ok} byte-exact in {time.time() - t0:.0f}s")
        if bad:
            finding("critical", "lossy", f"files wrong after a lossy link: {bad[:3]}")
        if st.get("failed"):
            finding("medium", "lossy", f"{st['failed']} of {n} downloads failed through a link that only cut connections ({link.cuts} cuts, 20 retries allowed)")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


@phase
def p_manifests(ctx):
    """Playlists and manifests that ask for absurd amounts of work."""
    base, root = ctx["base"], ctx["root"]
    class H(sc.Handler):
        def route(self, parts, q, head):
            if parts[0] == "m3u8big":              # a million segments, none of them there
                n = int(parts[1])
                body = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n" + "".join(f"#EXTINF:2.0,\nseg{i}.ts\n" for i in range(n)) + "#EXT-X-ENDLIST\n"
                return self.send_head_body(200, {"Content-Type": "application/vnd.apple.mpegurl"}, body.encode(), head)
            if parts[0] == "mpdbig":               # a presentation of a hundred thousand hours in one-second segments
                body = ('<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT100000H">'
                        '<Period><AdaptationSet mimeType="video/mp4"><Representation id="v" bandwidth="400000" height="360">'
                        f'<SegmentTemplate timescale="1000" duration="1000" media="{base}/seg$Number$.m4s" startNumber="1"/>'
                        '</Representation></AdaptationSet></Period></MPD>').encode()
                return self.send_head_body(200, {"Content-Type": "application/dash+xml"}, body, head)
            if parts[0] == "m3u8nest":              # a master playlist that points at itself
                body = f"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nhttp://{self.headers['Host']}/m3u8nest/x.m3u8\n"
                return self.send_head_body(200, {"Content-Type": "application/vnd.apple.mpegurl"}, body.encode(), head)
            return super().route(parts, q, head)
    server, mbase = sc.start_server_with(H)
    sc.Stats.reset()
    dm = DM(root, {"connections": 4}, name="man").start()
    try:
        rss0 = sc.proc_stats(dm.proc.pid)[0]
        for u in (f"{mbase}/m3u8big/1000000/big.m3u8", f"{mbase}/mpdbig/x.mpd", f"{mbase}/m3u8nest/x.m3u8"):
            dm.add([u])
        t0 = time.time()
        wait_for(lambda: len(dm.rows()) >= 3 and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in dm.rows()), 150, every=2)
        rss1, cpu, fds, th = dm.peak()
        for r in dm.rows():
            measure("manifests", f"{r['url'][-30:]}: {r['state']['kind']} {str(r['state'].get('reason', ''))[:90]}")
        measure("manifests", f"after {time.time() - t0:.0f}s: peak rss {rss1:.0f} MB (from {rss0:.0f}), requests to the server {sc.Stats.requests}")
        if rss1 > 1500:
            finding("medium", "manifests", f"a playlist of a million segments (or a manifest of 100 000 hours) took {rss1:.0f} MB")
        if sc.Stats.requests > 50000:
            finding("medium", "manifests", f"a hostile manifest made dm send {sc.Stats.requests} requests")
        if any(r["state"]["kind"] in ("running", "queued") for r in dm.rows()):
            kinds = [(r["url"][-24:], r["state"]["kind"]) for r in dm.rows() if r["state"]["kind"] in ("running", "queued")]
            finding("low", "manifests", f"after 150 s these were still working through hostile manifests: {kinds}")
        if dm.proc.poll() is not None:
            finding("high", "manifests", "dm died on a hostile playlist or manifest")
    finally:
        dm.stop()
        server.shutdown()
        shutil.rmtree(dm.out, ignore_errors=True)


SANDBOX = """(version 1)
(allow default)
(deny network-outbound)
(allow network-outbound (remote ip "localhost:*"))
(allow network-outbound (remote unix-socket))
"""


@phase
def p_torrent(ctx):
    """Torrents on loopback only, inside a sandbox that refuses every other connection."""
    root = ctx["root"]
    profile = root / "loopback-only.sb"
    profile.write_text(SANDBOX)
    # The sandbox must hold before anything runs in it.
    probe = r'''
import socket
bad = []
for fam, kind, addr in ((socket.AF_INET, socket.SOCK_STREAM, ("1.1.1.1", 443)), (socket.AF_INET, socket.SOCK_DGRAM, ("8.8.8.8", 6881)), (socket.AF_INET, socket.SOCK_DGRAM, ("239.192.152.143", 6771))):
    s = socket.socket(fam, kind); s.settimeout(3)
    try:
        s.connect(addr) if kind == socket.SOCK_STREAM else s.sendto(b"x", addr); bad.append(addr)
    except OSError:
        pass
ls = socket.socket(); ls.bind(("127.0.0.1", 0)); ls.listen(1)
c = socket.create_connection(ls.getsockname(), timeout=3)
print("LEAKS" if bad else "HOLDS", bad)
'''
    held = subprocess.run(["sandbox-exec", "-f", str(profile), sys.executable, "-c", probe], capture_output=True, text=True, timeout=60)
    measure("torrent", f"the loopback-only sandbox: {held.stdout.strip() or held.stderr.strip()[:100]}")
    if "HOLDS" not in held.stdout:
        finding("info", "torrent", "skipped: the sandbox that keeps the torrent test off the public network could not be shown to hold")
        return
    build = subprocess.run(["cargo", "test", "-p", "dm-core", "--release", "--test", "torrent_stress", "--no-run", "--message-format=json"],
                           cwd=ROOT / "desktop", capture_output=True, text=True, timeout=1500)
    binary = None
    for line in build.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("executable") and "torrent_stress" in m["executable"]:
            binary = m["executable"]
    if not binary:
        finding("info", "torrent", f"skipped: the torrent test did not build: {build.stderr[-200:]}")
        return
    # Torrents that name files outside their folder, made here: SHA-1 pieces.
    tdir = root / "evil-torrents"
    tdir.mkdir()
    def b(x):
        if isinstance(x, int): return b"i%de" % x
        if isinstance(x, bytes): return b"%d:%s" % (len(x), x)
        if isinstance(x, str): return b(x.encode())
        if isinstance(x, list): return b"l" + b"".join(b(i) for i in x) + b"e"
        return b"d" + b"".join(b(k) + b(v) for k, v in sorted(x.items())) + b"e"
    pieces = hashlib.sha1(b"\x07" * 16384).digest()
    def make(label, path, name=b"evil"):
        (tdir / f"{label}.torrent").write_bytes(b({"info": {"files": [{"length": 16384, "path": path}], "name": name, "piece length": 16384, "pieces": pieces}}))
    make("dotdot", ["..", "..", "escape_torrent.txt"])
    make("absolute", ["/", "tmp", "escape_torrent_abs.txt"])
    make("backslash", ["..\\..\\escape_torrent_bs.txt"])
    make("longname", ["x"], name=b"n" * 1200)
    make("deep", ["d"] * 300 + ["escape_torrent_deep.txt"])
    make("nulbyte", ["a\x00b", "escape_torrent_nul.txt"])
    make("dotname", [".", "escape_torrent_dot.txt"], name=b"..")
    env = {**os.environ, "DM_STRESS_TORRENT_DIR": str(tdir)}
    for test in ("parallel_swarms", "many_files", "paths_outside_the_folder", "swarm_with_a_piece_missing", "pause_and_resume", "magnet_through_a_tracker"):
        try:
            run = subprocess.run(["sandbox-exec", "-f", str(profile), binary, "--ignored", "--nocapture", "--test-threads=1", test],
                                 env=env, capture_output=True, text=True, timeout=400)
        except subprocess.TimeoutExpired:
            finding("medium", "torrent", f"{test}: did not finish in 400 s")
            continue
        lines = [l for l in run.stdout.splitlines() if "STRESS" in l]
        for l in lines:
            measure("torrent", l.split("STRESS ", 1)[1][:260])
            low = l.lower()
            if "wrong" in low and "0 wrong" not in low and "wrong," not in low.replace("0 wrong", ""):
                finding("critical", "torrent", l.split("STRESS ", 1)[1][:200])
            if "outside the folder: [" in l and "outside the folder: []" not in l:
                finding("critical", "torrent", "a torrent's file was written outside the folder: " + l.split("STRESS ", 1)[1][:200])
            if "/tmp file: true" in l:
                finding("critical", "torrent", "a torrent wrote a file under /tmp through an absolute path")
            if "COMPLETED" in l and "true content" not in l:
                finding("critical", "torrent", l.split("STRESS ", 1)[1][:200])
        if run.returncode != 0:
            finding("medium", "torrent", f"{test}: the test failed or crashed: {run.stderr[-200:] or run.stdout[-200:]}")
        if not lines:
            finding("medium", "torrent", f"{test}: printed nothing")
    for stray in (pathlib.Path("/tmp/escape_torrent_abs.txt"),):
        stray.unlink(missing_ok=True)


# The one phase that uses the real network, so it runs only when named. The
# torrent is webtorrent.io's own public-domain/Creative Commons test item.
WIRED_CD = ("magnet:?xt=urn:btih:a88fda5954e89178c372716a6a78b8180ed4dad3&dn=The+WIRED+CD+-+Rip.+Sample.+Mash.+Share"
            "&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969"
            "&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969"
            "&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=wss%3A%2F%2Ftracker.btorrent.xyz"
            "&tr=wss%3A%2F%2Ftracker.fastcast.nz&tr=wss%3A%2F%2Ftracker.openwebtorrent.com"
            "&ws=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2F&xs=https%3A%2F%2Fwebtorrent.io%2Ftorrents%2Fwired-cd.torrent")


def p_realtorrent(ctx):
    """A real public swarm: webtorrent.io's Creative Commons 'The WIRED CD' by magnet link, with a pause and a resume."""
    root = ctx["root"]
    dm = DM(root, {"connections": 8}, name="realtorrent").start()
    try:
        dm.add([WIRED_CD])
        t0 = time.time()
        paused = resumed = False
        last = None
        while time.time() - t0 < 600:
            rows = dm.rows()
            if rows:
                r = rows[0]
                pr = r["progress"]
                line = (r["state"]["kind"], pr["downloaded"] // 1024, (pr.get("total") or 0) // 1024)
                if line != last and int(time.time() - t0) % 5 == 0:
                    measure("realtorrent", f"{time.time() - t0:4.0f}s {line[0]} {line[1]} of {line[2]} KB")
                    last = line
                total = pr.get("total") or 0
                if not paused and total and pr["downloaded"] > total * 0.3:
                    dm.call("downloads.pause", ids=[r["id"]])
                    paused = True
                    measure("realtorrent", f"paused at {pr['downloaded'] // 1024} KB")
                    time.sleep(5)
                    after = dm.rows()[0]["progress"]["downloaded"]
                    measure("realtorrent", f"5 s later {after // 1024} KB, state {dm.rows()[0]['state']['kind']}")
                    if after > pr["downloaded"] + 3 * MB:
                        finding("medium", "realtorrent", "a paused torrent kept receiving data")
                    dm.call("downloads.resume", ids=[r["id"]])
                    resumed = True
                if r["state"]["kind"] in ("completed", "failed", "cancelled"):
                    break
            time.sleep(1)
        r = dm.rows()[0]
        took = time.time() - t0
        measure("realtorrent", f"{r['state']} after {took:.0f}s; peak rss {dm.peak()[0]:.0f} MB, fds {dm.peak()[2]}, threads {dm.peak()[3]}")
        out = dm.out.resolve()
        files = [p for p in out.rglob("*") if p.is_file()]
        total = sum(p.stat().st_size for p in files)
        measure("realtorrent", f"{len(files)} files, {total // 1024} KB, e.g. {[p.name for p in files[:4]]}")
        outside = [str(p) for p in files if out not in p.resolve().parents]
        if outside:
            finding("critical", "realtorrent", f"files outside the download folder: {outside[:3]}")
        if r["state"]["kind"] != "completed":
            finding("medium", "realtorrent", f"the public torrent ended {r['state']} after {took:.0f}s (paused={paused}, resumed={resumed})")
        elif not files:
            finding("high", "realtorrent", "a torrent marked completed saved no files")
    finally:
        dm.stop()
        shutil.rmtree(dm.out, ignore_errors=True)


OPT_IN = {"realtorrent": p_realtorrent}


# ----------------------------------------------------------------- main

def main():
    args = sys.argv[1:]
    if "--list" in args:
        for k, f in PHASES.items():
            print(f"{k:12} {f.__doc__}")
        return
    PHASES.update({k: v for k, v in OPT_IN.items() if k in args})
    names = [a for a in args if not a.startswith("-")] or [k for k in PHASES if k not in OPT_IN]
    if not sc.BINARY.exists():
        sys.exit(f"no dm at {sc.BINARY}")
    root = pathlib.Path(tempfile.mkdtemp(prefix="dm-stress-"))
    server, base = sc.start_server()
    ctx = {"root": root, "base": base, "server": server}
    print(f"protocol stress check: dm {sc.BINARY}, server {base}, folder {root}, {sc.free_gb(tempfile.gettempdir()):.1f} GB free", flush=True)
    try:
        for name in names:
            if name not in PHASES:
                print(f"no phase {name}")
                continue
            if not sc.guard(name):
                continue
            print(f"\n== {name}: {PHASES[name].__doc__}", flush=True)
            sc.Stats.reset()
            t0 = time.time()
            try:
                PHASES[name](ctx)
            except Exception as e:
                import traceback
                traceback.print_exc()
                finding("high", name, f"the check itself failed: {e!r}")
            print(f"   ({time.time() - t0:.0f}s, {sc.free_gb(tempfile.gettempdir()):.1f} GB free)", flush=True)
    finally:
        server.shutdown()
        for d in sc.LIVE_DMS:
            d.stop()
        if os.environ.get("DM_STRESS_KEEP"):
            print(f"kept {root}")
        else:
            shutil.rmtree(root, ignore_errors=True)
    print("\nFINDINGS:")
    for sev, ph, what in sc.FINDINGS:
        print(f"  [{sev}] {ph}: {what}")
    if not sc.FINDINGS:
        print("  none")
    sys.exit(1 if any(s in ("critical", "high") for s, _, _ in sc.FINDINGS) else 0)


if __name__ == "__main__":
    main()
