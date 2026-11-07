# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Loads the built extension into a real Chrome and watches it hand off.

    sh scripts/package.sh                      # builds dist/chrome
    (cd desktop && cargo build -p dm-desktop)  # the app the host starts
    python3 scripts/chrome-check.py

Chrome for Testing, because branded Chrome stopped honouring
--load-extension in 137 and this one still does. It is downloaded on first
use from Google's chrome-for-testing JSON endpoints into
~/.cache/dm-chrome-for-testing (CHROME_PATH names one instead).

What it proves, against a local server and nothing else on the network:

  1. the extension loads, its WebAssembly initialises, and its id is the one
     the manifest key fixes (the one `dm-desktop --register` assumes)
  2. a download started by clicking a link on a page is caught, handed to a
     running dm over native messaging, fetched by dm (with the page as its
     referrer) into dm's folder, and removed from the browser's list
     — and so is a PDF served fast, which Chromium finishes before it can
     be held (its copy in the browser's folder is removed once dm has it)
  3. a download of a type the app's settings do not list stays in the browser
  4. the catch rules are the app's: a .zip, which dm catches by default, is
     left alone when the app's settings say only .bin
  5. a link only the browser can fetch (it needs the page's cookie, which dm
     does not have) is refused by dm before anything is taken, and finishes
     in the browser
  6. "Download link with dm" hands a link over (the handler the menu calls,
     invoked from the worker: a real context menu cannot be clicked from here)
  7. with the host unregistered, a download is left to the browser and
     finishes there — nothing is lost
  8. nothing the worker logged was an error
  9. a file whose name is too long for the disk (Chromium would ask where
     to save it) goes to dm with no Save dialog left open (looked for with
     xdotool where there is an X display), and "Download all links" works
     on a page whose page script is not in it (one still arriving, as a
     tab open before dm was installed has none) (gap 122)

Everything lives in temporary directories: a profile, the browser's download
folder, and DM_HOME for the app, whose settings, queue, inbox and lock are
its own. The host binary is copied out of the checkout first: macOS keeps
~/Downloads from browsers, and a host started from there hangs in the loader
without a word (which is why `--register` refuses such a path). The native
messaging manifest goes into the profile's own NativeMessagingHosts, which
Chrome for Testing reads for a --user-data-dir profile.
"""

import base64, hashlib, http.server, json, os, pathlib, platform, shutil, signal, socket
import socketserver, struct, subprocess, sys, tempfile, threading, time, urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXT = ROOT / "dist" / "chrome"
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")
CACHE = pathlib.Path(os.environ.get("DM_CHROME_CACHE") or pathlib.Path.home() / ".cache/dm-chrome-for-testing")
ENDPOINT = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json"

results = []


# The extension's worker, once it is found: a failed check says what the
# extension last said (gaps row 141), so a failure that does not come again
# leaves its reason behind.
WATCHED = []
MARK = [0]


def check(name, ok, detail=""):
    results.append((ok, name, detail))
    print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f" — {detail}" if detail else ""), flush=True)
    if WATCHED:
        worker = WATCHED[0]
        if not ok:
            try:
                worker.drain(0.5)
                said = [line[:200] for line in worker.logged("log", "info", "warning", "error", since=MARK[0])
                        if line.startswith("dm")][-6:]
                print(f"        the extension said since the last check: {said}", flush=True)
            except Exception as e:  # the worker gone or stopped: the check's own result stands
                print(f"        (the extension could not be read: {e!r})", flush=True)
        MARK[0] = len(worker.events)
    return ok


# ------------------------------------------------------------ the browser


def chrome_platform():
    system, machine = platform.system(), platform.machine().lower()
    if system == "Darwin":
        return "mac-arm64" if machine in ("arm64", "aarch64") else "mac-x64"
    if system == "Linux":
        return "linux64"
    sys.exit(f"no Chrome for Testing build for {system} {machine} is known here")


def executable(root, plat):
    return root / {
        "mac-arm64": "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        "mac-x64": "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        "linux64": "chrome-linux64/chrome",
    }[plat]


EDGE_PATHS = {
    "Darwin": ["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
    "Linux": ["/usr/bin/microsoft-edge", "/usr/bin/microsoft-edge-stable", "/opt/microsoft/msedge/msedge"],
    "Windows": [r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"],
}


def find_chrome():
    if os.environ.get("CHROME_PATH"):
        return pathlib.Path(os.environ["CHROME_PATH"])
    # Microsoft Edge, where it is installed: the browser these checks are
    # run in (the maintainer's word, 2026-10-30); only without it is Chrome
    # for Testing fetched.
    for candidate in EDGE_PATHS.get(platform.system(), []):
        if pathlib.Path(candidate).exists():
            print(f"browser: Microsoft Edge at {candidate}", flush=True)
            return pathlib.Path(candidate)
    plat = chrome_platform()
    with urllib.request.urlopen(ENDPOINT, timeout=60) as reply:
        stable = json.load(reply)["channels"]["Stable"]
    root = CACHE / stable["version"]
    binary = executable(root, plat)
    if binary.exists():
        return binary
    url = next(d["url"] for d in stable["downloads"]["chrome"] if d["platform"] == plat)
    print(f"downloading Chrome for Testing {stable['version']} ({plat})", flush=True)
    root.mkdir(parents=True, exist_ok=True)
    archive = root / "chrome.zip"
    with urllib.request.urlopen(url, timeout=600) as reply, open(archive, "wb") as out:
        shutil.copyfileobj(reply, out)
    # ditto and unzip keep the symlinks and modes a .app needs; zipfile does not.
    if plat.startswith("mac"):
        subprocess.run(["ditto", "-x", "-k", str(archive), str(root)], check=True)
    else:
        subprocess.run(["unzip", "-q", str(archive), "-d", str(root)], check=True)
    archive.unlink()
    return binary


# ---------------------------------------------- a websocket, for DevTools


class Socket:
    """Just enough RFC 6455 to speak the DevTools protocol: text frames out,
    masked; text frames in, possibly fragmented; pings answered."""

    def __init__(self, url):
        u = urllib.parse.urlparse(url)
        self.sock = socket.create_connection((u.hostname, u.port), timeout=90)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            f"GET {u.path} HTTP/1.1\r\nHost: {u.hostname}:{u.port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            .encode()
        )
        head = b""
        while not head.endswith(b"\r\n\r\n"):
            byte = self.sock.recv(1)
            if not byte:
                raise EOFError("DevTools closed the connection during the handshake")
            head += byte
        if b" 101 " not in head.split(b"\r\n")[0]:
            raise RuntimeError(f"DevTools refused the websocket: {head[:200]!r}")

    def _frame(self, opcode, data):
        mask = os.urandom(4)
        n = len(data)
        head = bytearray([0x80 | opcode])
        if n < 126:
            head.append(0x80 | n)
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        self.sock.sendall(bytes(head) + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def send(self, text):
        self._frame(0x1, text.encode())

    def _exact(self, n):
        out = b""
        while len(out) < n:
            chunk = self.sock.recv(n - len(out))
            if not chunk:
                raise EOFError("DevTools closed the connection")
            out += chunk
        return out

    def recv(self):
        message = b""
        while True:
            b1, b2 = self._exact(2)
            n = b2 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._exact(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._exact(8))[0]
            mask = self._exact(4) if b2 & 0x80 else None
            data = self._exact(n)
            if mask:
                data = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
            opcode = b1 & 0x0F
            if opcode == 0x9:
                self._frame(0xA, data)
                continue
            if opcode == 0x8:
                raise EOFError("DevTools closed the connection")
            message += data
            if b1 & 0x80:
                return message.decode()


class Target:
    """One DevTools target: a page, or the extension's service worker."""

    def __init__(self, ws_url):
        self.ws = Socket(ws_url)
        self.next = 0
        self.events = []

    def call(self, method, **params):
        self.next += 1
        mine = self.next
        self.ws.send(json.dumps({"id": mine, "method": method, "params": params}))
        while True:
            message = json.loads(self.ws.recv())
            if message.get("id") == mine:
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error']}")
                return message["result"]
            self.events.append(message)

    def evaluate(self, expression, gesture=False):
        result = self.call(
            "Runtime.evaluate", expression=expression, awaitPromise=True,
            returnByValue=True, userGesture=gesture,
        )
        if "exceptionDetails" in result:
            details = result["exceptionDetails"]
            raise RuntimeError(details.get("exception", {}).get("description") or details.get("text"))
        return result["result"].get("value")

    def drain(self, seconds=0.5):
        self.ws.sock.settimeout(seconds)
        try:
            while True:
                self.events.append(json.loads(self.ws.recv()))
        except (socket.timeout, TimeoutError):
            pass
        finally:
            self.ws.sock.settimeout(90)

    def logged(self, *levels, since=0):
        """What the target logged at these levels, from event `since` on."""
        out = []
        for event in self.events[since:]:
            params = event.get("params", {})
            if event.get("method") == "Runtime.consoleAPICalled" and params.get("type") in levels:
                out.append(" ".join(str(a.get("value", a.get("description", ""))) for a in params.get("args", [])))
            if event.get("method") == "Runtime.exceptionThrown" and "error" in levels:
                d = params.get("exceptionDetails", {})
                out.append(d.get("exception", {}).get("description") or d.get("text", ""))
        return out


# ------------------------------------------------------------ the server


SIZE = 1024 * 1024
requests = []
# A name too long for the disk once Chromium adds ".crdownload": Chromium
# asks where to save it rather than shorten it.
LONG = "very" + "long" * 61 + ".bin"
ARRIVING = ("early1.bin", "early2.bin", "early3.bin")


def is_dm(agent):
    """Whether a request was dm's own and not the browser's: dm asks as
    Internet Explorer 11 first (the agent reference A sends), or by its name."""
    return agent.startswith("dm/") or "Trident/7.0" in agent


def body(name):
    seed = hashlib.sha256(name.encode()).digest()
    # A few files of the sizes that small ones are: 3 MB, which a server that
    # holds each connection to a megabyte a second serves in three; 1.5 MB.
    size = (300_000 if name.startswith("small") else
            3 * 1024 * 1024 if name.startswith("paced") else
            3 * 1024 * 1024 // 2 if name.startswith("quick") else SIZE)
    return (seed * (size // len(seed) + 1))[:size]


# Pages with players on them, for the floating panel. A transparent layer
# lies over the video, the way real players stack their controls.
OVERLAY = ('<div style="position:absolute;left:0;top:0;width:640px;height:360px;'
           'background:transparent;z-index:5"></div>')
PANEL_PAGES = {
    "/video.html": (
        '<!doctype html><title>a video</title><div style="position:relative;margin:40px">'
        '<video id="v" src="/files/clip.mp4" width="640" height="360" preload="none"></video>'
        + OVERLAY + '</div>', None),
    "/player.html": (
        '<!doctype html><title>a player</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());</script>',
        None),
    # A streaming player that also loads an HLS playlist, as a site's own
    # player does: what the page's traffic shows.
    "/stream.html": (
        '<!doctype html><title>a stream</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());'
        'fetch("/files/stream.m3u8");</script>',
        None),
    # A page whose player loads a master playlist: the stream's line is named
    # from what the playlist lists (gaps row 64).
    "/master.html": (
        '<!doctype html><title>a master playlist</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());'
        'fetch("/files/master.m3u8");</script>',
        None),
    # The same, on a page that also names its video: both kinds in one list.
    "/both.html": (
        '<!doctype html><title>both</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());'
        'fetch("/files/both.m3u8");</script>',
        None),
    # A player built as a web component, its video in an open shadow root
    # under its own control layer, as the Internet Archive's `play-av` is.
    "/shadow.html": (
        '<!doctype html><title>a component</title><div style="margin:40px"><dm-test-player></dm-test-player></div>'
        '<script>customElements.define("dm-test-player", class extends HTMLElement { connectedCallback() {'
        ' const r = this.attachShadow({mode: "open"});'
        ' r.innerHTML = \'<div style="position:relative;width:640px;height:360px">'
        '<video id="v" src="/files/inner.mp4" width="640" height="360" preload="none"></video>'
        '<div style="position:absolute;left:0;top:0;width:640px;height:360px;z-index:5"></div></div>\'; } });</script>',
        None),
    # The player page a site embeds from another origin, as Dailymotion's
    # player is: served as `localhost` into a page on 127.0.0.1, so Chrome
    # runs it in a frame of its own process.
    "/framevideo.html": (
        '<!doctype html><title>an embedded player</title><div style="position:relative;margin:10px">'
        '<video id="v" src="/files/framed.mp4" width="640" height="360" preload="none"></video>'
        + OVERLAY + '</div>', None),
    # A video only the signed-in browser can fetch (the file is refused
    # without the page's session cookie, as W3Schools refused dm with 403).
    "/privatevideo.html": (
        '<!doctype html><title>a signed-in video</title><div style="position:relative;margin:40px">'
        '<video id="v" src="/files/private-clip.mp4" width="640" height="360" preload="none"></video>'
        + OVERLAY + '</div>', None),
    "/tiny.html": (
        '<!doctype html><title>an avatar</title>'
        '<video id="v" src="/files/avatar.mp4" width="96" height="96" preload="none" style="margin:40px"></video>',
        None),
    # A site that allows almost nothing: the panel is the extension's, and
    # the page's policy must not keep it from loading.
    "/strict.html": (
        '<!doctype html><title>a strict site</title><div style="position:relative;margin:40px">'
        '<video id="v" src="/files/strict.mp4" width="640" height="360" preload="none"></video></div>',
        "default-src 'none'; media-src 'self'; style-src 'none'; script-src 'none'"),
}

# Pages for the request checks (gaps rows 116 to 119 and 123): a file guarded by a request header, the
# seven ways a page reads an answer, a player that fetches picture and sound in ranges, a feed whose
# players are told apart by React-like props, and a page with more links than `<a>`.
REQUEST_PAGES = {
    "/guard.html": (
        '<!doctype html><title>guarded</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());'
        'fetch("/guarded/guarded1.mp4", {headers: {Authorization: "Bearer t"}}).then(r => r.arrayBuffer());</script>',
        None),
    "/answers.html": (
        '<!doctype html><title>answers</title><pre id="log"></pre><script>'
        'window.__ways = [];'
        '(async () => {'
        ' const note = (w) => window.__ways.push(w);'
        ' await (await fetch("/files/a-text.m3u8")).text(); note("text");'
        ' await (await fetch("/api/play?x=1")).json(); note("json as octet-stream");'
        ' await (await fetch("/files/a-buffer.m3u8")).arrayBuffer(); note("arrayBuffer");'
        ' await (await fetch("/files/a-blob.m3u8")).blob(); note("blob");'
        ' const r = (await fetch("/files/a-body.m3u8")).body.getReader();'
        ' for (;;) { const { done } = await r.read(); if (done) break; } note("body");'
        ' for (const [type, url] of [["json", "/api/play?x=2"], ["arraybuffer", "/files/a-xhr-buffer.m3u8"],'
        '                            ["blob", "/files/a-xhr-blob.m3u8"]]) {'
        '  await new Promise((done) => { const x = new XMLHttpRequest(); x.open("GET", url); x.responseType = type;'
        '   x.onload = () => { note("xhr " + type); done(); }; x.send(); }); }'
        ' window.__finished = true;'
        '})();</script>', None),
    "/early.html": (
        '<!doctype html><title>early</title>'
        # In the head, before the body exists: this runs at document_start, when only the page
        # hook is in, and the answer is back long before content.js (document_idle) sends the rules.
        '<script>window.__early = fetch("/api/early?v=7").then(r => r.json());</script>'
        '<div style="position:relative;margin:40px"><video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());</script>',
        None),
    "/pieces.html": (
        '<!doctype html><title>pieces</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());'
        'for (const u of ["/media/low.mp4?id=1&bytestart=0&byteend=999", "/media/high.mp4?id=2&bytestart=0&byteend=999",'
        ' "/media/sound.m4a?id=3&bytestart=0&byteend=999"]) fetch(u).then(r => r.arrayBuffer());</script>', None),
    "/feed.html": (
        '<!doctype html><title>feed</title><div id="feed"></div>'
        '<script type="application/json">{"videos":['
        '{"videoFBID":"1111111111","progressive_urls":[{"progressive_url":"https:\\/\\/v.example\\/v\\/one-sd.mp4","metadata":{"quality":"SD"}}]},'
        '{"videoFBID":"2222222222","progressive_urls":[{"progressive_url":"https:\\/\\/v.example\\/v\\/two-sd.mp4","metadata":{"quality":"SD"}},'
        '{"progressive_url":"https:\\/\\/v.example\\/v\\/two-hd.mp4","metadata":{"quality":"HD"}}]}]}</script>'
        '<script>for (const id of ["1111111111", "2222222222"]) {'
        ' const box = document.createElement("div"); box.className = "vp-video";'
        ' box.style.cssText = "position:relative;margin:20px;width:640px;height:360px;background:#222";'
        ' const v = document.createElement("video"); v.className = "feedvideo"; v.width = 640; v.height = 360;'
        ' v.src = URL.createObjectURL(new MediaSource());'
        ' v["__reactFiber$" + Math.random().toString(36).slice(2)] ='
        '  {return: {return: {memoizedProps: {coreVideoPlayerMetaData: {videoFBID: id}}}}};'
        ' box.appendChild(v); document.getElementById("feed").appendChild(box); }'
        'fetch("/files/stream.m3u8");</script>', None),
    "/morelinks.html": (
        '<!doctype html><title>more links</title>'
        '<p><a href="/files/more-a.bin">a</a></p>'
        '<map name="m"><area shape="rect" coords="0,0,10,10" href="/files/more-area.bin"></map>'
        '<img usemap="#m" width="20" height="20" src="/files/more-img.bin">'
        '<iframe src="/files/more-frame.bin" width="10" height="10"></iframe>'
        '<script>var u = "{ORIGIN}/files/more-script.bin";</script>', None),
}
PANEL_PAGES.update(REQUEST_PAGES)
# Which player the panel goes on, and where (gaps row 125): one page for each rule.
PLACEMENT_PAGES = {
    # A dialog over the whole page, as a site's sign-in lies over its reel: the
    # player under it is not offered.
    "/covered.html": (
        '<!doctype html><title>covered</title><div style="position:relative;margin:40px">'
        '<video id="v" src="/files/clip.mp4" width="640" height="360" preload="none"></video></div>'
        '<div style="position:fixed;inset:0;background:#fff;z-index:20"><p>Sign in to go on</p></div>', None),
    # A parent that hides what overflows it (a carousel): the panel stays in
    # the part of the player that can be seen.
    "/clipped.html": (
        '<!doctype html><title>clipped</title>'
        '<div id="clip" style="position:relative;overflow:hidden;width:400px;height:300px;margin:40px">'
        '<video id="v" src="/files/clip.mp4" width="640" height="360" preload="none"></video></div>', None),
    # Smaller than any player (a tracking pixel playing): passed by.
    "/tiny.html": (
        '<!doctype html><title>tiny</title><div style="margin:40px">'
        '<video id="v" src="/files/clip.mp4" width="12" height="8" preload="none"></video></div>', None),
    # A site's fixed header lying over the top of the player: the panel goes
    # below it.
    "/topbar.html": (
        '<!doctype html><title>topbar</title>'
        '<div id="bar" style="position:fixed;top:0;left:0;right:0;height:70px;background:#333;z-index:10"></div>'
        '<div style="position:relative;margin:30px 40px 0">'
        '<video id="v" src="/files/clip.mp4" width="640" height="360" preload="none"></video></div>', None),
}
PANEL_PAGES.update(PLACEMENT_PAGES)


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_POST(self):
        # A file given only as the answer to its form (gaps row 128): an export
        # button's kind, which a GET of the same address does not get.
        path = urllib.parse.urlparse(self.path).path
        agent = self.headers.get("User-Agent", "")
        sent = self.rfile.read(int(self.headers.get("Content-Length", "0") or 0))
        requests.append({"path": path, "agent": agent, "method": "POST", "t": time.monotonic(),
                         "referer": self.headers.get("Referer", ""), "range": "", "auth": "",
                         "port": self.client_address[1], "body": sent[:200],
                         "headers": [f"{k}: {v}" for k, v in self.headers.items()]})
        # Read by field, as a server reads a form: the browser reports a form's
        # fields sorted by name, and dm sends them so.
        fields = urllib.parse.parse_qs(sent.decode("latin-1"))
        if path != "/export.bin" or fields != {"report": ["7"], "format": ["csv"]}:
            self.send_error(400)
            return
        data = body("posted.bin")
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", 'attachment; filename="posted.bin"')
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        # Slowly to the browser, so the extension holds it; at once to dm.
        if is_dm(agent):
            self.wfile.write(data)
        else:
            for at in range(0, len(data), 64 * 1024):
                self.wfile.write(data[at:at + 64 * 1024])
                time.sleep(0.05)

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        agent = self.headers.get("User-Agent", "")
        requests.append({"path": path, "agent": agent, "referer": self.headers.get("Referer", ""),
                         "range": self.headers.get("Range", ""), "t": time.monotonic(),
                         "auth": self.headers.get("Authorization", ""),
                         "port": self.client_address[1]})
        if path == "/magnets.html":
            # The seven ways a torrent site's button opens a magnet link (gaps
            # row 108, spec T6); the hashes are made up.
            page = (
                '<!doctype html><title>magnets</title>'
                '<p><a id="m1" href="magnet:?xt=urn:btih:0000000000000000000000000000000000abc00a&dn=plain">1</a></p>'
                '<p><a id="m2" href="magnet:?xt=urn:btih:0000000000000000000000000000000000abc00b&dn=onclick" '
                'onclick="window.location=this.href; return false;">2</a></p>'
                '<p><button id="m3" onclick="window.location.href=\'magnet:?xt=urn:btih:0000000000000000000000000000000000abc00c&dn=button\'">3</button></p>'
                '<p><a id="m4" href="magnet:?xt=urn:btih:0000000000000000000000000000000000abc00d&dn=blank" target="_blank">4</a></p>'
                '<p><a id="m5" href="magnet:?xt=urn:btih:0000000000000000000000000000000000abc00e&dn=icon"><span><i>&#9733;</i> 5</span></a></p>'
                '<p><a id="m6" href="#" onclick="window.open(\'magnet:?xt=urn:btih:0000000000000000000000000000000000abc00f&dn=windowopen\'); return false;">6</a></p>'
                '<p><a id="m7" href="#" onclick="var a=document.createElement(\'a\');a.href=\'magnet:?xt=urn:btih:0000000000000000000000000000000000abc010&dn=synthetic\';document.body.appendChild(a);a.click();return false;">7</a></p>'
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if path == "/newtab.html":
            # A download link that opens a tab of its own (gaps row 130).
            page = ('<!doctype html><title>a new tab</title>'
                    '<p><a id="go" target="_blank" href="/files/newtab.bin">newtab.bin</a></p>').encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if path == "/form.html":
            page = ('<!doctype html><title>an export</title>'
                    '<form id="export" method="post" action="/export.bin">'
                    '<input name="report" value="7"><input name="format" value="csv">'
                    '<button id="go" type="submit">Export</button></form>').encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if path == "/export.bin":
            # Asked for plainly, the export gives nothing.
            self.send_error(405)
            return
        if path == "/framed.html":
            port = self.server.server_address[1]
            page = ('<!doctype html><title>a page embedding a player</title><h1>an article</h1>'
                    f'<iframe id="f" src="http://localhost:{port}/framevideo.html" width="700" height="420"'
                    ' style="border:0;margin:20px"></iframe>').encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if path == "/api/early":
            # The one answer a feed's page makes first, naming its stream.
            data = b'{"id": 7, "stream": "https://cdn.example/early/playlist.m3u8"}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if path == "/api/play":
            # A player's API that answers JSON as application/octet-stream.
            data = b'{"hls": "https://cdn.example/a/master.m3u8"}'
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if path.startswith("/media/"):
            # One range of a picture or a sound, as a player asks for it in pieces.
            name = path.rsplit("/", 1)[1]
            total = {"low.mp4": 300_000, "high.mp4": 900_000, "sound.m4a": 200_000}.get(name, 100_000)
            data = body(name)[:1000]
            self.send_response(206)
            self.send_header("Content-Type", "audio/mp4" if name.endswith(".m4a") else "video/mp4")
            self.send_header("Content-Range", f"bytes 0-999/{total}")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if path.startswith("/guarded/"):
            # A file the site guards with a request header: 403 without it.
            if self.headers.get("Authorization") != "Bearer t":
                self.send_error(403)
                return
            name = path.rsplit("/", 1)[1]
            data = body(name)
            self.send_response(200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            try:
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError):
                pass
            return
        if path in PANEL_PAGES:
            page, csp = PANEL_PAGES[path]
            page = page.replace("{ORIGIN}", f"http://127.0.0.1:{self.server.server_address[1]}").encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            if csp:
                self.send_header("Content-Security-Policy", csp)
            if path == "/privatevideo.html":
                self.send_header("Set-Cookie", "session=1; Path=/")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if path == "/arriving.html":
            # Its links at once, the rest of the page much later: the page
            # script, which runs once the page is in, is not there yet.
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(("<!doctype html><title>arriving</title>" + "".join(
                f'<p><a href="/files/{n}">{n}</a></p>' for n in ARRIVING) + " " * 4096).encode())
            self.wfile.flush()
            time.sleep(25)
            self.close_connection = True
            return
        if path == "/page.html":
            links = "".join(f'<p><a id="{n}" href="/files/{n}">{n}</a></p>' for n in
                            ("caught.bin", "notes.txt", "kept.zip", "private.bin", "absent.bin",
                             "alt.bin", "off.bin", "forced.txt", "paused.bin", "resumed.bin", "small.pdf", "small-shown.pdf", "small.bin",
                             "paced.bin", "quick.bin", "quick2.bin", "quick3.bin", LONG))
            page = f"<!doctype html><title>dm check</title>{links}".encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            # A signed-in session, which the browser has and dm does not.
            self.send_header("Set-Cookie", "session=1; Path=/")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if not path.startswith("/files/"):
            self.send_error(404)
            return
        if path.endswith("/master.m3u8"):
            playlist = ("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720\n/files/hi.m3u8\n"
                        "#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n/files/lo.m3u8\n").encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/vnd.apple.mpegurl")
            self.send_header("Content-Length", str(len(playlist)))
            self.end_headers()
            self.wfile.write(playlist)
            return
        if path.endswith(".m3u8"):
            playlist = ("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n"
                        "#EXTINF:4,\n/files/seg-0.ts\n#EXTINF:4,\n/files/seg-1.ts\n#EXT-X-ENDLIST\n").encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/vnd.apple.mpegurl")
            self.send_header("Content-Length", str(len(playlist)))
            self.end_headers()
            self.wfile.write(playlist)
            return
        name = path.rsplit("/", 1)[1]
        if name.startswith("private") and "session=1" not in self.headers.get("Cookie", ""):
            self.send_error(403)
            return
        data = body(name)
        start, end = 0, len(data) - 1
        ranged = self.headers.get("Range", "")
        if ranged.startswith("bytes="):
            first, _, last = ranged[6:].partition("-")
            start = int(first or 0)
            end = min(int(last), end) if last else end
        self.send_response(206 if ranged else 200)
        self.send_header("Content-Type", "application/pdf" if name.endswith(".pdf") else "application/octet-stream")
        # A file a browser shows in the tab is served as a page would be,
        # with no Content-Disposition: opening it starts no download.
        if not name.startswith("small-shown"):
            self.send_header("Content-Disposition", f'attachment; filename="{name}"')
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        if ranged:
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(data)}")
        self.end_headers()
        # The browser is served slowly, the way a large file arrives, so a
        # download is still running when the extension holds it; dm is not.
        slow = not is_dm(agent) and not name.startswith("small")
        # "paced": every connection, dm's too, held to about a megabyte a
        # second, as a server that limits each one does.
        paced = name.startswith("paced")
        try:
            for at in range(start, end + 1, 32 * 1024):
                self.wfile.write(data[at:min(at + 32 * 1024, end + 1)])
                if paced:
                    time.sleep(0.03125)
                elif slow:
                    time.sleep(0.08)
        except (BrokenPipeError, ConnectionResetError):
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        # A browser dropping a connection it has cancelled is the point of
        # the exercise, not an error in it.
        if not isinstance(sys.exc_info()[1], (ConnectionResetError, BrokenPipeError)):
            super().handle_error(request, client_address)


# ------------------------------------------------------------- the check


# Under emulation (Edge for x86_64 on an arm64 machine, human-distro.sh)
# everything takes several times as long: DM_SLOW multiplies every wait.
SLOW = float(os.environ.get("DM_SLOW") or 1)


# What DevTools says when the page went to another document while it was
# asked: an answer still to come, as `loaded` takes it.
NAVIGATED = ("Inspected target navigated or closed", "Execution context was destroyed",
             "Cannot find context with specified id")


def wait_for(probe, seconds):
    def ask():
        try:
            return probe()
        except RuntimeError as e:
            # YouTube's watch page went to another document once after it had
            # loaded (the Windows session, 2026-11-07), and the question cut
            # short ended the whole run, every check after it unrun.
            if any(said in str(e) for said in NAVIGATED):
                return None
            raise
    deadline = time.time() + seconds * SLOW
    while time.time() < deadline:
        value = ask()
        if value:
            return value
        time.sleep(0.25)
    return ask()


def whole(folder, name):
    """The file, once it is all there and right."""
    path = folder / name
    return path.exists() and path.read_bytes() == body(name)


def devtools(profile):
    port_file = profile / "DevToolsActivePort"
    # A fresh CI runner's first start of the browser sets its profile up
    # and has taken over 30 s (Edge, ubuntu-latest, 2026-10-31).
    within = 90 if os.environ.get("CI") else 30
    def written():
        # On Windows the browser still holds the file while it writes it.
        try:
            return port_file.exists() and port_file.read_text().split("\n")[0]
        except OSError:
            return False
    port = wait_for(written, within)
    if not port:
        said = profile.parent / "chrome-stderr.txt"
        if said.exists():
            print("the browser's last words:", file=sys.stderr)
            print(said.read_text(errors="replace")[-3000:], file=sys.stderr)
        sys.exit("Chrome never opened its DevTools port")
    return f"http://127.0.0.1:{port}"


def targets(base):
    with urllib.request.urlopen(f"{base}/json/list", timeout=10) as reply:
        return json.load(reply)


def open_tab(base, url):
    """A new tab, then sent to `url`. Chrome's `/json/new?<url>` left the tab
    on about:blank now and then, which is where the link-click checks found
    no link; navigating it explicitly does not."""
    request = urllib.request.Request(f"{base}/json/new?about:blank", method="PUT")
    with urllib.request.urlopen(request, timeout=10) as reply:
        tab = Target(json.load(reply)["webSocketDebuggerUrl"])
    tab.call("Page.navigate", url=url)
    return tab


# A new tab can say "complete" for the blank page it starts on, before the
# page asked for has arrived; a click then found no link. So wait for the
# page by its address, and for what is wanted on it, asking again across
# the navigation (which ends the blank page's context mid-question).
def loaded(tab, want, selector="", seconds=20):
    probe = (f"location.pathname.endsWith({json.dumps(want)}) && document.readyState === 'complete'"
             + (f" && !!document.querySelector({json.dumps(selector)})" if selector else ""))
    deadline = time.time() + seconds
    last = None
    while time.time() < deadline:
        try:
            last = tab.evaluate(probe)
            if last is True:
                return True
        except RuntimeError as e:
            last = e
        time.sleep(0.1)
    try:
        where = tab.evaluate("location.href + ' ' + document.readyState")
    except RuntimeError as e:
        where = e
    raise RuntimeError(f"{want} never loaded (last answer {last!r}; the tab is at {where})")


def save_dialogs():
    """The browser's Save dialogs open on the X display, or None where they
    cannot be looked for (no display, no xdotool: macOS, headless)."""
    if not (os.environ.get("DISPLAY") and shutil.which("xdotool")):
        return None
    found = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "^(Save File|Save As)$"],
                           capture_output=True, text=True, timeout=20).stdout.split()
    return found


def human_click(base, origin, link):
    """Opens the page in a tab and presses the mouse on one link, as a
    person does: a real pointer, at the link's place, not a script's click."""
    tab = open_tab(base, f"{origin}/page.html")
    loaded(tab, "/page.html", f"[id={json.dumps(link)}]")
    # Scrolled to first, as a person scrolls to a link below the fold.
    x, y = tab.evaluate(f"""(() => {{ const el = document.getElementById({json.dumps(link)});
      el.scrollIntoView({{block: 'center'}}); const r = el.getBoundingClientRect();
      return [r.left + 5, r.top + r.height / 2]; }})()""")
    click_at(tab, x, y)
    return tab


def click(base, origin, link):
    """Opens the page in a tab of its own and clicks one link, as a person."""
    tab = open_tab(base, f"{origin}/page.html")
    loaded(tab, "/page.html", f"[id={json.dumps(link)}]")
    tab.evaluate(f"document.getElementById({json.dumps(link)}).click()", gesture=True)
    return tab


# What the page shows of dm's panel: whether it is visible, what it says,
# and where it is, read from the page through its open shadow root.
PANEL_STATE = """(() => {
  const host = document.querySelector('dm-panel');
  if (!host) return null;
  const wrap = host.shadowRoot && host.shadowRoot.querySelector('.wrap');
  const r = wrap ? wrap.getBoundingClientRect() : host.getBoundingClientRect();
  // The qualities; reference A's "Download all" line before them is apart.
  const lines = [...host.shadowRoot.querySelectorAll('.item:not(.all)')];
  const all = host.shadowRoot.querySelector('.item.all');
  const at = lines.map(l => { const b = l.getBoundingClientRect(); return [b.left + 30, b.top + b.height / 2]; });
  return { shown: getComputedStyle(host).display !== 'none',
           title: host.shadowRoot.querySelector('.title').textContent,
           sub: host.shadowRoot.querySelector('.sub').textContent,
           tip: host.shadowRoot.querySelector('.go').getAttribute('title') || '',
           caret: !host.shadowRoot.querySelector('.caret').hidden,
           lines: lines.map(l => l.textContent), at, all: all ? all.textContent : null,
           x: r.left, y: r.top, w: r.width, h: r.height };
})()"""

VIDEO_RECT = """(() => { const r = document.getElementById('v').getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height }; })()"""


def mouse(tab, kind, x, y):
    params = {"type": kind, "x": x, "y": y, "button": "left"}
    if kind in ("mousePressed", "mouseReleased"):
        params["clickCount"] = 1
    tab.call("Input.dispatchMouseEvent", **params)


def hover(tab, rect):
    """Moves the pointer onto the middle of a player, the way a person does."""
    for step in range(4):
        mouse(tab, "mouseMoved", rect["x"] + rect["w"] / 2 + step * 7, rect["y"] + rect["h"] / 2)
        time.sleep(0.12)


def panel(tab):
    return tab.evaluate(PANEL_STATE)


def panel_page(base, origin, path):
    tab = open_tab(base, f"{origin}{path}")
    tab.call("Runtime.enable")
    loaded(tab, path.split("?")[0])
    # The page script loads its WebAssembly after document_idle; a page that
    # is busy with its own scripts takes a moment longer.
    wait_for(lambda: tab.evaluate("!!document.querySelector('dm-panel')"), 15)
    time.sleep(0.3)
    return tab


CLOSE_AT = """(() => { const b = document.querySelector('dm-panel').shadowRoot
  .querySelector('.close').getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()"""


GRIP_AT = """(() => { const b = document.querySelector('dm-panel').shadowRoot
  .querySelector('.grip').getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()"""


def click_at(tab, x, y):
    mouse(tab, "mouseMoved", x, y)
    mouse(tab, "mousePressed", x, y)
    mouse(tab, "mouseReleased", x, y)


def click_panel(tab, player=None):
    """Clicks the panel's bar. `player` is the player's rectangle, for a page
    whose player is not this check's own `#v` (a real site's)."""
    state = panel(tab)
    if not (state or {}).get("shown"):
        # It hides itself when the pointer rests away from it (the wait
        # before this click was long enough), so a person would bring it
        # back by pointing at the player; the click must not land on nothing.
        hover(tab, player or tab.evaluate(VIDEO_RECT))
        state = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or state
    x, y = state["x"] + 60, state["y"] + state["h"] / 2
    mouse(tab, "mouseMoved", x, y)
    mouse(tab, "mousePressed", x, y)
    mouse(tab, "mouseReleased", x, y)


def request_checks(base, origin, engine_dl, engine_home, worker):
    """Gaps rows 116 to 119 and 123, in the browser: what the page's own requests carried, the seven
    ways an answer is read, a picture and a sound fetched in ranges, a feed's players, more links."""
    # 116: the file is guarded by a header the page's own request carried.
    tab = panel_page(base, origin, "/guard.html")
    wait_for(lambda: any(r["path"] == "/guarded/guarded1.mp4" and r["auth"] == "Bearer t" for r in requests), 10)
    hover(tab, tab.evaluate(VIDEO_RECT))
    wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    click_panel(tab)
    row = wait_for(lambda: next((r for r in queue_rows(engine_home)
                                 if r.get("url", "").endswith("/guarded/guarded1.mp4")
                                 and r["state"]["kind"] in ("completed", "failed")), None), 40) or {}
    check("a file guarded by a request header is fetched by dm with that header (row 116)",
          row.get("state", {}).get("kind") == "completed" and whole(engine_dl, "guarded1.mp4")
          and any(r["path"] == "/guarded/guarded1.mp4" and is_dm(r["agent"]) and r["auth"] == "Bearer t"
                  for r in requests),
          str(row.get("state")))
    check("and the header is not in the queue file (it is kept apart, like cookies)",
          "Bearer t" not in (engine_home / "queue.json").read_text(), "found in queue.json")

    # 117: eight ways to read an answer; each reaches the panel's list as a page body.
    tab = open_tab(base, "about:blank")
    tab.call("Page.enable")
    tab.call("Page.addScriptToEvaluateOnNewDocument", source=(
        "window.__bodies = []; addEventListener('message', (e) => {"
        " if (e.data && e.data.source === 'dm-page-body') window.__bodies.push(String(e.data.body).length); });"))
    tab.call("Page.navigate", url=f"{origin}/answers.html")
    loaded(tab, "/answers.html")
    wait_for(lambda: tab.evaluate("window.__finished === true"), 20)
    seen = wait_for(lambda: tab.evaluate("window.__bodies.length") >= 8 and tab.evaluate("window.__bodies.length"), 10)
    check("a page reading an answer by text, json, arrayBuffer, blob, body or XHR json/arraybuffer/blob is "
          "heard each time (row 117)", bool(seen), f"{tab.evaluate('window.__bodies.length')} of 8; ways {tab.evaluate('window.__ways')}")

    # 69: an answer the page fetched before the rules arrived is still read.
    tab = panel_page(base, origin, "/early.html")
    tab_id = worker.evaluate(f"chrome.tabs.query({{url: '{origin}/early.html*'}}).then(t => t[t.length - 1].id)")
    last = {}
    def early_listed():
        reply = worker.evaluate(
            f"dmReady.then(m => m.on_page_message({{type: 'dm.formats', src: '', frame: '{origin}/early.html'}},"
            f" {{id: chrome.runtime.id, tab: {{id: {tab_id}}}, url: '{origin}/early.html'}}))")
        last["reply"] = reply
        choices = (reply or {}).get("choices", [])
        return reply if any("early/playlist.m3u8" in (c.get("url") or "") for c in choices) else None
    # As long as the list's other checks (row 118): the list waits on the app, which in
    # human-qa.py's second pass is still finishing the fourteen downloads before this.
    began = time.time()
    reply = wait_for(early_listed, 30)
    check("an answer the page fetched before the rules arrived (document_start) is read once they do, and its "
          "stream is listed (row 69)", bool(reply),
          str(reply)[:400] if reply else f"not listed in {time.time() - began:.0f} s; last reply {str(last.get('reply'))[:300]}")

    # 119: a picture and a sound fetched in ranges become one line.
    tab = panel_page(base, origin, "/pieces.html")
    wait_for(lambda: any(r["path"] == "/media/sound.m4a" for r in requests), 10)
    time.sleep(1)
    tab_id = worker.evaluate(f"chrome.tabs.query({{url: '{origin}/pieces.html*'}}).then(t => t[t.length - 1].id)")
    reply = worker.evaluate(
        f"dmReady.then(m => m.on_page_message({{type: 'dm.formats', src: '', frame: '{origin}/pieces.html'}},"
        f" {{id: chrome.runtime.id, tab: {{id: {tab_id}}}, url: '{origin}/pieces.html'}}))")
    lines = [c for c in (reply or {}).get("choices", []) if c.get("sound")]
    check("a picture and a sound fetched in byte ranges are one line, the larger picture with the sound (row 119)",
          len(lines) == 1 and "high.mp4" in lines[0]["url"] and "sound.m4a" in lines[0]["sound"]
          and "bytestart" not in lines[0]["url"], str(reply)[:400])

    # 118: the feed's second player is told from the first by its React-like props.
    tab = panel_page(base, origin, "/feed.html")
    wait_for(lambda: any(r["path"] == "/files/stream.m3u8" for r in requests), 10)
    second = tab.evaluate("(() => { const r = document.querySelectorAll('video.feedvideo')[1].getBoundingClientRect();"
                          " window.scrollTo(0, window.scrollY + r.top - 40); const s = document.querySelectorAll('video.feedvideo')[1]"
                          ".getBoundingClientRect(); return { x: s.left, y: s.top, w: s.width, h: s.height }; })()")
    time.sleep(1)
    hover(tab, second)
    wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    click_panel(tab, second)
    lines = wait_for(lambda: (panel(tab) or {}).get("lines") or None, 30) or []
    check("the second player of a feed lists its own video's qualities, found by the site's finder (row 118)",
          any("HD" in l for l in lines) and any("SD" in l for l in lines), str(lines))

    # 123: all links reads more than <a>.
    tab = open_tab(base, f"{origin}/morelinks.html")
    loaded(tab, "/morelinks.html")
    time.sleep(1)
    def queued():
        return [r.get("url", "") for r in queue_rows(engine_home) if r.get("queue") == "Main queue"]
    before = set(queued())
    tab_id = worker.evaluate(f"chrome.tabs.query({{url: '{origin}/morelinks.html*'}}).then(t => t[t.length - 1].id)")
    worker.evaluate(
        f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-all-links', pageUrl: '{origin}/morelinks.html',"
        f" frameId: 0}}, {{id: {tab_id}}}))")
    want = ["more-a.bin", "more-area.bin", "more-img.bin", "more-frame.bin", "more-script.bin"]
    got = wait_for(lambda: len([n for n in want if any(u.endswith("/" + n) for u in set(queued()) - before)]) == len(want), 20)
    check("'Download all links' also takes an <area>, an image, a frame and an address written in a script (row 123)",
          bool(got), str([u for u in set(queued()) - before]))


def placement_checks(base, origin):
    """Which player the panel goes on and where, one page for each rule (gaps row 125)."""
    def raised(path):
        tab = panel_page(base, origin, path)
        video = wait_for(lambda: tab.evaluate("document.getElementById('v') ? 1 : 0") and tab.evaluate(VIDEO_RECT), 15)
        hover(tab, video)
        state = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or panel(tab) or {}
        return tab, video, state

    _, _, state = raised("/covered.html")
    check("a player under a dialog over the whole page is not offered (row 125)",
          not state.get("shown"), str(state))
    _, _, state = raised("/tiny.html")
    check("a player smaller than 15 by 10 is passed by (row 125)", not state.get("shown"), str(state))
    tab, video, state = raised("/clipped.html")
    clip = tab.evaluate("(() => { const r = document.getElementById('clip').getBoundingClientRect();"
                        " return { x: r.left, y: r.top, w: r.width, h: r.height }; })()") or {}
    check("on a player its parent cuts, the panel sits in the part that is seen (row 125)",
          state.get("shown") and state.get("x", 0) + state.get("w", 0) <= clip.get("x", 0) + clip.get("w", 0) + 1
          and state.get("x", -1) >= clip.get("x", 0),
          f"panel {state.get('x')},{state.get('y')} {state.get('w')}x{state.get('h')}; seen part {clip}; video {video}")
    tab, video, state = raised("/topbar.html")
    bar = tab.evaluate("document.getElementById('bar').getBoundingClientRect().bottom") or 0
    check("below a site's fixed header over the player's top, the panel goes under the header (row 125)",
          state.get("shown") and state.get("y", -1) >= bar - 1 and state.get("y", 0) <= video.get("y", 0) + video.get("h", 0),
          f"panel y {state.get('y')}; header ends at {bar}; video {video}")


def panel_checks(base, origin, engine_dl, engine_home, worker):
    # A file behind a player's control layer.
    tab = panel_page(base, origin, "/video.html")
    # The page's own video, not yet there on a slow load: waited for, not
    # read as null (which ended the whole check with a TypeError).
    video = wait_for(lambda: tab.evaluate(
        "document.getElementById('v') ? 1 : 0") and tab.evaluate(VIDEO_RECT), 15)
    hover(tab, video)
    shown = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or panel(tab) or {}
    check("hovering a video raises 'Download video', through the player's control layer",
          shown.get("shown") and shown.get("title") == "Download video", str(shown))
    corner = (shown.get("x", -1) >= video["x"] and shown.get("x", 0) + shown.get("w", 0) <= video["x"] + video["w"]
              and video["y"] <= shown.get("y", -1) <= video["y"] + 30)
    check("the panel sits in the player's top right corner", corner,
          f"panel {shown.get('x')},{shown.get('y')} {shown.get('w')}x{shown.get('h')}; video {video}")
    # The player changes size with the window unchanged, as a page laying
    # itself out late does (YouTube's settles after the panel is up): the
    # panel follows it into the new corner without a pointer move.
    tab.evaluate("(() => { const v = document.getElementById('v'); v.style.width = '420px'; v.style.height = '236px'; return 1; })()")
    shrunk = wait_for(lambda: (tab.evaluate(VIDEO_RECT) or {}).get("w", 0) < 500 and tab.evaluate(VIDEO_RECT), 5) or {}
    followed = wait_for(lambda: (lambda p: p.get("shown") and p.get("x", -1) >= shrunk.get("x", 0)
                                 and p.get("x", 0) + p.get("w", 0) <= shrunk.get("x", 0) + shrunk.get("w", 0) + 1
                                 and shrunk.get("y", 0) <= p.get("y", -1) <= shrunk.get("y", 0) + 30 and p)(panel(tab) or {}), 5)
    check("when the player changes size, the panel follows into its new corner, with no pointer move",
          bool(followed), f"panel {panel(tab)}; video now {shrunk}")
    tab.evaluate("(() => { const v = document.getElementById('v'); v.style.width = ''; v.style.height = ''; return 1; })()")
    wait_for(lambda: (tab.evaluate(VIDEO_RECT) or {}).get("w", 0) >= 500, 5)
    # The player moves without changing size, as it does when the page
    # above it folds (YouTube's goes 8 px left and 12 px up a second after
    # its first layout): the panel follows, with no pointer move.
    before = tab.evaluate(VIDEO_RECT) or {}
    tab.evaluate("(() => { document.body.insertAdjacentHTML('afterbegin', '<div id=\"dm-shift\" style=\"height:50px\"></div>'); return 1; })()")
    moved_to = wait_for(lambda: (tab.evaluate(VIDEO_RECT) or {}).get("y", 0) >= before.get("y", 0) + 49 and tab.evaluate(VIDEO_RECT), 5) or {}
    followed = wait_for(lambda: (lambda p: p.get("shown") and moved_to.get("y", 0) <= p.get("y", -1) <= moved_to.get("y", 0) + 30
                                 and moved_to.get("x", 0) <= p.get("x", -1) and p)(panel(tab) or {}), 5)
    check("when the player moves without changing size, the panel follows, with no pointer move",
          bool(followed), f"panel {panel(tab)}; video now {moved_to}, was {before}")
    tab.evaluate("(() => { document.getElementById('dm-shift').remove(); return 1; })()")
    wait_for(lambda: (tab.evaluate(VIDEO_RECT) or {}).get("y", 1e9) <= before.get("y", 0), 5)
    hover(tab, tab.evaluate(VIDEO_RECT))
    shown = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or panel(tab) or {}
    check("it says where the file comes from, on hover", shown.get("tip") == "127.0.0.1", shown.get("tip"))
    check("a file is one quality: no list arrow", not shown.get("caret"), str(shown))
    # Dragged by its grip, as a person drags it: it goes where it is put,
    # stays there for the page, and hands nothing over.
    gx, gy = tab.evaluate(GRIP_AT)
    mouse(tab, "mouseMoved", gx, gy)
    mouse(tab, "mousePressed", gx, gy)
    for step in range(1, 6):
        mouse(tab, "mouseMoved", gx - 30 * step, gy + 16 * step)
        time.sleep(0.05)
    mouse(tab, "mouseReleased", gx - 150, gy + 80)
    moved = panel(tab) or {}
    check("its grip drags it where it is put",
          abs(moved.get("x", 0) - (shown.get("x", 0) - 150)) <= 2
          and abs(moved.get("y", 0) - (shown.get("y", 0) + 80)) <= 2,
          f"from {shown.get('x')},{shown.get('y')} to {moved.get('x')},{moved.get('y')}")
    mouse(tab, "mouseMoved", 5, 5)
    wait_for(lambda: not (panel(tab) or {}).get("shown"), 8)
    hover(tab, video)
    again = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or {}
    check("and it comes back where it was put, for the page",
          abs(again.get("x", 0) - moved.get("x", -9)) <= 2 and abs(again.get("y", 0) - moved.get("y", -9)) <= 2,
          f"{again.get('x')},{again.get('y')} against {moved.get('x')},{moved.get('y')}")
    check("a drag hands nothing over", not (engine_dl / "clip.mp4").exists(), str(engine_dl))
    # The page's own script pressing the panel's button: not a person, so
    # nothing is handed over (the panel's root is open to the page).
    tab.evaluate("document.querySelector('dm-panel').shadowRoot.querySelector('.go').click()")
    time.sleep(2)
    check("a click a page script makes on the panel hands nothing over",
          not (engine_dl / "clip.mp4").exists() and (panel(tab) or {}).get("title") == "Download video",
          str(panel(tab)))
    click_panel(tab)
    landed = wait_for(lambda: whole(engine_dl, "clip.mp4"), 60)
    check("a click on it hands the file to dm, which fetches it", landed, str(engine_dl / "clip.mp4"))
    said = wait_for(lambda: (panel(tab) or {}).get("title") == "Sent to dm" and panel(tab), 5) or panel(tab) or {}
    check("and the panel says so", said.get("title") == "Sent to dm", str(said))

    # Moving away hides it.
    mouse(tab, "mouseMoved", 5, 5)
    gone = wait_for(lambda: not (panel(tab) or {}).get("shown"), 8)
    check("moving the pointer away hides it", gone, str(panel(tab)))

    # The close button: hidden, and not offered again for that player.
    hover(tab, video)
    wait_for(lambda: (panel(tab) or {}).get("shown"), 3)
    state = panel(tab)
    if state:
        x, y = state["x"] + state["w"] - 16, state["y"] + state["h"] / 2
        mouse(tab, "mouseMoved", x, y)
        mouse(tab, "mousePressed", x, y)
        mouse(tab, "mouseReleased", x, y)
    # × opens its menu; "Hide for now" is the first line.
    menu = wait_for(lambda: (panel(tab) or {}).get("at") and panel(tab), 5) or {}
    if menu.get("at"):
        click_at(tab, *menu["at"][0])
    time.sleep(0.3)
    hover(tab, video)
    time.sleep(0.4)
    check("its close button's 'Hide for now' hides it for that player", not (panel(tab) or {}).get("shown"), str(panel(tab)))

    # Starting playback raises it.
    tab = panel_page(base, origin, "/video.html")
    tab.evaluate("document.getElementById('v').dispatchEvent(new Event('play'))")
    check("a video starting to play raises it",
          wait_for(lambda: (panel(tab) or {}).get("shown"), 3), str(panel(tab)))

    # A streaming player has no file: the page goes to the app.
    tab = panel_page(base, origin, "/player.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    shown = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 5) or {}
    check("a streaming (blob:) player is offered too, as a page with qualities to list",
          "through the dm app" in shown.get("tip", "") and shown.get("caret"), str(shown))
    click_panel(tab)
    # Only YouTube's page is read for qualities (by dm itself); another
    # site's blob player with no stream loaded is handed over as the page.
    player = f"{origin}/player.html"
    row = wait_for(lambda: next((r for r in queue_rows(engine_home) if r.get("url") == player), None), 20) or {}
    check("a streaming (blob:) player's click hands dm the page when no stream of it was loaded",
          bool(row), str(row)[:300])

    # A player whose page loads an HLS playlist: the
    # stream the page's traffic showed is what the panel offers. One
    # choice goes at once.
    tab = panel_page(base, origin, "/stream.html")
    wait_for(lambda: any(r["path"] == "/files/stream.m3u8" for r in requests), 10)
    hover(tab, tab.evaluate(VIDEO_RECT))
    wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    click_panel(tab)
    stream = wait_for(lambda: next((r for r in queue_rows(engine_home)
                                    if r.get("url", "").endswith("/files/stream.m3u8")
                                    and r["state"]["kind"] in ("completed", "failed")), None), 40) or {}
    check("when a page loaded an HLS playlist, the panel hands dm that stream",
          stream.get("state", {}).get("kind") == "completed"
          and any(r["path"] == "/files/seg-1.ts" and is_dm(r["agent"]) for r in requests),
          # Not finished: where the row was, what else dm was busy with, and
          # which pieces it asked for (gaps row 141, "the slow part is in the app").
          str(stream.get("state")) if stream else
          f"rows: {[(r.get('url', '').rsplit('/', 1)[-1], r['state'].get('kind'), r.get('queue')) for r in queue_rows(engine_home)][-8:]}; "
          f"dm asked for: {[r['path'] for r in requests if is_dm(r['agent']) and ('seg-' in r['path'] or 'stream' in r['path'])]}")

    # A page that loads one playlist: its click hands the stream over at once,
    # and no list of qualities is read from the page.
    tab = panel_page(base, origin, "/both.html")
    wait_for(lambda: any(r["path"] == "/files/both.m3u8" for r in requests), 10)
    hover(tab, tab.evaluate(VIDEO_RECT))
    wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    click_panel(tab)
    both = wait_for(lambda: next((r for r in queue_rows(engine_home)
                                  if r.get("url", "").endswith("/files/both.m3u8")
                                  and r["state"]["kind"] in ("completed", "failed")), None), 40) or {}
    check("a page whose stream was loaded hands dm the stream", both.get("state", {}).get("kind") == "completed",
          str(both.get("state")))

    # A master playlist the page loaded: read by the extension in the browser,
    # its line named by the best picture it lists (gaps row 64).
    tab = panel_page(base, origin, "/master.html")
    wait_for(lambda: any(r["path"] == "/files/master.m3u8" for r in requests), 10)
    kept = wait_for(lambda: (lambda all_: "720p HLS stream" in all_ and all_)(
        worker.evaluate("chrome.storage.session.get(null).then(all => JSON.stringify(all))") or ""), 15)
    check("a master playlist the page loaded names its stream's line by its best picture",
          bool(kept), str(kept)[:300])

    # Decoration is left alone.
    tab = panel_page(base, origin, "/tiny.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    time.sleep(0.5)
    check("an avatar-sized video gets no panel", not (panel(tab) or {}).get("shown"), str(panel(tab)))

    # A site whose policy forbids WebAssembly. Chrome holds an extension's
    # page script to the page's policy for WebAssembly, so the panel cannot
    # load there — the one place it is missing. It fails quietly, and the
    # right-click menu, which needs no page script, still hands the video.
    tab = panel_page(base, origin, "/strict.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    time.sleep(0.5)
    errors = tab.logged("error")
    # A plain-JavaScript panel takes its place there (content.js), over the video: the
    # button, the list from the same messages, and the confirmation.
    lite = wait_for(lambda: (hover(tab, tab.evaluate(VIDEO_RECT)) or True) and tab.evaluate(
        "(() => { const h = document.querySelector('dm-panel-lite'); return !!h && h.style.display !== 'none'"
        " && getComputedStyle(h).visibility === 'visible'; })()"), 8)
    check("on a site whose policy forbids WebAssembly the full panel stays away, quietly, and the plain one is shown",
          not (panel(tab) or {}).get("shown") and bool(lite) and not [e for e in errors if "dm" in e],
          "; ".join(errors)[:300])
    worker.evaluate(
        f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-media', srcUrl: '{origin}/files/strict.mp4',"
        f" pageUrl: '{origin}/strict.html'}}, {{}}))")
    check("and right-click 'Download video with dm' still hands its video",
          wait_for(lambda: whole(engine_dl, "strict.mp4"), 60))

    # A player embedded from another origin, in a frame of its own process.
    tab = panel_page(base, origin, "/framed.html")
    frame = wait_for(lambda: next((Target(t["webSocketDebuggerUrl"]) for t in targets(base)
                                   if t["type"] == "iframe" and t["url"].endswith("/framevideo.html")), None), 15)
    if frame:
        frame.call("Runtime.enable")
        wait_for(lambda: frame.evaluate("!!document.querySelector('dm-panel')"), 15)
        box = tab.evaluate("(() => { const r = document.getElementById('f').getBoundingClientRect(); return {x: r.left, y: r.top}; })()")
        inner = frame.evaluate(VIDEO_RECT)
        hover(tab, {"x": box["x"] + inner["x"], "y": box["y"] + inner["y"], "w": inner["w"], "h": inner["h"]})
        shown = wait_for(lambda: (panel(frame) or {}).get("shown") and panel(frame), 5) or {}
    else:
        shown = {}
    check("a player embedded from another origin gets the panel, inside its frame",
          shown.get("title") == "Download video", str(shown) if frame else "no out-of-process frame")
    if shown:
        click_at(tab, box["x"] + shown["x"] + 60, box["y"] + shown["y"] + shown["h"] / 2)
    check("and its click lands the frame's video in dm", wait_for(lambda: whole(engine_dl, "framed.mp4"), 60))

    # A video dm is refused but the browser can fetch: asked of dm by the
    # panel, it is dm's. dm takes it and says if it fails; the browser is
    # never started on it behind the person's back.
    tab = panel_page(base, origin, "/privatevideo.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    if wait_for(lambda: (panel(tab) or {}).get("shown"), 5):
        click_panel(tab)
    said = wait_for(lambda: (panel(tab) or {}).get("title") == "Sent to dm" and panel(tab), 30) or panel(tab) or {}
    check("a video dm may be refused is still handed to dm, and the panel says so",
          said.get("title") == "Sent to dm", str(said))
    time.sleep(2)
    check("and the browser does not download it",
          not (engine_dl.parent / "browser-downloads" / "private-clip.mp4").exists())

    # A player that keeps its video in a web component's shadow root.
    tab = panel_page(base, origin, "/shadow.html")
    rect = tab.evaluate("(() => { const r = document.querySelector('dm-test-player').shadowRoot"
                        ".querySelector('video').getBoundingClientRect(); return {x: r.left, y: r.top, w: r.width, h: r.height}; })()")
    hover(tab, rect)
    shown = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 5) or {}
    check("a video inside a web component's shadow root gets the panel too",
          shown.get("title") == "Download video", str(shown))
    if shown:
        click_panel(tab)
    check("and its click lands the file in dm", wait_for(lambda: whole(engine_dl, "inner.mp4"), 60))

    # × offers "Hide for now" and "Never on" the site. Last, because every page here is on the one site.
    tab = panel_page(base, origin, "/video.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    click_at(tab, *tab.evaluate(CLOSE_AT))
    menu = wait_for(lambda: len((panel(tab) or {}).get("lines") or []) == 2 and panel(tab), 5) or panel(tab) or {}
    check("× offers 'Hide for now' and 'Never on' the site",
          menu.get("lines") == ["Hide for now", "Never on 127.0.0.1"], str(menu.get("lines")))
    if menu.get("at"):
        click_at(tab, *menu["at"][1])
    gone = wait_for(lambda: not (panel(tab) or {}).get("shown"), 5)
    tab = panel_page(base, origin, "/video.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    time.sleep(1)
    check("'Never on' keeps the panel off the site's other pages",
          gone and not (panel(tab) or {}).get("shown"), str(panel(tab)))
    # What the popup's switch does: the site taken off the list. The page
    # follows the list, so no reload is needed.
    worker.evaluate("chrome.storage.local.set({'dm.panel.off': []}).then(() => 1)")
    hover(tab, tab.evaluate(VIDEO_RECT))
    back = wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    check("and letting the site back brings the panel back, without a reload", bool(back), str(panel(tab)))


def plays(path):
    """What a downloaded video holds, read from its own structure
    (`media_check.py`): (seconds, height), or None when it is not a whole
    video."""
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
    import media_check
    info = media_check.inspect(str(path))
    if info["problems"] or not info["duration"]:
        return None
    heights = [t.get("height") for t in info["tracks"] if t.get("kind") == "video"]
    return round(float(info["duration"]), 1), (heights[0] if heights else None)


def queue_rows(home):
    try:
        return json.loads((home / "queue.json").read_text())
    except (OSError, ValueError):
        return []


def main():
    if not (EXT / "pkg" / "dm_catcher_bg.wasm").exists():
        sys.exit("no built extension at dist/chrome — run: sh scripts/package.sh")
    if not BINARY.exists():
        sys.exit(f"no dm binary at {BINARY} — run: (cd desktop && cargo build -p dm-desktop)")
    chrome = find_chrome()
    manifest = json.loads((EXT / "manifest.json").read_text())
    der = base64.b64decode(manifest["key"])
    expected_id = "".join(chr(ord("a") + int(c, 16)) for c in hashlib.sha256(der).hexdigest()[:32])

    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-chrome-check-"))
    # DM_CHROME_PROFILE names a profile kept between runs (a test account that stays
    # signed in); otherwise the profile is new and goes with the run.
    kept_profile = os.environ.get("DM_CHROME_PROFILE")
    profile = pathlib.Path(kept_profile) if kept_profile else work / "profile"
    # A kept profile remembers where its unpacked extension is, so that stays one place.
    ext = pathlib.Path(kept_profile + "-extension") if kept_profile else work / "extension"
    browser_dl, engine_home, engine_dl = work / "browser-downloads", work / "dm-home", work / "dm-downloads"
    for d in (profile / "Default", browser_dl, engine_home, engine_dl):
        d.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(ext, ignore_errors=True)
    shutil.copytree(EXT, ext)
    (profile / "Default" / "Preferences").write_text(json.dumps({
        "download": {"default_directory": str(browser_dl), "prompt_for_download": False,
                     "directory_upgrade": True},
        "savefile": {"default_directory": str(browser_dl)},
        # Google Chrome blocks an unpacked extension unless Developer mode is on.
        "extensions": {"ui": {"developer_mode": True}},
    }))

    # The app, copied out of the checkout, with a home and settings of its own.
    host = engine_home / ("dm-desktop.exe" if sys.platform == "win32" else "dm-desktop")
    shutil.copy2(BINARY, host)
    host.chmod(0o755)
    if sys.platform == "win32":
        # A freshly copied program is scanned the first time Windows runs it,
        # which took 8 s here; the first click's host would then outlast the
        # extension's wait and the browser would keep that download. Run once
        # now so the scan is not part of the check.
        subprocess.run([str(host), "--version"], capture_output=True, timeout=120)
    (engine_home / "settings.json").write_text(json.dumps({
        "download_dir": str(engine_dl), "categorize": False,
        "catch": {"enabled": True, "extensions": ["bin", "pdf"], "defaultsVersion": 1},
        # The dialog the app opens for a panel video or a caught download
        # (Download details, reference A's File Info; on in a new install) is
        # the window tests' to drive; here both go straight to the folder,
        # and no progress or finished window opens over the checks.
        "ask_panel_downloads": False,
        "ask_caught_downloads": False,
        "show_progress": False,
        "show_complete": False,
        "popups_version": 2,
    }))
    (engine_home / "rules.json").write_text(json.dumps({
        "format": 1, "revision": 999, "exclude": {},
        "sites": [{"host": "127.0.0.1",
                   # A request whose JSON answer names the stream (gaps row 69).
                   "answers": [{"url_contains": "/api/early", "keys": ["stream"]}],
                   "finders": [
            {"player": "div.vp-video",
             "id_path": "*.return.return.memoizedProps.coreVideoPlayerMetaData.videoFBID"}]}],
    }))
    if sys.platform == "win32":
        shim = engine_home / "host.bat"
        # A file of its own for each start's stderr (gaps row 141): while one host
        # held a shared host-stderr.txt open, cmd.exe could not open it for the
        # next ("being used by another process") and never started dm, so that
        # hand-over found no app and the browser kept the file, now and then.
        # The name is a folder made for it: cmd.exe seeds %RANDOM% with the
        # second it started, so two hosts started in the same second drew the
        # same number, and making a folder is the step that only one wins. The
        # parent is made here: two cmd.exe making it and their own at once could
        # both be told theirs was made, and one then found the other's file held.
        said_dir = engine_home / "host-stderr"
        said_dir.mkdir(exist_ok=True)
        shim.write_text(
            "@echo off\r\n"
            f"set DM_HOME={engine_home}\r\n"
            # Torrents held off the network: the check's magnets fail at once.
            "set DM_NO_SWARM=1\r\n"
            ":pick\r\n"
            "set DM_SAID=%RANDOM%%RANDOM%\r\n"
            f'mkdir "{said_dir}\\%DM_SAID%" 2>nul || goto pick\r\n'
            f'"{host}" %* 2>>"{said_dir}\\%DM_SAID%\\said.txt"\r\n'
        )
    else:
        shim = engine_home / "host.sh"
        shim.write_text(
            "#!/bin/sh\n"
            f"export DM_HOME='{engine_home}'\n"
            "export DM_NO_SWARM=1\n"
            f"exec '{host}' \"$@\" 2>>'{engine_home}/host-stderr.txt'\n"
        )
        shim.chmod(0o755)
    hosts = profile / "NativeMessagingHosts"
    hosts.mkdir(exist_ok=True)
    registration = hosts / "com.dm.engine.json"
    host_manifest = json.dumps({
        "name": "com.dm.engine", "description": "dm (chrome-check)", "path": str(shim),
        "type": "stdio", "allowed_origins": [f"chrome-extension://{expected_id}/"],
    })
    registration.write_text(host_manifest)
    # Brave and Edge read host manifests from their own folder in the real
    # home, whatever profile they run: the folder `dm --register` writes. For
    # the length of the check dm's manifest there names this copy (so Edge
    # finds this check's host, not the installed dm, and "no host" is true
    # when it is removed); whatever was there is put back afterwards, as
    # firefox-check does for Firefox.
    borrowed = []
    where = str(chrome).lower()
    home = pathlib.Path.home()
    mac = sys.platform == "darwin"
    folders = []
    if "brave" in where:
        folders.append(home / ("Library/Application Support/BraveSoftware/Brave-Browser" if mac
                               else ".config/BraveSoftware/Brave-Browser"))
    if "edge" in where:
        folders.append(home / ("Library/Application Support/Microsoft Edge" if mac else ".config/microsoft-edge"))
        # Edge falls back to Chrome's folder for a host it does not find in its own.
        folders.append(home / ("Library/Application Support/Google/Chrome" if mac else ".config/google-chrome"))
    # Windows browsers find a host only through the registry: the key's
    # default value names the manifest. What was there (an installed dm's)
    # is read now and put back after.
    registry_keys = []
    if sys.platform == "win32":
        import winreg
        subkeys = []
        if "edge" in where:
            subkeys.append(r"Software\Microsoft\Edge\NativeMessagingHosts\com.dm.engine")
        if "brave" in where:
            subkeys.append(r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.dm.engine")
        if "chrome" in where and "edge" not in where:
            subkeys.append(r"Software\Google\Chrome\NativeMessagingHosts\com.dm.engine")
        # The person's own entry is written down the first time it is seen, outside
        # this run's folders: a run stopped before it could put things back leaves
        # ours in the key, and the next run must not take that for the person's, or
        # delete the key and with it the registration of the dm they installed.
        backup_file = pathlib.Path.home() / ".cache" / "dm-chrome-check-registry.json"
        try:
            backup = json.loads(backup_file.read_text())
        except (OSError, ValueError):
            backup = {}
        installed = pathlib.Path(os.environ.get("LOCALAPPDATA", "")) / "dm/NativeMessagingHosts/chromium/com.dm.engine.json"
        for subkey in subkeys:
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, subkey) as k:
                    was_value = winreg.QueryValueEx(k, "")[0]
            except OSError:
                was_value = None
            if was_value and "dm-chrome-check" in was_value:
                # An earlier run stopped before it could put things back left our own.
                was_value = backup.get(subkey) or (str(installed) if installed.is_file() else None)
            elif was_value:
                backup[subkey] = was_value
                backup_file.parent.mkdir(parents=True, exist_ok=True)
                backup_file.write_text(json.dumps(backup))
            registry_keys.append((subkey, was_value))
            with winreg.CreateKey(winreg.HKEY_CURRENT_USER, subkey) as k:
                winreg.SetValueEx(k, "", 0, winreg.REG_SZ, str(registration))
        folders = []
    for folder in folders:
        path = folder / "NativeMessagingHosts/com.dm.engine.json"
        was = path.read_bytes() if path.exists() else None
        # What an earlier run that was stopped before it could put things back
        # left is not the person's own: kept as "what was there", it would be
        # put back for good, and their browser would hand every download to a
        # host in a temporary folder that is gone.
        if was is not None and b"dm (chrome-check)" in was:
            was = None
        borrowed.append((path, was))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(host_manifest)

    server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_address[1]}"

    args = [
        str(chrome), f"--user-data-dir={profile}", f"--load-extension={ext}",
        f"--disable-extensions-except={ext}", "--remote-debugging-port=0",
        "--no-first-run", "--no-default-browser-check", "--disable-gpu",
        # Brave upgrades http:// to https:// first, which a test server on
        # loopback never answers; Chrome ignores the names it does not know.
        # DM_CHROME_DISABLE_FEATURES adds more (a side-by-side run turns off the
        # Manifest V2 phase-out, which disabled reference A's extension in the
        # kept profile).
        "--disable-features=DisableLoadExtensionCommandLineSwitch,HttpsUpgrades,BraveHttpsByDefault"
        + "".join("," + f for f in os.environ.get("DM_CHROME_DISABLE_FEATURES", "").split(",") if f),
        # Lets DevTools press the extension's toolbar button.
        "--enable-unsafe-extension-debugging",
        "--enable-logging=stderr", "--v=0", "about:blank",
    ]
    # DM_KEEP_OTHER_EXTENSIONS=1: the extensions the kept profile already holds stay on (a
    # side-by-side look at another manager's extension), not only the one under test.
    if os.environ.get("DM_KEEP_OTHER_EXTENSIONS"):
        args = [a for a in args if not a.startswith("--disable-extensions-except=")]
    # Chromium will not start as root with its sandbox, which is how a
    # container runs it; anywhere else the sandbox stays on.
    if hasattr(os, "geteuid") and os.geteuid() == 0:
        args.insert(1, "--no-sandbox")
    # Without a display to draw on (an agent's shell, a container without
    # Xvfb): the new headless mode runs extensions, workers and DevTools
    # input events as the windowed browser does.
    if os.environ.get("DM_CHROME_HEADLESS"):
        args.insert(1, "--headless=new")
    if os.environ.get("CI"):
        args += ["--no-sandbox", "--disable-dev-shm-usage"]
    # Google Chrome itself (not Chrome for Testing, Chromium, Brave or Edge)
    # has ignored --load-extension since 137: there the extension is loaded
    # over the DevTools pipe (Extensions.loadUnpacked), the port kept for the
    # rest of the check.
    pipe = None
    if "google/chrome/" in str(chrome).lower() or "google chrome.app" in str(chrome).lower():
        to_chrome, from_us = os.pipe()
        to_us, from_chrome = os.pipe()
        os.set_inheritable(to_chrome, True)
        os.set_inheritable(from_chrome, True)
        args.insert(1, "--remote-debugging-pipe")
        pipe = (from_us, to_us, to_chrome, from_chrome)
    # A kept profile also holds the extension's cached worker and compiled scripts, from
    # the build before this one; the login (cookies) is elsewhere and stays.
    if kept_profile:
        for stale in ("Service Worker", "Code Cache", "Extension Scripts", "Sessions"):
            shutil.rmtree(profile / "Default" / stale, ignore_errors=True)
    # A kept profile still holds the last run's port: it is not this browser's.
    (profile / "DevToolsActivePort").unlink(missing_ok=True)
    log = open(work / "chrome-stderr.txt", "wb")
    # Its own process group, so the whole browser can be stopped at the end,
    # but not its own session: on macOS a new session is a new Mach bootstrap
    # namespace, and Chrome's helper processes could not reach their parent
    # ("bootstrap_look_up … Permission denied"; the tab was killed at once).
    def fds():
        os.setpgrp()
        if pipe:
            os.dup2(pipe[2], 3)
            os.dup2(pipe[3], 4)
    if sys.platform == "win32":
        browser = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=log,
                                   creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
    else:
        browser = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=log, preexec_fn=fds,
                                   pass_fds=(3, 4, pipe[2], pipe[3]) if pipe else ())
    if pipe:
        os.close(pipe[2])
        os.close(pipe[3])
        os.write(pipe[0], json.dumps({"id": 1, "method": "Extensions.loadUnpacked",
                                      "params": {"path": str(ext)}}).encode() + b"\0")
        answer, reply = b"", None
        while reply is None:
            chunk = os.read(pipe[1], 65536)
            if not chunk:
                break
            answer += chunk
            *whole_messages, answer = answer.split(b"\0")
            reply = next((m for m in whole_messages if json.loads(m).get("id") == 1), None)
        print("loaded over the DevTools pipe:", (reply or b"no answer").decode(errors="replace")[:200], flush=True)
    print(f"browser: {chrome}\nwork: {work}\n", flush=True)

    try:
        base = devtools(profile)
        if pipe:
            # Loaded over the pipe, the worker is registered but not started
            # until something asks for it: the popup's page does, and goes.
            woke = open_tab(base, f"chrome-extension://{expected_id}/popup.html")
            time.sleep(3)
            woke.evaluate("window.close()")
        # Chrome runs component extensions of its own; dm's is the one whose
        # worker is the loader.
        workers = lambda: [t for t in targets(base) if t["type"] == "service_worker"
                           and t["url"].endswith("/background.js")]
        found = wait_for(lambda: [t for t in workers() if expected_id in t["url"]], 30) or workers()
        if not check("the extension's service worker starts", bool(found)):
            sys.exit(1)
        worker_url = found[0]["url"]
        ext_id = urllib.parse.urlparse(worker_url).hostname
        check("its id is the one the manifest key fixes", ext_id == expected_id,
              f"{ext_id} (expected {expected_id})")
        worker = Target(found[0]["webSocketDebuggerUrl"])
        worker.call("Runtime.enable")
        WATCHED.append(worker)
        worker.call("Runtime.enable")
        worker.call("Log.enable")
        # The target is listed as soon as the script is fetched, before the
        # loader has run.
        wait_for(lambda: worker.evaluate("typeof dmReady") == "object", 15)
        try:
            exports = worker.evaluate("dmReady.then(m => Object.keys(m).filter(k => k.startsWith('on_')).sort().join(','))")
            check("the WebAssembly loads and its handlers are there", bool(exports), exports)
        except RuntimeError as e:
            check("the WebAssembly loads and its handlers are there", False, str(e))
            return
        # A check of real sites only (sites-check.py): the loopback checks
        # below are the usual run's.
        if os.environ.get("DM_SKIP_LOCAL"):
            panel_checks(base, origin, engine_dl, engine_home, worker)
            return
        # Each named, not counted: a run reading "6 of 7" could not say which.
        # An event the browser lacks counts as missing.
        names = ["downloads.onCreated", "downloads.onDeterminingFilename", "contextMenus.onClicked",
                 "action.onClicked", "runtime.onInstalled", "runtime.onStartup",
                 "webRequest.onResponseStarted"]
        missing = worker.evaluate(
            f"{json.dumps(names)}.filter(p => {{ const [a, b] = p.split('.');"
            " try { return !chrome[a][b].hasListeners(); } catch (e) { return true; } })")
        check("every listener is registered on the first turn", missing == [],
              f"{len(names) - len(missing)} of {len(names)}" + (f"; not registered: {missing}" if missing else ""))

        # The host is asked for a hello before the first click (gaps row 141): in
        # `human-qa.py`'s second pass the first click once found no app (the host,
        # a fresh copy each pass, ended at once with nothing on its stderr) and the
        # browser kept the file, which read as a failed hand-over. A host that cannot
        # be reached now fails as that, here, and the clicks below find it warm.
        # The frame's envelope as the extension sends it: without `protocolVersion`
        # the host answers nothing and the browser waits on it for ever.
        hello = ("new Promise(r => chrome.runtime.sendNativeMessage('com.dm.engine',"
                 " {protocolVersion: 1, type: 'engine.hello'},"
                 " m => r(chrome.runtime.lastError ? 'error: ' + chrome.runtime.lastError.message : m)))")
        said = []
        def host_answers():
            said.append(worker.evaluate(hello))
            return isinstance(said[-1], dict)
        check("the extension reaches the app's host before the first click", wait_for(host_answers, 30),
              f"{len(said)} tries; refused first: {sorted({str(s)[:120] for s in said if not isinstance(s, dict)})}")

        # 2. A link click, caught and handed over.
        page = f"{origin}/page.html"
        click(base, origin, "caught.bin")
        landed = wait_for(lambda: whole(engine_dl, "caught.bin"), 60)
        check("a clicked download is fetched by dm into its own folder", landed,
              f"{engine_dl / 'caught.bin'}")
        dm_asked = [r for r in requests if r["path"] == "/files/caught.bin" and is_dm(r["agent"])]
        check("dm fetched it with the page as referrer",
              bool(dm_asked) and all(r["referer"] == page for r in dm_asked),
              f"{len(dm_asked)} request(s) from dm, referer {sorted({r['referer'] for r in dm_asked})}")
        # Waited for, not a fixed pause: the copy is cancelled when the app's
        # answer reaches the extension, and on Windows the native host's first
        # start (a debug build the virus checker reads) took more than the
        # 1.5 s this once allowed, now and then, with dm's file already whole.
        caught_at = time.monotonic()
        browser_copy = lambda: worker.evaluate(
            "chrome.downloads.search({}).then(l => l.filter(d => d.url.endsWith('caught.bin')).map(d => d.state))")
        wait_for(lambda: browser_copy() == [] and not (browser_dl / "caught.bin").exists(), 10)
        left = browser_copy()
        check("the browser's copy is cancelled and erased", left == [] and not (browser_dl / "caught.bin").exists(),
              f"browser list {left}, browser folder {sorted(p.name for p in browser_dl.iterdir())}, "
              f"after {time.monotonic() - caught_at:.1f} s")
        check("the app answered", "present" in (worker.evaluate("dmReady.then(m => m.app_status())") or ""),
              worker.evaluate("dmReady.then(m => m.app_status())"))

        # A name too long for the disk: Chromium would ask where to save it,
        # and its Save dialog stayed open over a download dm had taken.
        long_tab = click(base, origin, LONG)
        def read_or_none(p):
            # The saved name (up to 200 characters) under this folder passes
            # Windows' 260-character limit, which Python's plain open obeys
            # unless long paths are on: the file is listed but cannot be opened.
            # The verbatim prefix reads it. A part file renamed between the
            # listing and the read is None.
            if sys.platform == "win32":
                p = pathlib.Path("\\\\?\\" + str(p.resolve()))
            try:
                return p.read_bytes()
            except OSError:
                return None
        long_in_dm = lambda: [p.name for p in engine_dl.iterdir()
                              if p.name.startswith("verylong") and read_or_none(p) == body(LONG)]
        landed = wait_for(long_in_dm, 60)
        time.sleep(3)
        dialogs = save_dialogs()
        left = worker.evaluate("chrome.downloads.search({}).then(l => l.filter(d => d.url.includes('verylong'))"
                               ".map(d => d.state + ' ' + d.filename.length))")
        check("a file whose name is too long for the disk goes to dm, and the browser asks nothing",
              bool(landed) and left == [] and not dialogs,
              f"dm folder: {[n[:20] + '…' for n in long_in_dm()]}; browser list {left}; "
              f"Save dialogs: {'not looked for (no X display)' if dialogs is None else len(dialogs)}")
        if dialogs:
            subprocess.run(["xdotool", "key", "--window", dialogs[0], "Escape"], check=False)
        long_tab.call("Page.navigate", url="about:blank")

        # A PDF Chromium finishes before it can be held: served fast, it is
        # complete by the time the extension asks (or completes despite the
        # pause), where a .bin waits for its danger check. It goes to dm all
        # the same, and the browser's whole copy is removed.
        # Clicked three times, since the race does not always go one way.
        pdfs = lambda folder: sorted(p.name for p in folder.iterdir() if p.name.startswith("small") and p.name.endswith(".pdf"))
        for _ in range(3):
            before = len(pdfs(engine_dl))
            click(base, origin, "small.pdf")
            wait_for(lambda: len(pdfs(engine_dl)) > before, 30)
        time.sleep(2)
        in_dm = pdfs(engine_dl)
        check("a PDF the browser finished before it could be held is fetched by dm, every time",
              len(in_dm) == 3 and all((engine_dl / n).read_bytes() == body("small.pdf") for n in in_dm),
              f"dm folder: {in_dm}")
        left = worker.evaluate("chrome.downloads.search({}).then(l => l.filter(d => d.url.endsWith('small.pdf')).map(d => d.state))")
        check("and the browser's finished copy is removed, file and list entry",
              left == [] and pdfs(browser_dl) == [],
              f"browser list {left}, browser folder {pdfs(browser_dl)}")

        # A PDF the browser would show in the tab starts no download at all:
        # a person's plain click on its link is the only thing dm can see,
        # and the app takes it by its rules; the tab stays on the page.
        tab = open_tab(base, f"{origin}/page.html")
        loaded(tab, "/page.html", '[id="small-shown.pdf"]')
        # Scrolled into view first: in a short window (Brave's or Edge's under
        # Xvfb) the link was below the fold and the click landed on nothing.
        x, y = tab.evaluate("""(() => { const l = document.getElementById('small-shown.pdf');
          l.scrollIntoView({ block: 'center' }); const r = l.getBoundingClientRect();
          return [r.left + 5, r.top + r.height / 2]; })()""")
        click_at(tab, x, y)
        shown = wait_for(lambda: whole(engine_dl, "small-shown.pdf"), 60)
        stayed = tab.evaluate("location.pathname")
        check("a plain click on a link to a PDF the browser would show in the tab goes to dm",
              shown and stayed == "/page.html", f"tab at {stayed}")
        check("and the browser kept nothing of it", not (browser_dl / "small-shown.pdf").exists())

        # A person clicks, and watches how long dm takes to start and to finish.
        # A small file from a server that holds each connection to a megabyte
        # a second: one connection takes three seconds and more for 3 MB; the
        # connections that join once the first is seen to be slow bring it
        # to about one.
        mark = len(requests)
        began = time.monotonic()
        human_click(base, origin, "paced.bin")
        done = wait_for(lambda: whole(engine_dl, "paced.bin"), 40)
        took = time.monotonic() - began
        ports = {r["port"] for r in requests[mark:]
                 if r["path"].endswith("/paced.bin") and is_dm(r["agent"])}
        check("a small file from a server that holds each connection to 1 MB/s is fetched on helping connections",
              done and took < 2.8 and len(ports) >= 4, f"{took:.2f} s on {len(ports)} of dm's connections")
        # The same size from a fast server: no connection is opened that it
        # did not need (the host's own probe and the app's one), and dm starts
        # at once: the time from the click to dm's first request for it.
        starts = []
        for name in ("quick.bin", "quick2.bin", "quick3.bin"):
            mark = len(requests)
            began = time.monotonic()
            human_click(base, origin, name)
            done = wait_for(lambda: whole(engine_dl, name), 40)
            asked = [r for r in requests[mark:] if r["path"].endswith("/" + name) and is_dm(r["agent"])]
            ports = {r["port"] for r in asked}
            starts.append((min(r["t"] for r in asked) - began) * 1000 if asked else 9999)
            check(f"{name}, from a fast server, is fetched on no more connections than it needs",
                  done and len(ports) <= 2, f"{len(ports)} of dm's connections")
        starts.sort()
        check("dm's first request comes soon after the click (the doorbell, not the half-second poll)",
              starts[1] < (1000 if sys.platform == "win32" else 700), f"{', '.join(f'{s:.0f} ms' for s in starts)} (median {starts[1]:.0f} ms)")

        # 3. Not a type the app catches.
        click(base, origin, "notes.txt")
        kept = wait_for(lambda: whole(browser_dl, "notes.txt"), 60)
        check("a type the app does not list finishes in the browser", kept and not (engine_dl / "notes.txt").exists())

        # 4. The rules are the app's.
        click(base, origin, "kept.zip")
        kept = wait_for(lambda: whole(browser_dl, "kept.zip"), 60)
        check("the app's settings decide: a .zip stays when they list only .bin",
              kept and not (engine_dl / "kept.zip").exists())

        # The popup's "Take every download from this site": a type the
        # app's rules leave is taken all the same, and only while listed.
        worker.evaluate("chrome.storage.local.set({'dm.catch.all': ['127.0.0.1']}).then(() => 'ok')")
        click(base, origin, "forced.txt")
        taken = wait_for(lambda: whole(engine_dl, "forced.txt"), 60)
        check("with 'Take every download from this site' on, a type the rules leave goes to dm",
              taken and not (browser_dl / "forced.txt").exists(),
              f"dm folder: {sorted(p.name for p in engine_dl.iterdir())}; "
              f"browser folder: {sorted(p.name for p in browser_dl.iterdir())}")
        worker.evaluate("chrome.storage.local.remove('dm.catch.all').then(() => 'ok')")

        # The popup's "Pause capturing on this site": a type the app catches
        # stays with the browser while the site is listed, and goes to dm
        # again once it is not.
        worker.evaluate("chrome.storage.local.set({'dm.catch.paused': ['127.0.0.1']}).then(() => 'ok')")
        click(base, origin, "paused.bin")
        kept = wait_for(lambda: whole(browser_dl, "paused.bin"), 60)
        check("with 'Pause capturing on this site' on, a type the app catches stays with the browser",
              kept and not (engine_dl / "paused.bin").exists(),
              f"dm folder: {sorted(p.name for p in engine_dl.iterdir())}")
        worker.evaluate("chrome.storage.local.remove('dm.catch.paused').then(() => 'ok')")
        click(base, origin, "resumed.bin")
        taken = wait_for(lambda: whole(engine_dl, "resumed.bin"), 60)
        check("and goes to dm again once the site is not paused",
              taken and not (browser_dl / "resumed.bin").exists())

        # 4b. A file that is a form's answer: dm sends the same form once and
        # has the file; the browser keeps nothing (gaps row 128).
        tab = open_tab(base, f"{origin}/form.html")
        loaded(tab, "/form.html", "#go")
        tab.evaluate("document.getElementById('go').click()", gesture=True)
        taken = wait_for(lambda: whole(engine_dl, "posted.bin"), 60)
        posts = [r for r in requests if r.get("method") == "POST" and is_dm(r["agent"])]
        check("a file that is a form's answer is fetched by dm sending the form again, once",
              taken and not (browser_dl / "posted.bin").exists() and len(posts) == 1,
              f"dm's posts: {len(posts)}; the browser's: "
              f"{len([r for r in requests if r.get('method') == 'POST' and not is_dm(r['agent'])])}; "
              f"in the browser's folder: {(browser_dl / 'posted.bin').exists()}; dm's row: "
              f"{[(r.get('filename'), r.get('state')) for r in queue_rows(engine_home) if 'export' in r.get('url', '')]}; "
              f"dm folder: {sorted(p.name for p in engine_dl.iterdir() if 'post' in p.name or 'export' in p.name)}; "
              f"dm sent: {[(r.get('body'), r.get('headers')) for r in posts]}")

        # 4c. A download link that opens a tab of its own: dm takes it, and no
        # empty tab is left behind (gaps row 130).
        tab = open_tab(base, f"{origin}/newtab.html")
        loaded(tab, "/newtab.html", "#go")
        before_tabs = {t["id"] for t in targets(base) if t.get("type") == "page"}
        tab.evaluate("document.getElementById('go').click()", gesture=True)
        taken = wait_for(lambda: whole(engine_dl, "newtab.bin"), 60)
        time.sleep(3)
        left = [t.get("url", "") for t in targets(base)
                if t.get("type") == "page" and t["id"] not in before_tabs]
        check("a download link that opens a new tab goes to dm and leaves no empty tab behind",
              taken and not left and not (browser_dl / "newtab.bin").exists(), f"tabs left: {left}")

        # 4d. The seven ways a page opens a magnet link each reach dm (gaps row
        # 108). dm's torrent engine is held off the network in this run
        # (DM_NO_SWARM), so each row fails at once rather than going to the DHT
        # and raising Windows' firewall question for this run's copy of dm.
        tab = open_tab(base, f"{origin}/magnets.html")
        loaded(tab, "/magnets.html", "#m7")
        names = ["plain", "onclick", "button", "blank", "icon", "windowopen", "synthetic"]
        for n in range(1, 8):
            tab.evaluate(f"document.getElementById('m{n}').click()", gesture=True)
            time.sleep(1.0)
        def magnets():
            return {name for r in queue_rows(engine_home) for name in names
                    if r.get("url", "").startswith("magnet:") and f"dn={name}" in r.get("url", "")}
        got = wait_for(lambda: magnets() if len(magnets()) == 7 else None, 30) or magnets()
        check("each of the seven ways a page opens a magnet link reaches dm",
              len(got) == 7, f"missing: {sorted(set(names) - set(got))}")

        # 5. Only the browser can fetch it: dm is refused without the
        # browser's session, so it is handed over again with it, for this
        # download, and dm fetches it; the browser keeps nothing.
        click(base, origin, "private.bin")
        taken = wait_for(lambda: whole(engine_dl, "private.bin"), 60)
        check("a link only the signed-in browser can fetch is fetched by dm with the browser's session",
              taken and not (browser_dl / "private.bin").exists())

        # 6. The context menu's handler.
        worker.evaluate(
            f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-link', linkUrl: '{origin}/files/menu.bin',"
            f" pageUrl: '{page}'}}, {{}}))")
        check("'Download link with dm' hands the link to dm",
              wait_for(lambda: whole(engine_dl, "menu.bin"), 60))

        # Alt-click: a type the app catches, left with the browser.
        tab = open_tab(base, f"{origin}/page.html")
        loaded(tab, "/page.html")
        wait_for(lambda: tab.evaluate("!!document.querySelector('dm-panel')"), 15)
        tab.evaluate("document.getElementById('alt.bin').dispatchEvent(new MouseEvent('click',"
                     " {bubbles: true, cancelable: true, altKey: true}))", gesture=True)
        kept = wait_for(lambda: whole(browser_dl, "alt.bin"), 60)
        check("an Alt-clicked link is left with the browser",
              kept and not (engine_dl / "alt.bin").exists())

        # "Download all links" and "Download selected links". With the
        # app's dialog off, they go straight into the main queue, waiting.
        def queued_main():
            return [r for r in queue_rows(engine_home) if r.get("queue") == "Main queue"]
        before = len(queued_main())
        # The tab this page is in, found by its address (the extension's host
        # permission for every page lets it match tabs by address). "The
        # active tab of the last focused window" was another tab now and then
        # on Windows, an edge:// page the links reader may not enter.
        tab_id = worker.evaluate(
            f"chrome.tabs.query({{url: '{origin}/page.html*'}}).then(t => t[t.length - 1].id)")
        # A link's `download` attribute names its file (gaps row 123): set in
        # the tab whose links are read.
        worker.evaluate(
            f"chrome.scripting.executeScript({{target: {{tabId: {tab_id}}}, func: () => "
            "document.querySelector('a[href$=\"kept.zip\"]').setAttribute('download', 'renamed by the page.zip')"
            "}).then(() => 'ok')")
        worker.evaluate(
            f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-all-links', pageUrl: '{origin}/page.html',"
            f" frameId: 0}}, {{id: {tab_id}}}))")
        # Counted off the page rather than written down: a link added to the
        # page for another check must not fail this one.
        on_page = tab.evaluate("document.links.length")
        got = wait_for(lambda: len(queued_main()) - before >= on_page and len(queued_main()) - before, 20)
        check("'Download all links with dm' hands every link on the page to dm's queue",
              got == on_page, f"{len(queued_main()) - before} queued of {on_page} links")
        # Each link's words on the page go with it as its description (gaps
        # row 94): on this page a link's words are its file's name.
        words = {r.get("url", "").rsplit("/", 1)[-1]: r.get("description", "") for r in queued_main()}
        check("each link's words on the page go with it to dm as its description",
              words.get("notes.txt") == "notes.txt" and words.get("kept.zip") == "kept.zip",
              str({k: words[k] for k in list(words)[:6]}))
        given = {r.get("url", "").rsplit("/", 1)[-1]: r.get("save_name") for r in queued_main()}
        check("a link's download attribute names its file in dm",
              given.get("kept.zip") == "renamed by the page.zip", f"kept.zip: {given.get('kept.zip')!r}")
        before = len(queued_main())
        tab.evaluate("(() => { const r = document.createRange();"
                     " r.setStartBefore(document.querySelectorAll('p')[0]);"
                     " r.setEndAfter(document.querySelectorAll('p')[1]);"
                     " getSelection().removeAllRanges(); getSelection().addRange(r); return 1; })()")
        worker.evaluate(
            f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-selected-links', pageUrl: '{origin}/page.html',"
            f" frameId: 0}}, {{id: {tab_id}}}))")
        got = wait_for(lambda: len(queued_main()) - before >= 2 and len(queued_main()) - before, 20)
        check("'Download selected links with dm' hands only the two selected",
              got == 2, f"{len(queued_main()) - before} queued")

        # 7. The floating "Download video" panel over players.
        panel_checks(base, origin, engine_dl, engine_home, worker)
        placement_checks(base, origin)

        # 7b. Requests the page itself made: headers, answers, pieces, feeds, more links.
        request_checks(base, origin, engine_dl, engine_home, worker)

        # The toolbar popup, opened as the page it is: it says dm is there,
        # "Open dm" brings dm's window up and downloads nothing, and with
        # "Catch downloads" off the browser keeps what dm would have caught.
        popup = open_tab(base, f"chrome-extension://{ext_id}/popup.html")
        loaded(popup, "/popup.html", "#open")
        status_of = "document.getElementById('status').textContent"
        said = wait_for(lambda: (popup.evaluate(status_of) or "").startswith("Connected") and popup.evaluate(status_of), 20)
        check("the toolbar popup says the dm app is there", bool(said), said or popup.evaluate(status_of))
        before = len(queue_rows(engine_home))
        popup.evaluate("document.getElementById('open').click(), 1", gesture=True)
        asked = wait_for(lambda: (engine_home / "inbox" / "show").exists(), 10)
        consumed = wait_for(lambda: not (engine_home / "inbox" / "show").exists(), 15)
        check("its 'Open dm' brings dm's window up", bool(asked or consumed),
              "asked and taken" if consumed else "never asked")
        time.sleep(2)
        check("and downloads nothing", len(queue_rows(engine_home)) == before,
              f"{len(queue_rows(engine_home)) - before} row(s) added")

        popup = open_tab(base, f"chrome-extension://{ext_id}/popup.html")
        loaded(popup, "/popup.html", "#catching")
        catching = lambda: worker.evaluate("chrome.storage.local.get('dm.catching').then(r => String(r['dm.catching']))")
        popup.evaluate("document.getElementById('catching').click(), 1", gesture=True)
        wait_for(lambda: catching() == "false", 5)
        # off.bin is in dm's list already, from "Download all links"; what
        # counts is that this click adds no row of its own.
        off_rows = lambda: len([r for r in queue_rows(engine_home) if r.get("url", "").endswith("/files/off.bin")])
        before_off = off_rows()
        click(base, origin, "off.bin")
        kept = wait_for(lambda: whole(browser_dl, "off.bin"), 60)
        worker.drain(1)
        check("with 'Catch downloads' off, a download dm would catch stays in the browser",
              kept and off_rows() == before_off,
              f"catching: {catching()}; in the browser's folder: {sorted(p.name for p in browser_dl.iterdir())}; "
              f"new dm rows: {off_rows() - before_off}")
        # A popup opened afresh, as a person would open it again: the one
        # opened before the download can be a minute stale by now.
        popup = open_tab(base, f"chrome-extension://{ext_id}/popup.html")
        loaded(popup, "/popup.html", "#catching")
        wait_for(lambda: popup.evaluate("document.getElementById('catching').checked === false"), 5)
        popup.evaluate("document.getElementById('catching').click(), 1", gesture=True)
        check("and switching it back on is kept", wait_for(lambda: catching() == "true", 10), catching())
        # The per-site switch is there; opened as a tab of its own the popup
        # has no site in front, and says so.
        label = wait_for(lambda: popup.evaluate("document.getElementById('catchall-label').textContent"), 5)
        check("the popup has the 'Take every download from this site' switch", bool(label), label)

        # A tab whose page script cannot answer, as one open before dm was
        # installed (Chromium puts page scripts only into pages loaded
        # since): a page still arriving, whose script is not in yet. The
        # links reader is put in when it is asked for, and the menu works.
        stale = open_tab(base, f"{origin}/arriving.html")
        wait_for(lambda: stale.evaluate("document.links.length") == len(ARRIVING), 10)
        stale_tab = worker.evaluate(f"chrome.tabs.query({{}}).then(t => t.filter(x => x.url === '{origin}/arriving.html').map(x => x.id))")
        alive = worker.evaluate(f"chrome.tabs.sendMessage({stale_tab[-1]}, {{type: 'dm.links'}})"
                                ".then(() => 'answered', e => String(e))")
        before = len(queued_main())
        worker.evaluate(
            f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-all-links', pageUrl: '{origin}/arriving.html',"
            f" frameId: 0}}, {{id: {stale_tab[-1]}}}))")
        got = wait_for(lambda: len(queued_main()) - before >= len(ARRIVING) and len(queued_main()) - before, 20)
        check("'Download all links' works on a page with no page script, by putting one in",
              got == len(ARRIVING) and alive != "answered",
              f"asked first: {alive}; {len(queued_main()) - before} queued of {len(ARRIVING)} links")
        stale.call("Page.navigate", url="about:blank")

        # 8. No app: the browser keeps it.
        registration.unlink()
        for path, _ in borrowed:
            path.unlink(missing_ok=True)
        worker.evaluate("dmReady.then(m => m.forget_app())")
        click(base, origin, "absent.bin")
        kept = wait_for(lambda: whole(browser_dl, "absent.bin"), 60)
        status = worker.evaluate("dmReady.then(m => m.app_status())")
        check("with no host, a download it would catch finishes in the browser",
              kept and not (engine_dl / "absent.bin").exists(), status)

        # The popup with no app: it says so, and names the fix.
        popup = open_tab(base, f"chrome-extension://{ext_id}/popup.html")
        loaded(popup, "/popup.html", "#status")
        said = wait_for(lambda: "--register" in (popup.evaluate(status_of) or "") and popup.evaluate(status_of), 20)
        check("with no app, the toolbar popup says so and names the fix", bool(said),
              (said or popup.evaluate(status_of) or "")[:200])

        # 8. Errors.
        worker.drain(1)
        errors = worker.logged("error")
        check("the worker logged no errors", not errors, "; ".join(errors)[:500])
        print("\nwhat the worker said:")
        for line in worker.logged("log", "info", "warning", "error", "debug"):
            print(f"    {line[:200]}")
    finally:
        if sys.platform == "win32":
            subprocess.run(["taskkill", "/PID", str(browser.pid), "/T", "/F"], capture_output=True, check=False)
            browser.wait(timeout=30)
            import winreg
            for subkey, was_value in registry_keys:
                if was_value is not None:
                    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, subkey) as k:
                        winreg.SetValueEx(k, "", 0, winreg.REG_SZ, was_value)
                else:
                    try:
                        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, subkey)
                    except OSError:
                        pass
        else:
            os.killpg(os.getpgid(browser.pid), signal.SIGTERM)
            try:
                browser.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(os.getpgid(browser.pid), signal.SIGKILL)
        for path, was in borrowed:
            if was is not None:
                path.write_bytes(was)
            else:
                path.unlink(missing_ok=True)
        # The app the host started outlives the browser by design.
        if sys.platform == "win32":
            ours = str(host).replace("'", "''")
            subprocess.run(["powershell", "-NoProfile", "-Command",
                            "Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq '" + ours
                            + "' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }"],
                           capture_output=True, check=False)
        else:
            subprocess.run(["pkill", "-f", str(host)], check=False)
        server.shutdown()
        said = "".join(f.read_text(errors="replace") for f in sorted(
            [*engine_home.glob("host-stderr*.txt"), *engine_home.glob("host-stderr/*/said.txt")]))
        if said.strip():
            print(f"\nhost stderr:\n{said[:2000]}")

    failed = [r for r in results if not r[0]]
    print(f"\n{len(results) - len(failed)} of {len(results)} checks passed")
    if failed:
        print(f"kept for reading: {work}")
        sys.exit(1)
    shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
