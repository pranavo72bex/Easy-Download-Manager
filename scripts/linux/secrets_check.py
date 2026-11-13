# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The installed dm keeps its secrets in the session's Secret Service (gaps row 39, Linux).

Run by `package.sh` with the installed binary (DM_BINARY), inside a D-Bus session with an
unlocked gnome-keyring: what a person's desktop gives dm. Checks, against a local server that
refuses a request without its sign-in:

  * a row with a login in its address, and a browser hand-off with cookies (the native host's
    `download.start`), are fetched with their sign-in;
  * nothing under dm's home holds the login, the cookie or a Sign-in's password in clear: the
    row's are in a sealed `secrets.json`, whose key the Secret Service keeps, and the Sign-in's
    password is in the Secret Service itself, the settings file keeping it empty;
  * dm ended mid-download and started again: both rows come back and resume with ranged
    requests that carry their sign-in, and both files are whole; the Sign-in is still kept.
"""

import base64
import http.server
import json
import os
import pathlib
import secrets
import shutil
import socket
import socketserver
import struct
import subprocess
import sys
import tempfile
import threading
import time

BINARY = os.environ.get("DM_BINARY") or "dm-gpui"
# Made up afresh for each run, never written in the repository (a secret scanner reads its history),
# and never printed: a check that fails names the file, not what it found there.
LOGIN = ("jo", secrets.token_urlsafe(18))
COOKIE_VALUE = secrets.token_urlsafe(18)
COOKIE = f"sid={COOKIE_VALUE}"
SIGN_IN = {"host": "files.example", "username": "ana b'c", "password": secrets.token_urlsafe(18)}
SECRETS = {"the row's login": LOGIN[1], "the browser's cookie": COOKIE_VALUE, "the Sign-in's password": SIGN_IN["password"]}
SIZE = 6 * 1024 * 1024
BODY = bytes((i * 7 + 3) % 251 for i in range(SIZE))
LOG = []
FAILED = []


def check(what, ok, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {what}" + (f" — {detail}" if detail else ""), flush=True)
    if not ok:
        FAILED.append(what)


class Serve(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        auth = self.headers.get("Authorization", "")
        cookie = self.headers.get("Cookie", "")
        LOG.append({"path": self.path, "auth": auth, "cookie": cookie,
                    "range": self.headers.get("Range", ""), "t": time.time()})
        wanted = {"/auth/a.bin": auth == "Basic " + base64.b64encode(":".join(LOGIN).encode()).decode(),
                  "/cookie/b.bin": COOKIE in cookie}
        if self.path not in wanted:
            self.send_error(404)
            return
        if not wanted[self.path]:
            self.send_response(401 if self.path.startswith("/auth") else 403)
            if self.path.startswith("/auth"):
                self.send_header("WWW-Authenticate", 'Basic realm="dm"')
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        start, end = 0, SIZE - 1
        ranged = self.headers.get("Range", "")
        if ranged.startswith("bytes="):
            first, _, last = ranged[6:].partition("-")
            start = int(first or 0)
            end = min(int(last), end) if last else end
        self.send_response(206 if ranged else 200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        if ranged:
            self.send_header("Content-Range", f"bytes {start}-{end}/{SIZE}")
        self.end_headers()
        try:
            # About a megabyte a second for each connection, so dm is ended mid-download.
            for at in range(start, end + 1, 32 * 1024):
                self.wfile.write(BODY[at:min(at + 32 * 1024, end + 1)])
                time.sleep(0.03)
        except (BrokenPipeError, ConnectionResetError):
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def frame(message):
    body = json.dumps(message).encode()
    return struct.pack("<I", len(body)) + body


def read_frames(data):
    out, at = [], 0
    while at + 4 <= len(data):
        n = struct.unpack("<I", data[at:at + 4])[0]
        out.append(json.loads(data[at + 4:at + 4 + n]))
        at += 4 + n
    return out


class Dm:
    def __init__(self, home):
        self.home, self.proc = home, None

    def start(self):
        (self.home / "control.port").unlink(missing_ok=True)
        env = dict(os.environ, DM_HOME=str(self.home))
        env.pop("DM_NO_SECRET_STORE", None)
        self.proc = subprocess.Popen([BINARY, "--control", "0"], env=env, stdout=subprocess.DEVNULL,
                                     stderr=open(self.home.parent / "dm-stderr.txt", "a"))
        port = self.home / "control.port"
        for _ in range(300):
            if port.exists() and port.read_text().strip():
                break
            time.sleep(0.1)
        self.conn = socket.create_connection(("127.0.0.1", int(port.read_text())), timeout=30)
        self.replies = self.conn.makefile("r", encoding="utf-8")
        self.call("auth", token=(self.home / "control.token").read_text().strip())

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(self.replies.readline())

    def rows(self):
        return self.call("queue.list").get("result") or []

    def row(self, part):
        return next((r for r in self.rows() if part in r.get("url", "")), None)

    def stop(self):
        self.proc.terminate()
        self.proc.wait(30)


def files_holding(home, named):
    """Which files under `home` (dm's home and what dm printed beside it) hold which of `named`'s
    secrets in clear: by their names only."""
    found = []
    for path in home.rglob("*"):
        if path.is_file():
            data = path.read_bytes()
            found += [f"{path.relative_to(home)} holds {name}" for name, value in named.items() if value.encode() in data]
    return found


def store_items():
    """What the Secret Service keeps under dm's service: each item's name and its secret."""
    if not shutil.which("secret-tool"):
        return []
    got = subprocess.run(["secret-tool", "search", "--all", "--unlock", "service", "dm"],
                         capture_output=True, text=True, timeout=20)
    items, current = [], {}
    for line in (got.stdout + got.stderr).splitlines():
        line = line.strip()
        if line.startswith("[/"):
            current = {}
            items.append(current)
        elif " = " in line:
            key, _, value = line.partition(" = ")
            current[key.strip()] = value
    return items


def store_lookup(name):
    """The value the Secret Service keeps under dm's `name`, asked for exactly, or nothing."""
    if not shutil.which("secret-tool"):
        return b""
    got = subprocess.run(["secret-tool", "lookup", "service", "dm", "name", name],
                         capture_output=True, text=True, timeout=20)
    return decoded(got.stdout) if got.returncode == 0 else b""


def decoded(text):
    """A secret as dm keeps it in the store: base64."""
    try:
        return base64.b64decode(text.strip(), validate=True)
    except ValueError:
        return b""


def sign_in_kept(items):
    """Whether the store keeps the Sign-in's password. Found by its value: its item's name holds a
    line break between the site and the user, which `secret-tool search` prints across two lines."""
    return any(decoded(i.get("secret", "")) == SIGN_IN["password"].encode() for i in items)


def main():
    if not shutil.which("secret-tool"):
        check("secret-tool, which dm reaches the Secret Service through, is installed with the package", False,
              "not on PATH: the store is never used and the secrets stay in a file in clear")
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-secrets-"))
    home = work / "home"
    home.mkdir()
    # A Sign-in written in clear by an earlier dm: moved into the store on the first read.
    (home / "settings.json").write_text(json.dumps({"site_logins": [SIGN_IN]}))
    server = Server(("127.0.0.1", 0), Serve)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_address[1]}"

    dm = Dm(home)
    dm.start()
    added = dm.call("downloads.add", url=f"http://{LOGIN[0]}:{LOGIN[1]}@127.0.0.1:{server.server_address[1]}/auth/a.bin")
    env = dict(os.environ, DM_HOME=str(home))
    env.pop("DM_NO_SECRET_STORE", None)
    host = subprocess.run([BINARY, "--native-host"], env=env, timeout=30, capture_output=True,
                          input=frame({"protocolVersion": 1, "type": "engine.hello", "id": "h"})
                          + frame({"protocolVersion": 1, "type": "download.start", "id": "c",
                                   "url": f"{base}/cookie/b.bin", "referrer": f"{base}/page",
                                   "cookies": COOKIE}))
    replies = [r.get("type") for r in read_frames(host.stdout)]
    check("both are taken: a login in the address, and a browser hand-off with cookies",
          added.get("ok", "error" not in added) and "download.started" in replies,
          f"added {(added.get('result') or {}).get('added')} / {replies}")
    # Under way on both, with their sign-in, then dm is ended.
    deadline = time.time() + 30
    while time.time() < deadline:
        moving = [r for r in dm.rows() if (r.get("progress") or {}).get("downloaded", 0) > 512 * 1024]
        if len(moving) >= 2:
            break
        time.sleep(0.3)
    signed = lambda path, field, want: any(e["path"] == path and want in e[field] for e in LOG)
    check("each is fetched with its sign-in", signed("/auth/a.bin", "auth", "Basic") and signed("/cookie/b.bin", "cookie", COOKIE),
          f"{[(e['path'], bool(e['auth']), bool(e['cookie'])) for e in LOG][:6]}")
    held = files_holding(work, SECRETS)
    check("nothing in dm's home or what it printed holds the login, the cookie or the Sign-in's password in clear",
          not held, str(held))
    sealed_file = home / "secrets.json"
    try:
        envelope = json.loads(sealed_file.read_text())
    except (OSError, ValueError):
        envelope = {}
    check("the rows' secrets file is sealed (ChaCha20-Poly1305, under the key the store keeps)",
          isinstance(envelope, dict) and envelope.get("alg") == "chacha20poly1305"
          and envelope.get("key") == "dm:queue-secrets-key",
          f"alg {envelope.get('alg')!r}, key {envelope.get('key')!r}" if isinstance(envelope, dict) else "not an envelope")
    items = store_items()
    check("the Secret Service keeps the key that seals it (32 bytes; its value is not printed)",
          len(store_lookup("dm:queue-secrets-key")) == 32)
    check("and the Sign-in's password, which the settings file keeps empty",
          sign_in_kept(items) and json.loads((home / "settings.json").read_text())["site_logins"][0]["password"] == "",
          f"{len(items)} items under dm's service")
    dm.stop()
    before = {p: max([e["t"] for e in LOG if e["path"] == p] or [0]) for p in ("/auth/a.bin", "/cookie/b.bin")}

    dm.start()
    time.sleep(1.5)
    for r in dm.rows():
        if r["state"]["kind"] in ("paused", "failed", "stopped"):
            dm.call("downloads.resume", id=r["id"])
    deadline = time.time() + 90
    while time.time() < deadline:
        states = {r["url"].rsplit("/", 1)[-1]: r["state"]["kind"] for r in dm.rows()}
        if all(s == "completed" for s in states.values()) and len(states) == 2:
            break
        time.sleep(0.5)
    check("dm started again: both rows come back and finish", states == {"a.bin": "completed", "b.bin": "completed"}, str(states))
    after = [e for e in LOG if e["t"] > before.get(e["path"], 0) and e["range"].startswith("bytes=") and not e["range"].startswith("bytes=0-")]
    check("they resume with ranged requests that carry their sign-in",
          any(e["path"] == "/auth/a.bin" and e["auth"].startswith("Basic") for e in after)
          and any(e["path"] == "/cookie/b.bin" and COOKIE in e["cookie"] for e in after),
          str([(e["path"], e["range"], bool(e["auth"]), bool(e["cookie"])) for e in after][:6]))
    done = [r for r in dm.rows() if r["state"]["kind"] == "completed"]
    whole = [pathlib.Path(r["path"]).read_bytes() == BODY for r in done if r.get("path")]
    check("and both files are whole", len(whole) == 2 and all(whole), str(whole))
    check("the Sign-in is still kept after the restart",
          sign_in_kept(store_items())
          and json.loads((home / "settings.json").read_text())["site_logins"][0]["password"] == "")
    held = files_holding(work, SECRETS)
    check("and still nothing in dm's home or what it printed holds a secret in clear", not held, str(held))
    dm.stop()
    print(f"\n{'all' if not FAILED else len(FAILED)} {'checks passed' if not FAILED else 'failed: ' + '; '.join(FAILED)}")
    sys.exit(1 if FAILED else 0)


main()
