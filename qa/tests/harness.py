"""The QA harness: a headless dm of each scenario's own, the servers, the checks.

In the compose stack the servers are other containers (ORIGIN, TOXIPROXY,
MOCKVIDEO). Run on a developer's computer with none set, the origin runs in
this process and scenarios that need the others are skipped, never passed.
"""
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
import urllib.parse
import urllib.request

QA = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(QA / "lib"))
sys.path.insert(0, str(QA / "origin"))
import content  # noqa: E402

ROOT = QA.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")
DOWNLOADS = pathlib.Path(os.environ.get("QA_DOWNLOADS") or "/downloads")
if not DOWNLOADS.exists():
    DOWNLOADS = pathlib.Path(tempfile.mkdtemp(prefix="qa-downloads-"))
TOXIPROXY = os.environ.get("TOXIPROXY")
MOCKVIDEO = os.environ.get("MOCKVIDEO")

if os.environ.get("ORIGIN"):
    ORIGIN = os.environ["ORIGIN"]
    ORIGIN_HOST = os.environ.get("ORIGIN_HOST", urllib.parse.urlsplit(ORIGIN).hostname)
else:
    import origin as _origin

    _server = _origin.Server(("127.0.0.1", 0), _origin.Handler)
    threading.Thread(target=_server.serve_forever, daemon=True).start()
    ORIGIN = f"http://127.0.0.1:{_server.server_address[1]}"
    ORIGIN_HOST = "127.0.0.1"


class Skip(Exception):
    """A scenario that cannot run here (a service missing): reported as skipped."""


class Fail(AssertionError):
    pass


def check(cond, what):
    if not cond:
        raise Fail(what)


def seed_of(name):
    return sum(name.encode()) * 131 + len(name)


def url(name, size, **opts):
    """An origin file; its expected bytes are content.py's for (seed_of(name), size)."""
    q = urllib.parse.urlencode({"size": size, **opts})
    return f"{ORIGIN}/f/{urllib.parse.quote(name)}?{q}"


def stats(name=None):
    s = json.load(urllib.request.urlopen(f"{ORIGIN}/stats", timeout=10))
    return s.get(name, {}) if name else s


# ------------------------------------------------------------------ dm

class DM:
    """One headless dm with a home and settings of its own."""

    def __init__(self, scenario, settings=None, out=None):
        self.root = pathlib.Path(tempfile.mkdtemp(prefix=f"qa-{scenario}-"))
        self.home = self.root / "home"
        self.home.mkdir()
        self.out = out or (DOWNLOADS / scenario)
        self.out.mkdir(parents=True, exist_ok=True)
        self.settings = {"download_dir": str(self.out), "categorize": False, "show_progress": False,
                         "show_complete": False, "check_updates": False, "watch_clipboard": False,
                         **(settings or {})}
        self.proc = None
        self.stderr = b""
        self.conn = None

    def start(self):
        (self.home / "settings.json").write_text(json.dumps(self.settings))
        for stale in ("control.port", "control.token"):
            (self.home / stale).unlink(missing_ok=True)
        self.errlog = open(self.root / "stderr.log", "ab")
        self.proc = subprocess.Popen([str(BINARY), "--headless", "--control", "0"],
                                     env={**os.environ, "DM_HOME": str(self.home)},
                                     stdout=subprocess.DEVNULL, stderr=self.errlog)
        deadline = time.time() + 60
        while not ((self.home / "control.port").exists() and (self.home / "control.token").exists()
                   and (self.home / "control.port").read_text().strip()):
            if self.proc.poll() is not None:
                raise Fail(f"dm exited at start ({self.proc.returncode}): {self.log()[-800:]}")
            if time.time() > deadline:
                raise Fail("dm never opened its control port")
            time.sleep(0.1)
        self.conn = socket.create_connection(("127.0.0.1", int((self.home / "control.port").read_text())), timeout=60)
        self.replies = self.conn.makefile("r")
        r = self.call("auth", token=(self.home / "control.token").read_text().strip())
        check(r.get("ok"), f"auth refused: {r}")
        return self

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        line = self.replies.readline()
        if not line:
            raise Fail(f"dm closed the control connection on {method}: {self.log()[-800:]}")
        return json.loads(line)

    def add(self, *urls, **kw):
        r = self.call("downloads.add", urls=list(urls), **kw)
        check(r.get("ok"), f"downloads.add refused: {r}")
        return r["result"]

    def rows(self):
        r = self.call("queue.list")
        return r.get("result", []) if r.get("ok") else []

    def row(self, needle):
        for r in self.rows():
            if needle in r["url"]:
                return r
        return None

    def wait(self, needle, kinds=("completed", "failed", "cancelled"), seconds=60):
        deadline = time.time() + seconds
        last = None
        while time.time() < deadline:
            last = self.row(needle)
            if last and last["state"]["kind"] in kinds:
                return last
            if self.proc.poll() is not None:
                raise Fail(f"dm died ({self.proc.returncode}) while waiting for {needle}: {self.log()[-800:]}")
            time.sleep(0.2)
        raise Fail(f"{needle} not {'/'.join(kinds)} after {seconds}s: {last and last['state']} "
                   f"{last and last['progress']}")

    def act(self, method, row):
        return self.call(method, ids=[row["id"]])

    def log(self):
        try:
            return (self.root / "stderr.log").read_text(errors="replace")
        except OSError:
            return ""

    def pid_stats(self):
        """RSS in KB, open file descriptors, threads (Linux /proc, else ps)."""
        pid = self.proc.pid
        proc = pathlib.Path(f"/proc/{pid}")
        if proc.exists():
            status = (proc / "status").read_text()
            rss = int(next(l.split()[1] for l in status.splitlines() if l.startswith("VmRSS")))
            threads = int(next(l.split()[1] for l in status.splitlines() if l.startswith("Threads")))
            return rss, len(list((proc / "fd").iterdir())), threads
        out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
        fds = len(subprocess.run(["lsof", "-p", str(pid)], capture_output=True, text=True).stdout.splitlines()) - 1
        return int(out[0]) if out else 0, fds, 0

    def stop(self, kill=False):
        if self.proc and self.proc.poll() is None:
            (self.proc.kill if kill else self.proc.terminate)()
            try:
                self.proc.wait(20)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(10)
        if self.conn:
            try:
                self.conn.close()
            except OSError:
                pass
            self.conn = None

    def close(self):
        self.stop()
        log = self.log()
        # Its files go with it: the scenarios share a 200 MB disk, and what
        # one left made the next one's download "not fit" (BUGLOG.md, T-008).
        shutil.rmtree(self.out, ignore_errors=True)
        shutil.rmtree(self.root, ignore_errors=True)
        check("panicked" not in log, f"dm panicked: {log[-1500:]}")


# ------------------------------------------------------------------ checks

def exact(row, name, size):
    """The row completed and its file is byte for byte the origin's."""
    check(row["state"]["kind"] == "completed", f"{name}: {row['state']}")
    p = pathlib.Path(row["path"])
    check(p.exists(), f"{name}: {p} missing")
    check(p.stat().st_size == size, f"{name}: size {p.stat().st_size} != {size}")
    check(content.file_sha256(p) == content.sha256(seed_of(name), size), f"{name}: content differs")
    return p


def no_leftovers(dm, row=None):
    """Nothing unfinished left: no parts folder for the row, no part files beside the file."""
    if row is not None:
        parts = pathlib.Path(row.get("parts_dir") or dm.home / "parts" / str(row["id"]))
        if parts.exists() and any(parts.iterdir()):
            raise Fail(f"parts left in {parts}: {[p.name for p in parts.iterdir()][:5]}")
    junk = [p.name for p in dm.out.rglob("*") if p.is_file() and (p.suffix in (".part", ".tmp", ".dmpart", ".crdownload") or p.name.startswith("."))]
    check(not junk, f"unfinished files in the download folder: {junk[:5]}")


def reason(row):
    return (row["state"].get("reason") or "") if isinstance(row["state"], dict) else ""


def toxiproxy(method, path, body=None):
    if not TOXIPROXY:
        raise Skip("no toxiproxy")
    req = urllib.request.Request(TOXIPROXY + path, method=method,
                                 data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=10) as r:
        text = r.read()
        return json.loads(text) if text else None


def proxied(port=18080):
    """A toxiproxy proxy to origin:8080, fresh, and its address for dm."""
    if not TOXIPROXY:
        raise Skip("no toxiproxy")
    try:
        toxiproxy("DELETE", "/proxies/origin")
    except Exception:
        pass
    host = urllib.parse.urlsplit(ORIGIN)
    toxiproxy("POST", "/proxies", {"name": "origin", "listen": f"0.0.0.0:{port}",
                                   "upstream": f"{host.hostname}:{host.port or 80}", "enabled": True})
    return f"http://{urllib.parse.urlsplit(TOXIPROXY).hostname}:{port}"
