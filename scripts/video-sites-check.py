#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The video website compatibility test plan, run against the real dm.

    python scripts/video-sites-check.py                        # engine, public-url, public-panel
    python scripts/video-sites-check.py --phases engine        # one or more phases, comma-separated
    python scripts/video-sites-check.py --auth youtube,reddit  # then the optional signed-in suite
    python scripts/video-sites-check.py --list                 # what each test is, and stop
    python scripts/video-sites-check.py --phases public-panel --panel-sites twitch,bilibili   # again, a few

Signed out first, in this order, each phase on a throwaway home that is
deleted at the end (the report is kept):

  engine        CORE-08..14 and ENG-01..08 against a server on 127.0.0.1 that
                this script runs: byte ranges and none, redirects, pause and
                resume, a quit and a kill mid-download, dropped connections,
                429/503 with Retry-After, the server going away, an address
                that expires, a destination that is not a folder, a file too
                big for the disk (refused before anything is written) and a
                file past 4 GiB. Every byte that lands is compared with what
                was served.
  public-url    Real public sites through dm's own Add URL (the control port's
                `downloads.add`), headless: YouTube (dm's own reader: best, a
                height, a picture-and-sound pair, sound alone), Internet
                Archive (each format's MD5 from the item's metadata), Wikimedia
                Commons (the original's SHA-1 from the API, and a transcode),
                NASA's library, Test-Videos.co.uk, an HLS and a DASH test
                stream, a pause and resume of a stream, and refusals (a video
                that does not exist, one that needs a sign-in).
  public-panel  The video sites dm reaches through its browser panel
                (scripts/sites-check.py): YouTube, Vimeo, Dailymotion, Rumble,
                Reddit, TikTok, Twitch (a past broadcast and a clip),
                Bilibili and PeerTube, in Microsoft Edge with a new profile
                that has never signed in to anything.
  --auth        Optional, after the public phases and never part of the
                release gate: for each site named, a visible Edge opens at its
                sign-in page and waits while the tester signs in BY HAND with
                an account of their own. The script never creates accounts,
                never types, reads or keeps a password, and its profile is the
                tester's to keep (DM_AUTH_PROFILES) or is deleted with the run.

dm is given no cookies file and no login in the public phases, and each row's
`signed_in` (whether the row holds any cookies) is read back to prove it.
A site that shows a login wall, a bot check or an error page to a signed-out
visitor is BLOCKED, never retried signed in and counted as a public pass.

Results are PASS, FAIL, BLOCKED, UNSUPPORTED or N/A, as the plan defines them,
written with the plan's evidence fields to report.md and report.json in the
report folder, with the release gate worked out from them.

Environment:
  DM_BINARY          dm-gpui to test (default: $CARGO_TARGET_DIR/debug, then desktop/target/debug)
  DM_VIDEO_WORK      folder for the run's throwaway files (default: the system's temp)
  DM_VIDEO_REPORT    where the report goes (default: <work>/report, kept after the run)
  DM_TEST_LARGE_GB   size of ENG-08's file, in GiB (default 4.5; 0 skips it)
  DM_TEST_REMOVABLE  a removable drive's folder for ENG-07b (asks you to pull the drive)
  DM_NASA_MAX_MB     the largest NASA original to fetch for PUB-12b (default 400)
  DM_FFMPEG          an ffmpeg to decode each video with, as well (default: ffmpeg on PATH)
  DM_AUTH_PROFILES   folder of kept browser profiles for --auth, one per site
  SITES_URL_<NAME>   a page for a panel site instead of the one found (see sites-check.py)

Not in CI: it reaches real sites, which change. Needs a built dm, and for
public-panel a packaged extension (sh scripts/package.sh) and Microsoft Edge.
"""
import argparse
import datetime
import hashlib
import http.server
import json
import os
import pathlib
import platform
import shutil
import socket
import socketserver
import ssl
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
import media_check  # noqa: E402

EXE = ".exe" if sys.platform == "win32" else ""
# The window dm ships, on every system: GPUI (docs/gaps.md G9).
SHIPPED = "dm-gpui"
# The script's own name to the sites it asks (APIs and reference copies), not a person's.
AGENT = "dm-video-sites-check/1.0 (+https://github.com/pranavo72bex/Easy-Download-Manager)"
MB = 1024 * 1024


def default_binary():
    if os.environ.get("DM_BINARY"):
        return pathlib.Path(os.environ["DM_BINARY"])
    for target in filter(None, [os.environ.get("CARGO_TARGET_DIR"), "D:/dm-target" if sys.platform == "win32" else None,
                                str(ROOT / "desktop" / "target")]):
        candidate = pathlib.Path(target) / "debug" / f"{SHIPPED}{EXE}"
        if candidate.exists():
            return candidate
    return ROOT / "desktop" / "target" / "debug" / f"{SHIPPED}{EXE}"


BINARY = default_binary()


# ------------------------------------------------------------ the plan


# (id, website, what is tested, priority, phase). The gate counts PUB P0 and every CORE/ENG.
PLAN = [
    ("CORE-08", "local server", "pause and resume: no bytes while paused, the rest fetched after", "core", "engine"),
    ("CORE-09a", "local server", "dm quits mid-download (Exit) and starts again: the row goes on", "core", "engine"),
    ("CORE-09b", "local server", "dm is killed mid-download and starts again: the row goes on", "core", "engine"),
    ("CORE-10", "local server", "connections dropped mid-body three times: retried, whole", "core", "engine"),
    ("CORE-11a", "local", "a line that is not an address is refused", "core", "engine"),
    ("CORE-11b", "local", "a host that does not exist fails with a reason", "core", "engine"),
    ("CORE-12a", "local server", "404: fails with a reason, nothing saved, no hang", "core", "engine"),
    ("CORE-12b", "local server", "410: fails with a reason, nothing saved, no hang", "core", "engine"),
    ("CORE-13a", "local server", "401: the restriction is reported, nothing saved", "core", "engine"),
    ("CORE-13b", "local server", "403: the restriction is reported, nothing saved", "core", "engine"),
    ("CORE-14", "local server", "a file bigger than the disk: refused, no success state", "core", "engine"),
    ("ENG-01", "local server", "three redirects to the file: followed, whole", "core", "engine"),
    ("ENG-02", "local server", "byte ranges: several connections, whole", "core", "engine"),
    ("ENG-03", "local server", "no byte ranges: one stream, whole", "core", "engine"),
    ("ENG-04", "local server", "an address that expires while paused: reported, not completed", "core", "engine"),
    ("ENG-05", "local server", "429, 429, 503 with Retry-After: waited, then whole", "core", "engine"),
    ("ENG-06", "local server", "the server goes away for 10 s mid-download: recovered, whole", "core", "engine"),
    ("ENG-07a", "local", "the destination folder is a file: an I/O error, not completed", "core", "engine"),
    ("ENG-07b", "removable drive", "the drive is pulled mid-download (DM_TEST_REMOVABLE)", "core", "engine"),
    ("ENG-08", "local server", "a file past 4 GiB: progress, size and bytes stay right", "core", "engine"),
    ("PUB-01a", "YouTube", "a public video, best quality, by Add URL (CORE-01, CORE-04)", "P0", "public-url"),
    ("PUB-01b", "YouTube", "a chosen height, 360 (CORE-03)", "P0", "public-url"),
    ("PUB-01c", "YouTube", "480: picture and sound fetched apart and joined (CORE-05)", "P0", "public-url"),
    ("PUB-01d", "YouTube", "sound only (CORE-07)", "P0", "public-url"),
    ("CORE-12c", "YouTube", "a video that does not exist: reported, no hang", "core", "public-url"),
    ("CORE-13c", "Vimeo", "a video that needs a sign-in, signed out: refused", "core", "public-url"),
    ("PUB-10a", "Internet Archive", "an item's MP4 file, MD5 as the item says", "P0", "public-url"),
    ("PUB-10b", "Internet Archive", "the same item in another format, MD5 as the item says", "P0", "public-url"),
    ("PUB-10c", "Internet Archive", "the item's page address pasted into Add URL (informational)", "info", "public-url"),
    ("PUB-11a", "Wikimedia Commons", "a video file's original, SHA-1 as the API says", "P1", "public-url"),
    ("PUB-11b", "Wikimedia Commons", "the same video's transcode", "P1", "public-url"),
    ("PUB-12a", "NASA media", "a library video's small MP4", "P2", "public-url"),
    ("PUB-12b", "NASA media", "the same video's original (large media)", "P2", "public-url"),
    ("DIR-01", "Test-Videos.co.uk", "an MP4 (H.264), byte for byte", "P1", "public-url"),
    ("DIR-02", "Test-Videos.co.uk", "a WebM (VP9), byte for byte", "P1", "public-url"),
    ("CORE-06", "Mux test stream", "an HLS stream (CORE-06)", "core", "public-url"),
    ("CORE-05b", "Shaka test stream", "a DASH stream, picture and sound apart (CORE-05)", "core", "public-url"),
    ("CORE-08b", "Mux test stream", "pause and resume of an HLS stream", "core", "public-url"),
    ("PUB-01p", "YouTube", "the panel on a public watch page (CORE-02, CORE-03)", "P0", "public-panel"),
    ("PUB-02", "Vimeo", "the panel on a public video", "P1", "public-panel"),
    ("PUB-03", "Dailymotion", "the panel on a public video", "P1", "public-panel"),
    ("PUB-04", "Rumble", "the panel on a public video", "P1", "public-panel"),
    ("PUB-05", "Reddit", "a public video post: picture and sound joined", "P1", "public-panel"),
    ("PUB-06", "TikTok", "a public video", "P1", "public-panel"),
    ("PUB-07a", "Twitch", "a past broadcast (HLS)", "P1", "public-panel"),
    ("PUB-07b", "Twitch", "a clip", "P1", "public-panel"),
    ("PUB-08", "Bilibili", "a public video (DASH)", "P2", "public-panel"),
    ("PUB-09", "PeerTube", "a video on video.blender.org", "P1", "public-panel"),
]
PANEL_SITES = {"youtube": "PUB-01p", "vimeo": "PUB-02", "dailymotion": "PUB-03", "rumble": "PUB-04",
               "reddit": "PUB-05", "tiktok": "PUB-06", "twitch": "PUB-07a", "twitch-clip": "PUB-07b",
               "bilibili": "PUB-08", "peertube": "PUB-09"}
# The optional suite: the site's sign-in name in sites-check.py and the pages checked once signed in.
AUTH_SITES = {
    "vimeo": ("AUTH-05", ["vimeo"]), "youtube": ("AUTH-06", ["youtube"]),
    "facebook": ("AUTH-07", ["facebook", "facebook-watch"]), "instagram": ("AUTH-08", ["instagram"]),
    "tiktok": ("AUTH-09", ["tiktok"]), "reddit": ("AUTH-R", ["reddit", "reddit-videos"]),
}
DESCRIBED = {p[0]: p for p in PLAN}

RESULTS = []


def record(test_id, result, url="", fmt="", size=None, playable="N/A", notes="", log="", **extra):
    _, site, what, priority, phase = DESCRIBED.get(test_id, (test_id, extra.pop("site", "?"), extra.pop("what", ""),
                                                             extra.pop("priority", "auth"), extra.pop("phase", "auth")))
    row = {"id": test_id, "website": site, "test": what, "priority": priority, "phase": phase, "url": url,
           "time": datetime.datetime.now().isoformat(timespec="seconds"), "result": result, "format": fmt,
           "size": size, "playable": playable, "notes": notes, "log": log[:600],
           "signed_out": extra.pop("signed_out", "Yes" if phase != "auth" else "No"),
           "cookies_to_dm": extra.pop("cookies_to_dm", "No"), **extra}
    RESULTS.append(row)
    shown = f"{size / MB:.1f} MB" if isinstance(size, (int, float)) and size else ""
    print(f"  {result:<11} {test_id:<9} {site:<18} {notes[:110]} {shown}".rstrip()
          + (f"\n              dm: {log[:200]}" if log and result != "PASS" else ""), flush=True)
    return row


def wait(probe, seconds, every=0.5):
    deadline = time.monotonic() + seconds
    while True:
        got = probe()
        if got or time.monotonic() > deadline:
            return got
        time.sleep(every)


# ------------------------------------------------------------ dm, headless


class DmError(Exception):
    pass


class Dm:
    """One dm, headless, on a home and download folder of its own, driven over its control port."""

    def __init__(self, root, **settings):
        self.root = pathlib.Path(root)
        self.home, self.downloads, self.cache = self.root / "home", self.root / "downloads", self.root / "cache"
        for d in (self.home, self.downloads, self.cache):
            d.mkdir(parents=True, exist_ok=True)
        self.settings = {
            "download_dir": str(self.downloads), "categorize": False, "connections": 8,
            "stream_quality": "best", "ask_panel_downloads": False, "ask_caught_downloads": False,
            "show_progress": False, "show_complete": False, "popups_version": 2, "sound_on_complete": False,
            "start_on_login": False, "check_updates": False, "cookies_file": "", "watch_clipboard": False,
            **settings,
        }
        (self.home / "settings.json").write_text(json.dumps(self.settings))
        self.app = None
        self.conn = None

    def start(self):
        (self.home / "control.port").unlink(missing_ok=True)
        env = dict(os.environ, DM_HOME=str(self.home), XDG_CACHE_HOME=str(self.cache))
        self.stderr = open(self.home / "stderr.txt", "ab")
        self.app = subprocess.Popen([str(BINARY), "--headless", "--control", "0"], env=env,
                                    stdout=subprocess.DEVNULL, stderr=self.stderr)
        port = wait(lambda: (self.home / "control.port").exists() and (self.home / "control.port").read_text().strip(), 60)
        if not port:
            raise DmError(f"dm did not open its control port within 60 s (see {self.home / 'stderr.txt'})")
        self.conn = socket.create_connection(("127.0.0.1", int(port)), timeout=120)
        self.replies = self.conn.makefile("r", encoding="utf-8")
        self.call("auth", token=(self.home / "control.token").read_text().strip())
        return self

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        reply = json.loads(self.replies.readline())
        if not reply.get("ok", True):
            raise DmError(reply.get("error"))
        return reply.get("result")

    def rows(self):
        return self.call("queue.list") or []

    def row(self, ident):
        return next((r for r in self.rows() if r.get("id") == ident), None)

    def add(self, url, **options):
        """The new row's id, or raises DmError with dm's own words."""
        before = {r["id"] for r in self.rows()}
        self.call("downloads.add", urls=[url], again=True, **options)
        new = wait(lambda: [r for r in self.rows() if r["id"] not in before], 20)
        if not new:
            raise DmError("dm said nothing and listed no new row")
        return new[0]["id"]

    def settle(self, ident, seconds):
        """The row once it is finished, failed, cancelled, or held with a reason it cannot get past alone."""
        def done():
            r = self.row(ident)
            return r if r and (r["state"]["kind"] in ("completed", "failed", "cancelled") or held(r)) else None
        return wait(done, seconds, 1) or self.row(ident)

    def stop(self, graceful=True):
        if not self.app:
            return
        if graceful and self.conn:
            try:
                self.call("command.run", id="program.exit")
            except (DmError, OSError, ValueError):
                pass
            try:
                self.app.wait(20)
            except subprocess.TimeoutExpired:
                pass
        if self.app.poll() is None:
            self.app.kill()
            self.app.wait(15)
        try:
            self.conn and self.conn.close()
        except OSError:
            pass
        self.conn = None
        self.stderr.close()

    def kill(self):
        self.app.kill()
        self.app.wait(15)
        try:
            self.conn.close()
        except OSError:
            pass
        self.conn = None
        self.stderr.close()


def held(row):
    """A row dm paused itself, saying why (`stalled`): no room on the disk, a folder it cannot write."""
    return bool(row) and row["state"]["kind"] == "paused" and bool(row.get("stalled"))


def state(row):
    """(kind, dm's words): a failure's reason, or what holds a paused row."""
    if not row:
        return "missing", ""
    return row["state"]["kind"], str(row["state"].get("reason") or row.get("stalled") or "")


def got_bytes(row):
    return (row or {}).get("progress", {}).get("downloaded", 0) or 0


def landed(row):
    path = pathlib.Path(row["path"]) if row and row.get("path") else None
    if path and path.is_dir():
        files = [f for f in path.rglob("*") if f.is_file()]
        path = max(files, key=lambda f: f.stat().st_size) if files else None
    return path if path and path.exists() else None


def accounting(row, path):
    """CORE-15: the row's own numbers agree with the file on disk."""
    p = row.get("progress", {})
    size = path.stat().st_size
    if p.get("total") not in (None, size):
        return f"row says {p['total']} bytes in all, file has {size}"
    if p.get("downloaded") not in (None, size):
        return f"row says {p['downloaded']} bytes fetched, file has {size}"
    return ""


def nothing_saved(dm, before):
    after = {f for f in dm.downloads.rglob("*") if f.is_file()}
    return sorted(f.name for f in after - before)


# ------------------------------------------------------------ media checks


FFMPEG = os.environ.get("DM_FFMPEG") or shutil.which("ffmpeg")


def media(path):
    """(whole, playable text, tracks summary, info)."""
    info = media_check.inspect(str(path))
    if info["container"] is None and path.suffix.lower() in (".ogv", ".ogg", ".ts", ".mp3"):
        info = {"container": path.suffix[1:], "duration": None, "tracks": [], "problems": [], "unread": True}
    whole = not info["problems"]
    playable = "No" if not whole else ("Yes (structure whole)" if not info.get("unread") else "Not read (no reader for this container)")
    if whole and FFMPEG:
        run = subprocess.run([FFMPEG, "-v", "error", "-xerror", "-i", str(path), "-f", "null", "-"],
                             capture_output=True, text=True, timeout=900)
        playable = "Yes (decoded by ffmpeg, no error)" if run.returncode == 0 else f"No (ffmpeg: {run.stderr.strip()[:120]})"
        whole = run.returncode == 0
    summary = "" if info.get("unread") else media_check.summary(info)
    return whole, playable, summary, info


def kinds(info):
    return {t["kind"] for t in info.get("tracks", [])}


def heights(info):
    return [t["height"] for t in info.get("tracks", []) if t["kind"] == "video" and t.get("height")]


# ------------------------------------------------------------ the local server


BLOCK = 64 * 1024
BLOCK_CACHE = {}


def block(k):
    """Block k of every served file: its own index, over and over, so a byte in the wrong place shows."""
    b = BLOCK_CACHE.get(k)
    if b is None:
        b = struct.pack("<Q", k) * (BLOCK // 8)
        if len(BLOCK_CACHE) < 4096:
            BLOCK_CACHE[k] = b
    return b


def content(start, length, piece):
    pos, end = start, start + length
    while pos < end:
        k, off = divmod(pos, BLOCK)
        b = block(k)[off:off + min(BLOCK - off, end - pos)]
        for i in range(0, len(b), piece):
            yield b[i:i + piece]
        pos += len(b)


def verify_generated(path, size):
    if not path or not path.is_file():
        return False, "no file"
    got = path.stat().st_size
    if got != size:
        return False, f"{got} bytes on disk, {size} served"
    with open(path, "rb") as f:
        k = 0
        while True:
            data = f.read(BLOCK * 64)
            if not data:
                break
            for i in range(0, len(data), BLOCK):
                piece = data[i:i + BLOCK]
                if piece != block(k)[:len(piece)]:
                    return False, f"wrong bytes in the block at {k * BLOCK}"
                k += 1
    return True, "byte for byte as served"


class Dropped(Exception):
    pass


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        self.answer(True)

    def do_GET(self):
        self.answer(False)

    def reset(self):
        """A connection cut as a network cuts it: a reset, no clean end."""
        try:
            self.connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER,
                                       struct.pack("HH" if sys.platform == "win32" else "ii", 1, 0))
        except OSError:
            pass
        self.close_connection = True
        raise Dropped()

    def plain(self, code, body=b"", headers=()):
        self.send_response(code)
        for k, v in headers:
            self.send_header(k, v)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def answer(self, head):
        s = self.server
        if s.down:
            self.reset()
        parts = urllib.parse.urlsplit(self.path).path.strip("/").split("/")
        if parts[0] == "r" and len(parts) == 3:
            n = int(parts[1])
            return self.plain(302, headers=[("Location", f"/r/{n - 1}/{parts[2]}" if n > 1 else f"/f/{parts[2]}")])
        if parts[0] == "s" and len(parts) == 3:
            return self.plain(int(parts[1]), b"not for you")
        case = s.cases.get(parts[1]) if parts[0] == "f" and len(parts) == 2 else None
        if not case:
            return self.plain(404, b"no such file")
        with s.lock:
            case["n"] += 1
            n = case["n"]
            s.log.append({"t": time.time(), "case": parts[1], "method": self.command,
                          "range": self.headers.get("Range", ""), "n": n,
                          "agent": self.headers.get("User-Agent", "")})
        early = case.get("status_first", [])
        if n <= len(early):
            return self.plain(early[n - 1], b"slow down", [("Retry-After", case.get("retry_after", "2"))])
        # However often it is asked, a case serves no more than this in all: a
        # program that does not check the disk's room is not let fill it.
        if case.get("total_cap") and case["sent"] >= case["total_cap"]:
            return self.plain(503, b"served all this case will serve", [("Retry-After", "3600")])
        if case.get("expires") and time.time() > case["expires"]:
            return self.plain(403, b"this address has expired")
        size = case["size"]
        start, end, status = 0, size - 1, 200
        rng = self.headers.get("Range", "")
        if rng.startswith("bytes=") and case.get("ranges", True):
            a, _, b = rng[6:].split(",")[0].partition("-")
            if a:
                start, end = int(a), min(int(b) if b else size - 1, size - 1)
            else:
                start = max(0, size - int(b))
            if start >= size:
                return self.plain(416, headers=[("Content-Range", f"bytes */{size}")])
            status = 206
        length = end - start + 1
        self.send_response(status)
        self.send_header("Content-Type", case.get("type", "application/octet-stream"))
        self.send_header("Content-Length", str(length))
        if case.get("ranges", True):
            self.send_header("Accept-Ranges", "bytes")
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("ETag", f'"{parts[1]}-{size}"')
        self.send_header("Last-Modified", "Mon, 02 Nov 2026 10:00:00 GMT")
        self.end_headers()
        if head:
            return
        rate = case.get("rate", 0)
        drop = n <= len(early) + case.get("drop_first", 0)
        sent = 0
        with s.lock:
            case["active"] += 1
            case["max_active"] = max(case["max_active"], case["active"])
        try:
            for chunk in content(start, length, 32 * 1024 if rate else BLOCK):
                if s.down or (drop and sent >= case.get("drop_after", MB)) or sent >= case.get("cap", 1 << 62):
                    self.reset()
                self.wfile.write(chunk)
                sent += len(chunk)
                with s.lock:
                    case["sent"] += len(chunk)
                if rate:
                    time.sleep(len(chunk) / rate)
        finally:
            with s.lock:
                case["active"] -= 1


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self):
        super().__init__(("127.0.0.1", 0), Handler)
        self.lock = threading.Lock()
        self.cases, self.log, self.down = {}, [], False
        threading.Thread(target=self.serve_forever, daemon=True).start()
        self.origin = f"http://127.0.0.1:{self.server_address[1]}"

    def handle_error(self, request, client_address):
        pass  # a reset, a client that went: both are what the tests do

    def file(self, name, size, **options):
        self.cases[name] = {"size": size, "n": 0, "sent": 0, "active": 0, "max_active": 0, **options}
        return f"{self.origin}/f/{name}"

    def requests(self, name, since=0):
        with self.lock:
            return [r for r in self.log if r["case"] == name and r["t"] >= since]


# ------------------------------------------------------------ phase: engine


def engine_phase(work):
    print("\n== engine: CORE and ENG cases against a local server, dm headless ==", flush=True)
    server = Server()
    dm = Dm(work / "engine", connections=8, retry_wait=0).start()
    try:
        def whole_file(test_id, url, size, row, extra_notes="", problem=""):
            """The file is every byte served, the row's numbers agree, and `problem` (a check of the
            case's own, judged before this is printed) found nothing."""
            kind, reason = state(row)
            path = landed(row)
            if kind != "completed" or not path:
                return record(test_id, "FAIL", url, notes="; ".join(filter(None, [f"ended {kind}", extra_notes])), log=reason)
            ok, why = verify_generated(path, size)
            wrong = accounting(row, path)
            return record(test_id, "PASS" if ok and not wrong and not problem else "FAIL", url, size=path.stat().st_size,
                          notes="; ".join(filter(None, [why, wrong, extra_notes, problem])), log=reason)

        def guarded(test_id, fn):
            try:
                fn()
            except Exception as e:  # one case breaking must not stop the others
                record(test_id, "FAIL", notes=f"the check itself failed: {e!r}")

        # ENG-02: ranges, several connections.
        def eng02():
            size = 48 * MB
            url = server.file("ranged.bin", size, rate=4 * MB)
            row = dm.settle(dm.add(url), 180)
            asked = [r for r in server.requests("ranged.bin") if r["method"] == "GET" and r["range"]]
            starts = {r["range"] for r in asked}
            whole_file("ENG-02", url, size, row,
                       f"{len(starts)} distinct ranges, at most {server.cases['ranged.bin']['max_active']} at once",
                       "only one connection on a server that takes ranges" if len(starts) < 2 else "")
        guarded("ENG-02", eng02)

        def eng03():
            size = 24 * MB
            url = server.file("plain.bin", size, ranges=False, rate=8 * MB)
            row = dm.settle(dm.add(url), 180)
            whole_file("ENG-03", url, size, row, f"{len(server.requests('plain.bin'))} requests")
        guarded("ENG-03", eng03)

        def eng01():
            size = 6 * MB
            server.file("redirected.bin", size)
            url = f"{server.origin}/r/3/redirected.bin"
            row = dm.settle(dm.add(url), 120)
            whole_file("ENG-01", url, size, row, f"saved as {(row or {}).get('filename')}")
        guarded("ENG-01", eng01)

        # CORE-08: pause and resume.
        def core08():
            size = 40 * MB
            url = server.file("paused.bin", size, rate=512 * 1024)
            ident = dm.add(url)
            wait(lambda: got_bytes(dm.row(ident)) > size // 5, 90)
            dm.call("downloads.pause", ids=[ident])
            paused = wait(lambda: state(dm.row(ident))[0] == "paused", 15)
            time.sleep(1.5)  # what was already in the sockets' buffers when the pause came
            at_pause, sent_pause = got_bytes(dm.row(ident)), server.cases["paused.bin"]["sent"]
            time.sleep(4)
            still = server.cases["paused.bin"]["sent"] - sent_pause
            mark = time.time()
            dm.call("downloads.resume", ids=[ident])
            row = dm.settle(ident, 240)
            after = sum(1 for r in server.requests("paused.bin", mark) if r["method"] == "GET")
            refetched = server.cases["paused.bin"]["sent"] - sent_pause - still
            whole_file("CORE-08", url, size, row,
                       f"paused at {at_pause / MB:.1f} MB; {still} bytes sent while paused; "
                       f"{refetched / MB:.1f} MB fetched after Resume in {after} requests",
                       "never paused" if not paused else "kept fetching while paused" if still > 256 * 1024 else
                       "started again from the beginning" if refetched > size * 0.95 else "")
        guarded("CORE-08", core08)

        # CORE-09: Exit, then a kill, mid-download; dm started again on the same home.
        def core09(test_id, how):
            nonlocal dm
            size = 40 * MB
            name = f"restart-{how}.bin"
            url = server.file(name, size, rate=512 * 1024)
            ident = dm.add(url)
            wait(lambda: got_bytes(dm.row(ident)) > size // 4, 90)
            at_stop = got_bytes(dm.row(ident))
            if how == "exit":
                dm.stop(graceful=True)
            else:
                dm.kill()
            sent_stop = server.cases[name]["sent"]
            # The same home and queue: only the process is new.
            dm = Dm(work / "engine", connections=8, retry_wait=0).start()
            row = wait(lambda: dm.row(ident), 20)
            listed = state(row)
            time.sleep(4)
            resumed_by = "by itself"
            if state(dm.row(ident))[0] in ("paused", "failed", "queued"):
                dm.call("downloads.resume", ids=[ident])
                resumed_by = "after Resume"
            row = dm.settle(ident, 240)
            refetched = server.cases[name]["sent"] - sent_stop
            whole_file(test_id, url, size, row,
                       f"stopped at {at_stop / MB:.1f} MB; listed again as {listed[0]}; went on {resumed_by}; "
                       f"{refetched / MB:.1f} MB fetched after the start"
                       + (" (from the beginning: the part was not kept)" if refetched > size * 0.95 else ""))
        guarded("CORE-09a", lambda: core09("CORE-09a", "exit"))
        guarded("CORE-09b", lambda: core09("CORE-09b", "kill"))

        def core10():
            size = 16 * MB
            url = server.file("dropped.bin", size, drop_first=3, drop_after=MB)
            row = dm.settle(dm.add(url), 180)
            whole_file("CORE-10", url, size, row, f"{len(server.requests('dropped.bin'))} requests, the first 3 cut at 1 MB")
        guarded("CORE-10", core10)

        def eng05():
            size = 8 * MB
            url = server.file("limited.bin", size, status_first=[429, 429, 503], retry_after="2")
            row = dm.settle(dm.add(url), 240)
            log = server.requests("limited.bin")
            t0 = log[0]["t"] if log else 0
            timeline = ", ".join(f"+{r['t'] - t0:.1f}s {r['range'] or 'no Range'}" for r in log[:5])
            # The same request asked again: a 429 or 503 answered by asking again without Range at
            # once (dm's fallback for a server that refuses ranges) is a different request, and so
            # is one in another name: a 429 to a browser's name is asked once more at once in dm's
            # own (`probe.rs`, for servers that limit browsers' names from a program).
            again = [next((b for b in log[i + 1:] if (b["range"], b["agent"]) == (a["range"], a["agent"])), None)
                     for i, a in enumerate(log[:3])]
            too_soon = [b["t"] - a["t"] for a, b in zip(log, again) if b and b["t"] - a["t"] < 1.5]
            whole_file("ENG-05", url, size, row, f"requests: {timeline} (Retry-After: 2)",
                       f"the same request again {min(too_soon):.1f} s after a 429/503" if too_soon else "")
        guarded("ENG-05", eng05)

        def eng06():
            size = 32 * MB
            url = server.file("outage.bin", size, rate=512 * 1024)
            ident = dm.add(url)
            wait(lambda: got_bytes(dm.row(ident)) > size // 4, 90)
            server.down = True
            time.sleep(10)
            during = state(dm.row(ident))
            server.down = False
            row = dm.settle(ident, 180)
            how = "by itself"
            if state(row)[0] == "failed":
                dm.call("downloads.resume", ids=[ident])
                row = dm.settle(ident, 180)
                how = "after Resume"
            whole_file("ENG-06", url, size, row, f"during the outage dm said {during[0]} {during[1][:80]!r}; recovered {how}")
        guarded("ENG-06", eng06)

        def eng04():
            size = 24 * MB
            url = server.file("expiring.bin", size, rate=256 * 1024)
            ident = dm.add(url)
            wait(lambda: got_bytes(dm.row(ident)) > 2 * MB, 60)
            dm.call("downloads.pause", ids=[ident])
            wait(lambda: state(dm.row(ident))[0] == "paused", 15)
            server.cases["expiring.bin"]["expires"] = time.time()
            time.sleep(1)
            dm.call("downloads.resume", ids=[ident])
            row = dm.settle(ident, 240)
            kind, reason = state(row)
            if kind == "completed":
                record("ENG-04", "FAIL", url, notes="completed from an address that answers 403", log=reason)
            elif reason:
                record("ENG-04", "PASS", url, notes=f"ended {kind}; is the reason clear? (a person reads it)", log=reason)
            else:
                record("ENG-04", "FAIL", url, notes=f"still {kind} after 4 minutes, with no reason given", log=reason)
        guarded("ENG-04", eng04)

        def refusals():
            before = {f for f in dm.downloads.rglob("*") if f.is_file()}
            first = len(RESULTS)
            # CORE-11a: not an address at all.
            try:
                ident = dm.add("this is not a link")
                row = dm.settle(ident, 60)
                kind, reason = state(row)
                record("CORE-11a", "PASS" if kind == "failed" and reason else "FAIL", "this is not a link",
                       notes=f"added, then {kind}", log=reason)
            except DmError as e:
                record("CORE-11a", "PASS", "this is not a link", notes="refused when added", log=str(e))
            cases = [("CORE-11b", "http://no-such-host.invalid/video.mp4"),
                     ("CORE-12a", f"{server.origin}/s/404/deleted.mp4"),
                     ("CORE-12b", f"{server.origin}/s/410/removed.mp4"),
                     ("CORE-13a", f"{server.origin}/s/401/private.mp4"),
                     ("CORE-13b", f"{server.origin}/s/403/members-only.mp4")]
            idents, added = {}, {}
            for test_id, url in cases:
                try:
                    idents[test_id] = dm.add(url)
                    added[test_id] = time.monotonic()
                except DmError as e:
                    record(test_id, "PASS", url, notes="refused when added", log=str(e))
            # All watched at once, each timed from its own add. What the window says of a row
            # (`ui.inspect`'s status: "Server not found (retry 6 of 15)") is read beside the row,
            # since a retried row is "running" to the control port until its tries run out.
            settled, said = {}, {}
            while len(settled) < len(idents) and time.monotonic() - min(added.values(), default=0) < 300:
                shown = {r.get("id"): r.get("status", "") for r in (dm.call("ui.inspect") or {}).get("rows", [])}
                for test_id, ident in idents.items():
                    row = dm.row(ident)
                    words = shown.get(ident, "")
                    if test_id not in said and words and not words.startswith(("Starting", "Queued")) and "%" not in words:
                        said[test_id] = (words, time.monotonic() - added[test_id])
                    if test_id not in settled and row and (state(row)[0] in ("failed", "cancelled", "completed") or held(row)):
                        settled[test_id] = (row, time.monotonic() - added[test_id])
                time.sleep(1)
            for test_id, url in cases:
                if test_id not in idents:
                    continue
                row, took = settled.get(test_id, (dm.row(idents[test_id]), None))
                kind, reason = state(row)
                ok = took is not None and kind != "completed" and bool(reason)
                window = f"the window said {said[test_id][0]!r} after {said[test_id][1]:.0f} s; " if test_id in said else ""
                record(test_id, "PASS" if ok else "FAIL", url,
                       notes=window + (f"{kind} after {took:.0f} s" if ok else
                                       f"still {kind} after 5 minutes" if took is None else f"{kind}, no reason"),
                       log=reason)
            left = nothing_saved(dm, before)
            if left:
                for r in RESULTS[first:]:
                    r["result"], r["notes"] = "FAIL", r["notes"] + f"; left in the folder: {left}"
        guarded("CORE-11", refusals)

        def core14():
            # 64 TiB: no more than 16 MB a response and 64 MB in all is ever sent.
            url = server.file("enormous.bin", 64 * 1024 ** 4, cap=16 * MB, total_cap=64 * MB)
            ident = dm.add(url)
            row = dm.settle(ident, 60)
            kind, reason = state(row)
            sent = server.cases["enormous.bin"]["sent"]
            if kind != "failed":
                dm.call("downloads.cancel", ids=[ident])
            ok = kind in ("failed", "paused") and any(w in reason.lower() for w in ("room", "space", "disk", "full"))
            record("CORE-14", "PASS" if ok else "FAIL", url,
                   notes=f"{kind}; {sent / MB:.1f} MB fetched before that" + ("" if ok else "; no word of the disk"),
                   log=reason)
        guarded("CORE-14", core14)

        def eng07a():
            size = 4 * MB
            url = server.file("nowhere.bin", size)
            blocker = dm.downloads / "not-a-folder"
            blocker.write_text("a file where the download's folder should be")
            try:
                ident = dm.add(url, folder=str(blocker))
            except DmError as e:
                return record("ENG-07a", "PASS", url, notes="refused when added", log=str(e))
            row = dm.settle(ident, 120)
            kind, reason = state(row)
            path = landed(row)
            if kind == "completed":
                return record("ENG-07a", "FAIL" if not path or blocker in path.parents else "PASS", url,
                              notes=f"completed at {path}" if path else "completed with no file", log=reason)
            record("ENG-07a", "PASS" if kind in ("failed", "paused") and reason else "FAIL", url,
                   notes=f"ended {kind}, saying why", log=reason)
            dm.call("downloads.cancel", ids=[ident])
        guarded("ENG-07a", eng07a)

        removable = os.environ.get("DM_TEST_REMOVABLE")
        if removable and sys.stdin.isatty():
            def eng07b():
                folder = pathlib.Path(removable) / "dm-video-sites-check"
                other = Dm(work / "removable", download_dir=str(folder)).start()
                try:
                    size = 200 * MB
                    url = server.file("pulled.bin", size, rate=MB)
                    ident = other.add(url)
                    wait(lambda: got_bytes(other.row(ident)) > 10 * MB, 60)
                    input(f"  ENG-07b: pull the drive {removable} out now, then press Enter ")
                    row = other.settle(ident, 120)
                    kind, reason = state(row)
                    record("ENG-07b", "PASS" if kind == "failed" and reason else "FAIL", url,
                           notes=f"ended {kind}", log=reason)
                finally:
                    other.stop()
            guarded("ENG-07b", eng07b)
        else:
            record("ENG-07b", "BLOCKED", notes="needs a removable drive: set DM_TEST_REMOVABLE and run in a terminal")

        def eng08():
            gib = float(os.environ.get("DM_TEST_LARGE_GB", "4.5"))
            if gib <= 0:
                return record("ENG-08", "N/A", notes="skipped (DM_TEST_LARGE_GB=0)")
            size = int(gib * 1024 ** 3)
            free = shutil.disk_usage(dm.downloads).free
            if free < size * 1.2 + 1024 ** 3:
                return record("ENG-08", "BLOCKED", notes=f"needs {size * 1.2 / 1024 ** 3:.1f} GiB free, "
                                                         f"{free / 1024 ** 3:.1f} GiB is (DM_VIDEO_WORK on a bigger disk)")
            url = server.file("large.bin", size)
            ident = dm.add(url)
            seen, problems, started = [], [], time.monotonic()
            while time.monotonic() - started < 1800:
                row = dm.row(ident)
                kind, _ = state(row)
                p = (row or {}).get("progress", {})
                now = p.get("downloaded") or 0
                if seen and now < seen[-1] and kind == "running":
                    problems.append(f"progress went back from {seen[-1]} to {now}")
                if p.get("total") not in (None, size):
                    problems.append(f"total said {p.get('total')}")
                if now > size:
                    problems.append(f"fetched {now} of {size}")
                seen.append(now)
                if kind in ("completed", "failed", "cancelled"):
                    break
                time.sleep(1)
            took = time.monotonic() - started
            row = dm.row(ident)
            whole_file("ENG-08", url, size, row, f"{size / 1024 ** 3:.2f} GiB in {took:.0f} s"
                       + ("" if problems else "; progress only went up"),
                       "; ".join(sorted(set(problems))[:3]))
            path = landed(row)
            if path:
                path.unlink(missing_ok=True)
        guarded("ENG-08", eng08)
    finally:
        dm.stop()
        server.shutdown()


# ------------------------------------------------------------ phase: public, Add URL


# Certificates are checked, but not with Python 3.13's strict extra rules, which refuse some
# real chains (a CA without a critical Basic Constraints) that browsers and dm accept.
TLS = ssl.create_default_context()
TLS.verify_flags &= ~getattr(ssl, "VERIFY_X509_STRICT", 0)


def open_url(url, method="GET", timeout=60):
    return urllib.request.urlopen(urllib.request.Request(url, method=method, headers={"User-Agent": AGENT}),
                                  timeout=timeout, context=TLS)


def fetch_json(url):
    with open_url(url) as r:
        return json.load(r)


def quoted(url):
    """An address as a server reads it: spaces and the like in its path escaped."""
    parts = urllib.parse.urlsplit(url)
    return urllib.parse.urlunsplit(parts._replace(path=urllib.parse.quote(urllib.parse.unquote(parts.path), safe="/~@:+")))


def head_size(url):
    try:
        with open_url(url, "HEAD") as r:
            return int(r.headers.get("Content-Length") or 0) or None
    except (OSError, ValueError):
        return None


def digest(path, algorithm):
    h = hashlib.new(algorithm)
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(MB), b""):
            h.update(chunk)
    return h.hexdigest()


YT_SHORT = "https://www.youtube.com/watch?v=jNQXAC9IVRw"     # "Me at the zoo", 19 s
YT_HEIGHTS = os.environ.get("DM_YT_QUALITY_URL", "https://www.youtube.com/watch?v=aqz-KE-bpKQ")  # Big Buck Bunny, CC-BY
YT_MISSING = "https://www.youtube.com/watch?v=Zz0Zz0Zz0Zz"
VIMEO_PRIVATE = "https://vimeo.com/76979871"
MUX_HLS = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8"
SHAKA_DASH = "https://storage.googleapis.com/shaka-demo-assets/angel-one/dash.mpd"
TEST_VIDEOS = [("DIR-01", "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/360/Big_Buck_Bunny_360_10s_1MB.mp4"),
               ("DIR-02", "https://test-videos.co.uk/vids/bigbuckbunny/webm/vp9/360/Big_Buck_Bunny_360_10s_1MB.webm")]
ARCHIVE_ITEM = os.environ.get("DM_ARCHIVE_ITEM", "Popeye_forPresident")


def archive_cases():
    meta = fetch_json(f"https://archive.org/metadata/{ARCHIVE_ITEM}")
    files = [f for f in meta.get("files", []) if f.get("md5") and int(f.get("size") or 0) < 300 * MB]
    pick = lambda *names: next((f for f in files if any(n.lower() in (f.get("format") or "").lower() for n in names)), None)
    out = []
    for test_id, f in (("PUB-10a", pick("MPEG4", "h.264")), ("PUB-10b", pick("Ogg Video", "WebM", "Matroska"))):
        if f:
            url = f"https://archive.org/download/{ARCHIVE_ITEM}/{urllib.parse.quote(f['name'])}"
            out.append((test_id, url, {"md5": f["md5"], "size": int(f["size"]), "format": f.get("format")}))
    return out


def wikimedia_cases():
    api = ("https://commons.wikimedia.org/w/api.php?action=query&format=json&generator=search&gsrnamespace=6"
           "&gsrsearch=" + urllib.parse.quote("filetype:video filesize:1000,40000 nasa")
           + "&gsrlimit=10&prop=videoinfo&viprop=url|size|sha1|mime|derivatives")
    pages = sorted(fetch_json(api).get("query", {}).get("pages", {}).values(), key=lambda p: p.get("index", 0))
    for page in pages:
        info = (page.get("videoinfo") or [{}])[0]
        if info.get("url") and info.get("sha1") and (info.get("mime") or "").startswith("video/"):
            original = info["url"].split("?")[0]
            derived = sorted((d for d in info.get("derivatives", []) if d.get("src") and d["src"].split("?")[0] != original
                              and ("360p" in d["src"] or "480p" in d["src"])),
                             key=lambda d: (not d["src"].endswith(".webm"), d["src"]))
            out = [("PUB-11a", original, {"sha1": info["sha1"], "size": info.get("size"), "format": info.get("mime"),
                                          "title": page.get("title")})]
            if derived:
                out.append(("PUB-11b", derived[0]["src"], {"format": derived[0].get("type"), "title": page.get("title")}))
            return out
    return []


def nasa_cases():
    found = fetch_json("https://images-api.nasa.gov/search?q=" + urllib.parse.quote("apollo 11")
                       + "&media_type=video&page_size=10")
    limit = float(os.environ.get("DM_NASA_MAX_MB", "400")) * MB
    ids = [i.get("data", [{}])[0].get("nasa_id") for i in found.get("collection", {}).get("items", [])]
    # Plain ids first: some items' names hold characters their own files are not served under.
    for nasa_id in sorted(filter(None, ids), key=lambda n: (not n.isascii() or " " in n, n)):
        assets = [quoted(i["href"].replace("http://", "https://")) for i in
                  fetch_json(f"https://images-api.nasa.gov/asset/{urllib.parse.quote(nasa_id)}")["collection"]["items"]]
        small = next((a for a in assets if a.endswith(("~mobile.mp4", "~small.mp4"))), None)
        orig = next((a for a in assets if a.endswith("~orig.mp4")), None)
        if not small or not orig:
            continue
        orig_size, small_size = head_size(orig), head_size(small)
        if not orig_size or not small_size or orig_size > limit:
            continue
        return [("PUB-12a", small, {"size": small_size, "nasa_id": nasa_id}),
                ("PUB-12b", orig, {"size": orig_size, "nasa_id": nasa_id})]
    return []


def judge_public(test_id, url, row, expect, ref=None, quality="", note="", problem=""):
    """One public download, judged as the plan says; `problem`, a check of the case's own, fails it."""
    kind, reason = state(row)
    cookies = "Yes" if (row or {}).get("signed_in") else "No"
    fmt = f"stream_quality={quality}" if quality else ""
    if expect == "refused":
        path = landed(row)
        if kind == "completed":
            return record(test_id, "FAIL", url, fmt, notes=f"saved {path.name if path else '?'} as if it were the video",
                          log=reason, cookies_to_dm=cookies)
        reported = (kind == "failed" or held(row)) and bool(reason)
        return record(test_id, "PASS" if reported else "FAIL", url, fmt,
                      notes=f"{kind}, saying why" if reported else f"still {kind} with no word of why",
                      log=reason, cookies_to_dm=cookies)
    path = landed(row) if kind == "completed" else None
    if not path:
        low = reason.lower()
        verdict = ("UNSUPPORTED" if expect.get("page") else
                   "BLOCKED" if any(w in low for w in ("sign in", "sign-in", "login", "bot", "captcha", "429", "too many",
                                                       "not available in your country", "region")) else "FAIL")
        if any(w in low for w in ("drm", "protected", "widevine", "playready", "fairplay")):
            verdict = "UNSUPPORTED"
        return record(test_id, verdict, url, fmt, notes=f"ended {kind}", log=reason, cookies_to_dm=cookies)
    size = path.stat().st_size
    problems = []
    # A joined or rebuilt stream's file is not the bytes fetched, so only a plain file is held to this.
    wrong = accounting(row, path) if ref else ""
    if wrong:
        problems.append(wrong)
    if cookies == "Yes":
        problems.append("the row holds cookies in a signed-out run")
    if ref and ref.get("md5") and digest(path, "md5") != ref["md5"]:
        problems.append("MD5 differs from the item's")
    if ref and ref.get("sha1") and digest(path, "sha1") != ref["sha1"]:
        problems.append("SHA-1 differs from the API's")
    if ref and ref.get("size") and size != ref["size"]:
        problems.append(f"{size} bytes, the source says {ref['size']}")
    if ref and ref.get("bytes_of"):
        try:
            with open_url(ref["bytes_of"], timeout=120) as r:
                theirs = hashlib.sha256(r.read()).hexdigest()
            if theirs != digest(path, "sha256"):
                problems.append("differs from a plain fetch of the same address")
        except OSError as e:
            problems.append(f"no reference copy to compare with ({e})")
    whole, playable, summary, info = media(path)
    if not whole:
        problems.append(f"not whole: {'; '.join(info['problems']) or playable}")
    tracks = kinds(info)
    need = expect.get("tracks", set())
    if need and not info.get("unread") and not need <= tracks:
        problems.append(f"has {sorted(tracks) or 'no tracks'}, wanted {sorted(need)}")
    if expect.get("only") and tracks and tracks != expect["only"]:
        problems.append(f"has {sorted(tracks)}, wanted only {sorted(expect['only'])}")
    if expect.get("height") and heights(info) and heights(info)[0] != expect["height"]:
        problems.append(f"{heights(info)[0]} tall, {expect['height']} was chosen")
    if problem:
        problems.append(problem)
    checks = [c for c in ("md5", "sha1") if ref and ref.get(c)]
    said = f"{path.name}; {summary}" + (f"; {'/'.join(c.upper() for c in checks)} match" if checks and not problems else "")
    return record(test_id, "FAIL" if problems else "PASS", url, fmt or summary, size=size, playable=playable,
                  notes="; ".join(filter(None, [said, note] + problems)), log=reason, cookies_to_dm=cookies)


def public_url_phase(work):
    print("\n== public-url: real public sites by Add URL, dm headless, signed in nowhere ==", flush=True)
    found = {}
    for name, finder in (("archive", archive_cases), ("wikimedia", wikimedia_cases), ("nasa", nasa_cases)):
        try:
            found[name] = finder()
        except (OSError, ValueError, KeyError) as e:
            found[name] = []
            print(f"  could not ask {name}'s API: {e}", flush=True)
    for name, ids in (("archive", ["PUB-10a", "PUB-10b"]), ("wikimedia", ["PUB-11a", "PUB-11b"]), ("nasa", ["PUB-12a", "PUB-12b"])):
        have = {c[0] for c in found[name]}
        for test_id in ids:
            if test_id not in have:
                record(test_id, "BLOCKED", notes=f"{name}'s API named no suitable file today")

    # Each group is one dm, its stream_quality set for all of it.
    groups = [
        ("best", [("PUB-01a", YT_SHORT, {"tracks": {"video", "audio"}}, None),
                  ("CORE-12c", YT_MISSING, "refused", None),
                  ("CORE-13c", VIMEO_PRIVATE, "refused", None),
                  ("PUB-10c", f"https://archive.org/details/{ARCHIVE_ITEM}", {"tracks": {"video"}, "page": True}, None)]
         + [(i, u, {"tracks": {"video"}}, ref) for i, u, ref in found["archive"]]
         + [(i, u, {"tracks": {"video"}}, ref if i == "PUB-11a" else None) for i, u, ref in found["wikimedia"]]
         + [(i, u, {"tracks": {"video"}}, ref) for i, u, ref in found["nasa"]]
         + [(i, u, {"tracks": {"video"}}, {"bytes_of": u}) for i, u in TEST_VIDEOS]
         + [("CORE-05b", SHAKA_DASH, {"tracks": {"video", "audio"}}, None)]),
        ("360", [("PUB-01b", YT_HEIGHTS, {"tracks": {"video", "audio"}, "height": 360}, None),
                 ("CORE-06", MUX_HLS, {"tracks": {"video"}}, None)]),
        ("480", [("PUB-01c", YT_HEIGHTS, {"tracks": {"video", "audio"}, "height": 480}, None)]),
        ("audio", [("PUB-01d", YT_SHORT, {"tracks": {"audio"}, "only": {"audio"}}, None)]),
    ]
    for quality, cases in groups:
        dm = Dm(work / f"public-{quality}", stream_quality=quality).start()
        try:
            idents = {}
            for test_id, url, expect, ref in cases:
                try:
                    idents[test_id] = dm.add(url)
                except DmError as e:
                    if expect == "refused":
                        record(test_id, "PASS", url, notes="refused when added", log=str(e))
                    else:
                        record(test_id, "UNSUPPORTED" if expect.get("page") else "FAIL", url,
                               notes="refused when added", log=str(e))
            for test_id, url, expect, ref in cases:
                if test_id in idents:
                    row = dm.settle(idents[test_id], 1800)
                    judge_public(test_id, url, row, expect, ref, quality)
            if quality == "360":
                hls_pause_resume(dm)
        finally:
            dm.stop()


def hls_pause_resume(dm):
    """CORE-08b: a stream, slowed so there is time to pause it, paused, then resumed."""
    try:
        dm.call("downloads.speed_limit", kb=400)
        ident = dm.add(MUX_HLS)
        wait(lambda: got_bytes(dm.row(ident)) > 2 * MB, 180)
        dm.call("downloads.pause", ids=[ident])
        paused = wait(lambda: state(dm.row(ident))[0] == "paused", 20)
        at = got_bytes(dm.row(ident))
        time.sleep(4)
        moved = got_bytes(dm.row(ident)) - at
        dm.call("downloads.speed_limit", kb=0)
        dm.call("downloads.resume", ids=[ident])
        row = dm.settle(ident, 1800)
        judge_public("CORE-08b", MUX_HLS, row, {"tracks": {"video"}}, None, "360",
                     note=f"paused at {at / MB:.1f} MB, {moved} bytes counted while paused",
                     problem="never paused" if not paused else "went on while paused" if moved > 256 * 1024 else "")
    except Exception as e:
        record("CORE-08b", "FAIL", MUX_HLS, notes=f"the check itself failed: {e!r}")


# ------------------------------------------------------------ phase: public, the browser panel


def edge_version():
    for base in (r"C:\Program Files (x86)\Microsoft\Edge\Application", r"C:\Program Files\Microsoft\Edge\Application"):
        p = pathlib.Path(base)
        if p.is_dir():
            versions = sorted((d.name for d in p.iterdir() if d.is_dir() and d.name[:1].isdigit()),
                              key=lambda v: [int(x) for x in v.split(".") if x.isdigit()])
            if versions:
                return f"Microsoft Edge {versions[-1]}"
    return "Microsoft Edge (version not read)" if shutil.which("msedge") else "Chromium (see sites-check's output)"


def extension_version():
    try:
        return json.loads((ROOT / "dist" / "chrome" / "manifest.json").read_text())["version"]
    except (OSError, ValueError, KeyError):
        return "?"


def resolve_panel_urls():
    """Exact pages for the sites whose own APIs name one, so the report can record them."""
    urls = {}
    try:
        videos = fetch_json("https://api.dailymotion.com/videos?fields=id,url,duration,private&limit=20&sort=trending")
        v = next((v for v in videos.get("list", []) if not v.get("private") and 20 < (v.get("duration") or 0) < 1200), None)
        if v:
            urls["dailymotion"] = v["url"]
    except (OSError, ValueError):
        pass
    try:
        listed = fetch_json("https://video.blender.org/api/v1/videos?count=25&sort=-publishedAt&isLocal=true")
        v = next((v for v in listed.get("data", []) if not v.get("isLive") and 20 < (v.get("duration") or 0) < 1500), None)
        if v:
            urls["peertube"] = f"https://video.blender.org/w/{v['shortUUID']}"
    except (OSError, ValueError, KeyError):
        pass
    return urls


def run_sites_check(names, env_extra, work):
    env = {k: v for k, v in os.environ.items()
           if k not in ("DM_CHROME_PROFILE", "SITES_SIGNIN", "DM_KEEP_OTHER_EXTENSIONS", "SITES_HOLD")}
    env.update({"DM_SKIP_LOCAL": "1", "DM_BINARY": str(BINARY), "PYTHONUNBUFFERED": "1",
                "SITES_FINISH_WAIT": os.environ.get("SITES_FINISH_WAIT", "600"),
                "TMP": str(work), "TEMP": str(work), "TMPDIR": str(work), **env_extra})
    out = []
    proc = subprocess.Popen([sys.executable, str(HERE / "sites-check.py"), *names], env=env, cwd=str(ROOT),
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
    for line in proc.stdout:
        if line.startswith("SITE "):
            try:
                out.append(json.loads(line[5:]))
            except ValueError:
                pass
        elif line.strip() and not line.startswith(("  ok", "SUMMARY")):
            print("    | " + line.rstrip()[:200], flush=True)
    proc.wait()
    return out, proc.returncode


def judge_panel(test_id, r, signed_in=False):
    result = r.get("result") or ""
    url = r.get("url") or r.get("found") or ""
    lines = r.get("lines") or []
    m = r.get("media") or {}
    fmt = lines[0] if lines else ("sent at once (one file)" if r.get("sent") else "")
    extra = {"cookies_to_dm": "browser session (signed in)" if signed_in else
             "No login; a fresh profile's own visitor cookies may go with the hand-over"}
    if signed_in:
        extra["signed_out"] = "No"
    words = json.dumps(r.get("panel_state") or r.get("panel_after") or "").lower()
    if result == "OK":
        problems = []
        want = next((int(w[:-1]) for w in fmt.split() if w.endswith("p") and w[:-1].isdigit()), None)
        if want and m.get("height") and m["height"] != want:
            problems.append(f"the line said {want}p, the file is {m['height']} tall")
        if len(set(lines)) != len(lines):
            problems.append("the list names one quality twice")
        return record(test_id, "FAIL" if problems else "PASS", url, fmt, size=r.get("size"),
                      playable=f"Yes (structure whole: {', '.join(m.get('kinds') or [])}, {m.get('seconds') or 0:.0f} s)",
                      notes="; ".join([f"{r.get('file')}; listed: {lines}" if lines else str(r.get("file"))] + problems), **extra)
    if any(w in words for w in ("drm", "protected", "widevine", "playready")):
        return record(test_id, "UNSUPPORTED", url, fmt, notes="a protected (DRM) stream, refused in words", log=words[:300], **extra)
    blocked = ("no public video post found", "no video player appeared", "not signed in within")
    verdict = "BLOCKED" if any(b in result for b in blocked) else "FAIL"
    detail = r.get("body") or r.get("title") or ""
    # A file that did not land: what dm's row said by then (sites-check's `dm_rows`).
    for row in r.get("dm_rows") or []:
        st, p = row.get("state") or {}, row.get("progress") or {}
        got = (f"{(p.get('downloaded') or 0) / MB:.1f} of {p['total'] / MB:.1f} MB" if p.get("total")
               else f"{(p.get('downloaded') or 0) / MB:.1f} MB")
        result += f"; dm's row: {st.get('kind')}, {got} {st.get('reason') or row.get('stalled') or ''}".rstrip()
        if st.get("kind") == "running" and p.get("downloaded"):
            verdict, result = "BLOCKED", result + " (still arriving when the wait ran out: not judged)"
    return record(test_id, verdict, url, fmt, notes=result, log=str(detail)[:300], **extra)


def public_panel_phase(work, only=()):
    print("\n== public-panel: video sites through the browser panel, a new profile signed in nowhere ==", flush=True)
    sites = {k: x for k, x in PANEL_SITES.items() if not only or k in only}
    if not (ROOT / "dist" / "chrome" / "pkg" / "dm_catcher_bg.wasm").exists():
        for test_id in sites.values():
            record(test_id, "BLOCKED", notes="no packaged extension: run sh scripts/package.sh")
        return
    urls = resolve_panel_urls()
    env = {f"SITES_URL_{k.upper().replace('-', '_')}": v for k, v in urls.items() if k in sites}
    results, _ = run_sites_check(list(sites), env, work)
    by_site = {r["site"]: r for r in results}
    for site, test_id in sites.items():
        r = by_site.get(site)
        if not r:
            record(test_id, "FAIL", urls.get(site, ""), notes="sites-check.py gave no result for this site")
        else:
            judge_panel(test_id, r)


# ------------------------------------------------------------ phase: optional, signed in


def auth_phase(work, sites):
    print("\n== optional authenticated suite: signed in BY HAND, not part of the release gate ==", flush=True)
    print("   The script creates no account and never types or keeps a password. Sign in with an account\n"
          "   of your own (a spare one made for testing is fine) when the Edge window opens.", flush=True)
    profiles = pathlib.Path(os.environ.get("DM_AUTH_PROFILES") or work / "auth-profiles")
    for name in sites:
        if name not in AUTH_SITES:
            print(f"  {name}: not a site this suite knows ({', '.join(AUTH_SITES)})", flush=True)
            continue
        test_id, pages = AUTH_SITES[name]
        profiles.mkdir(parents=True, exist_ok=True)
        env = {"SITES_SIGNIN": name, "DM_CHROME_PROFILE": str(profiles / name)}
        results, _ = run_sites_check(pages, env, work)
        for i, r in enumerate(results):
            judge_panel(test_id + (chr(ord("a") + i) if len(results) > 1 else ""), r, signed_in=True)
            RESULTS[-1].update({"website": name.capitalize(), "test": f"signed in: {r['site']}",
                                "priority": "optional", "phase": "auth"})
    for test_id, site in (("AUTH-01", "Google Drive"), ("AUTH-02", "Dropbox"), ("AUTH-03", "OneDrive"), ("AUTH-04", "MEGA")):
        record(test_id, "N/A", site=site, what="a private file of the tester's", priority="optional", phase="auth",
               notes="not automated: the tester's own share link, by hand, as docs/site-list.md says",
               signed_out="No")


# ------------------------------------------------------------ the report


def gate():
    p0 = [r for r in RESULTS if r["priority"] == "P0"]
    core = [r for r in RESULTS if r["priority"] == "core"]
    public = [r for r in RESULTS if r["phase"] in ("public-url", "public-panel")]
    count = lambda rows, res: sum(1 for r in rows if r["result"] == res)
    needing_cookies = sum(1 for r in public if r["cookies_to_dm"] == "Yes")
    needing_login = sum(1 for r in public if r["signed_out"] != "Yes")
    ok = (p0 and all(r["result"] == "PASS" for r in p0) and not any(r["result"] == "FAIL" for r in core)
          and needing_cookies == 0 and needing_login == 0)
    return {
        "P0 tests passed": f"{count(p0, 'PASS')} / {len(p0)}", "P0 tests failed": f"{count(p0, 'FAIL')} / {len(p0)}",
        "P0 tests blocked": f"{count(p0, 'BLOCKED')} / {len(p0)}",
        "Core and engine tests passed": f"{count(core, 'PASS')} / {len(core)} ({count(core, 'FAIL')} failed, "
                                        f"{count(core, 'BLOCKED')} blocked)",
        "P1 tests passed": "{} / {}".format(count([r for r in RESULTS if r["priority"] == "P1"], "PASS"),
                                            sum(1 for r in RESULTS if r["priority"] == "P1")),
        "P2 tests passed": "{} / {}".format(count([r for r in RESULTS if r["priority"] == "P2"], "PASS"),
                                            sum(1 for r in RESULTS if r["priority"] == "P2")),
        "Optional authenticated tests passed": "{} / {}".format(
            count([r for r in RESULTS if r["phase"] == "auth" and r["result"] != "N/A"], "PASS"),
            sum(1 for r in RESULTS if r["phase"] == "auth" and r["result"] != "N/A")),
        "Public tests requiring cookies": str(needing_cookies),
        "Public tests requiring login": str(needing_login),
        "Release gate": "PASS" if ok else "FAIL",
    }


def write_report(folder, meta, phases):
    folder.mkdir(parents=True, exist_ok=True)
    summary = gate()
    (folder / "report.json").write_text(json.dumps({"meta": meta, "summary": summary, "results": RESULTS}, indent=2), encoding="utf-8")
    esc = lambda s: str(s if s is not None else "").replace("|", "\\|").replace("\n", " ")
    lines = [f"# dm video website compatibility run, {meta['date']}", "",
             "| Field | Value |", "|---|---|"]
    lines += [f"| {k} | {esc(v)} |" for k, v in meta.items()]
    lines += ["", "## Results summary", "", "| Metric | Result |", "|---|---|"]
    lines += [f"| {k} | {'**' + v + '**' if k == 'Release gate' else v} |" for k, v in summary.items()]
    lines += ["", "Phases run: " + ", ".join(phases) + ". A gate worked out from fewer than the three public phases "
              "says nothing of the ones left out.", ""]
    lines += ["## Every test", "",
              "| Test ID | Website | Test | Priority | Exact URL | Time | Signed out | Cookies or credentials to dm "
              "| Format and quality | Result | Size | Playable | Notes | dm said |",
              "|" + "---|" * 14]
    for r in RESULTS:
        size = f"{r['size'] / MB:.1f} MB" if isinstance(r["size"], (int, float)) and r["size"] else ""
        lines.append("| " + " | ".join(esc(x) for x in (r["id"], r["website"], r["test"], r["priority"], r["url"], r["time"],
                                                         r["signed_out"], r["cookies_to_dm"], r["format"], r["result"],
                                                         size, r["playable"], r["notes"], r["log"])) + " |")
    lines += ["", "## Release decision", "",
              f"- [{'x' if summary['Release gate'] == 'PASS' else ' '}] Mandatory public-video acceptance suite passed.",
              "- [x] Public downloads were run in a clean signed-out session (a new dm home; a new browser profile).",
              f"- [{'x' if summary['Public tests requiring cookies'] == '0' else ' '}] No public test required cookies "
              "or browser-session extraction.",
              f"- [{'x' if not any(r['result'] == 'FAIL' and r['priority'] == 'core' and 'reason' in r['notes'] for r in RESULTS) else ' '}]"
              " Unsupported/restricted content produced clear errors (read the \"dm said\" column).",
              "- [x] Optional authenticated results are reported separately (phase `auth`, priority `optional`).",
              "", f"**Final decision:** {'APPROVE' if summary['Release gate'] == 'PASS' else 'DO NOT APPROVE'} "
              "(a person confirms, having read the FAIL and BLOCKED rows)", "",
              "**Known issues:**", ""]
    lines += [f"- {r['id']} {r['website']}: {r['notes'][:200]}" for r in RESULTS if r["result"] == "FAIL"] or ["- none found"]
    (folder / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return summary


def main():
    # dm's words hold "—" and "›": as UTF-8 in a log, not the Windows code page.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--phases", default="engine,public-url,public-panel")
    ap.add_argument("--auth", default="", help="sites for the optional signed-in suite: " + ",".join(AUTH_SITES))
    ap.add_argument("--panel-sites", default="", help="only these of public-panel's sites: " + ",".join(PANEL_SITES))
    ap.add_argument("--keep", action="store_true", help="keep the downloads and homes, not only the report")
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()
    if args.list:
        for test_id, site, what, priority, phase in PLAN:
            print(f"{test_id:<9} {priority:<5} {phase:<13} {site:<18} {what}")
        return 0
    phases = [p.strip() for p in args.phases.split(",") if p.strip()]
    unknown = set(phases) - {"engine", "public-url", "public-panel"}
    if unknown:
        sys.exit(f"no phase called {', '.join(sorted(unknown))}")
    if not BINARY.exists():
        sys.exit(f"no dm binary at {BINARY}: cd desktop; cargo build -p {SHIPPED} (or set DM_BINARY)")
    base = os.environ.get("DM_VIDEO_WORK")
    if base:
        pathlib.Path(base).mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-video-sites-", dir=base))
    report = pathlib.Path(os.environ.get("DM_VIDEO_REPORT") or work / "report")
    version = subprocess.run([str(BINARY), "--version"], capture_output=True, text=True, timeout=120).stdout.strip()
    meta = {"date": datetime.datetime.now().isoformat(timespec="seconds"),
            "dm version/build": f"{version} ({BINARY})",
            "git commit": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                                         cwd=str(ROOT)).stdout.strip(),
            "Operating system": f"{platform.system()} {platform.release()} ({platform.version()})",
            "Browser and extension version": f"{edge_version()}, dm extension {extension_version()}"
                                             if "public-panel" in phases or args.auth else "not used",
            "Clean session": "a new dm home per phase, no cookies file, no login; a new browser profile for the panel",
            "ffmpeg decode check": FFMPEG or "not installed: structure read by scripts/media_check.py only"}
    print(f"dm: {meta['dm version/build']}\nwork: {work}\nreport: {report}", flush=True)
    try:
        if "engine" in phases:
            engine_phase(work)
        if "public-url" in phases:
            public_url_phase(work)
        if "public-panel" in phases:
            public_panel_phase(work, [s.strip() for s in args.panel_sites.split(",") if s.strip()])
        if args.auth:
            auth_phase(work, [s.strip() for s in args.auth.split(",") if s.strip()])
    except KeyboardInterrupt:
        print("\nstopped by hand; the report has what ran", flush=True)
    finally:
        summary = write_report(report, meta, phases + (["auth"] if args.auth else []))
        if not args.keep:
            for child in work.iterdir():
                if child != report and child.name != "auth-profiles":
                    shutil.rmtree(child, ignore_errors=True) if child.is_dir() else child.unlink(missing_ok=True)
    print("\nRESULTS SUMMARY")
    for k, v in summary.items():
        print(f"  {k:<38} {v}")
    print(f"\nreport: {report / 'report.md'}")
    return 0 if summary["Release gate"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
