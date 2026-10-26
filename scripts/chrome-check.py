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


def check(name, ok, detail=""):
    results.append((ok, name, detail))
    print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f" — {detail}" if detail else ""), flush=True)
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


def find_chrome():
    if os.environ.get("CHROME_PATH"):
        return pathlib.Path(os.environ["CHROME_PATH"])
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

    def logged(self, *levels):
        out = []
        for event in self.events:
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


def body(name):
    seed = hashlib.sha256(name.encode()).digest()
    size = 300_000 if name.startswith("small") else SIZE
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
    # player does: what yt-dlp cannot read, the page's traffic shows.
    "/stream.html": (
        '<!doctype html><title>a stream</title><div style="position:relative;margin:40px">'
        '<video id="v" width="640" height="360"></video>' + OVERLAY + '</div>'
        '<script>document.getElementById("v").src = URL.createObjectURL(new MediaSource());'
        'fetch("/files/stream.m3u8");</script>',
        None),
    # The same, on a page the app's yt-dlp can read: both kinds in one list.
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


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        agent = self.headers.get("User-Agent", "")
        requests.append({"path": path, "agent": agent, "referer": self.headers.get("Referer", ""),
                         "range": self.headers.get("Range", "")})
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
        if path in PANEL_PAGES:
            page, csp = PANEL_PAGES[path]
            page = page.encode()
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
                             "alt.bin", "off.bin", "forced.txt", "paused.bin", "resumed.bin", "small.pdf", "small-shown.pdf", "small.bin", LONG))
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
        slow = not agent.startswith("dm/") and not name.startswith("small")
        try:
            for at in range(start, end + 1, 32 * 1024):
                self.wfile.write(data[at:min(at + 32 * 1024, end + 1)])
                if slow:
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


def wait_for(probe, seconds):
    deadline = time.time() + seconds * SLOW
    while time.time() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(0.25)
    return probe()


def whole(folder, name):
    """The file, once it is all there and right."""
    path = folder / name
    return path.exists() and path.read_bytes() == body(name)


def devtools(profile):
    port_file = profile / "DevToolsActivePort"
    port = wait_for(lambda: port_file.exists() and port_file.read_text().split("\n")[0], 30)
    if not port:
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
  const lines = [...host.shadowRoot.querySelectorAll('.item')];
  const at = lines.map(l => { const b = l.getBoundingClientRect(); return [b.left + 30, b.top + b.height / 2]; });
  return { shown: getComputedStyle(host).display !== 'none',
           title: host.shadowRoot.querySelector('.title').textContent,
           sub: host.shadowRoot.querySelector('.sub').textContent,
           tip: host.shadowRoot.querySelector('.go').getAttribute('title') || '',
           caret: !host.shadowRoot.querySelector('.caret').hidden,
           lines: lines.map(l => l.textContent), at,
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


def click_panel(tab):
    state = panel(tab)
    if not (state or {}).get("shown"):
        # It hides itself when the pointer rests away from it (the wait
        # before this click was long enough), so a person would bring it
        # back by pointing at the player; the click must not land on nothing.
        hover(tab, tab.evaluate(VIDEO_RECT))
        state = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or state
    x, y = state["x"] + 60, state["y"] + state["h"] / 2
    mouse(tab, "mouseMoved", x, y)
    mouse(tab, "mousePressed", x, y)
    mouse(tab, "mouseReleased", x, y)


def panel_checks(base, origin, engine_dl, engine_home, worker):
    # A file behind a player's control layer.
    tab = panel_page(base, origin, "/video.html")
    video = tab.evaluate(VIDEO_RECT)
    hover(tab, video)
    shown = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 3) or panel(tab) or {}
    check("hovering a video raises 'Download video', through the player's control layer",
          shown.get("shown") and shown.get("title") == "Download video", str(shown))
    corner = (shown.get("x", -1) >= video["x"] and shown.get("x", 0) + shown.get("w", 0) <= video["x"] + video["w"]
              and video["y"] <= shown.get("y", -1) <= video["y"] + 30)
    check("the panel sits in the player's top right corner", corner,
          f"panel {shown.get('x')},{shown.get('y')} {shown.get('w')}x{shown.get('h')}; video {video}")
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

    # A streaming player has no file: the page goes, for the app's yt-dlp.
    tab = panel_page(base, origin, "/player.html")
    hover(tab, tab.evaluate(VIDEO_RECT))
    shown = wait_for(lambda: (panel(tab) or {}).get("shown") and panel(tab), 5) or {}
    check("a streaming (blob:) player is offered too, as a page with qualities to list",
          "through the dm app" in shown.get("tip", "") and shown.get("caret"), str(shown))
    click_panel(tab)
    # The qualities the video comes in, each with its size.
    listed = wait_for(lambda: len((panel(tab) or {}).get("lines") or []) >= 2 and panel(tab), 40) or panel(tab) or {}
    check("its click lists the qualities the app's yt-dlp found, with sizes",
          listed.get("lines") == ["MP4 1280×720 — 9.5 MB", "M4A audio — 977 KB"], str(listed))
    if listed.get("at"):
        x, y = listed["at"][0]
        mouse(tab, "mouseMoved", x, y)
        mouse(tab, "mousePressed", x, y)
        mouse(tab, "mouseReleased", x, y)
    player = f"{origin}/player.html"
    row = wait_for(lambda: next((r for r in queue_rows(engine_home) if r.get("url") == player), None), 15) or {}
    check("a line picked hands dm the page, not a blob: address, at that quality",
          row.get("quality") == "720/mp4", str(row)[:300])

    # A player whose page loads an HLS playlist that yt-dlp cannot read: the
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
    check("on a site yt-dlp cannot read, the panel hands dm the HLS stream the page loaded",
          stream.get("state", {}).get("kind") == "completed"
          and any(r["path"] == "/files/seg-1.ts" and r["agent"].startswith("dm/") for r in requests),
          str(stream.get("state")))

    # Where yt-dlp reads the page too, both are offered in one list.
    tab = panel_page(base, origin, "/both.html")
    wait_for(lambda: any(r["path"] == "/files/both.m3u8" for r in requests), 10)
    hover(tab, tab.evaluate(VIDEO_RECT))
    wait_for(lambda: (panel(tab) or {}).get("shown"), 5)
    click_panel(tab)
    listed = wait_for(lambda: len((panel(tab) or {}).get("lines") or []) >= 3 and panel(tab), 40) or panel(tab) or {}
    lines = listed.get("lines") or []
    check("where yt-dlp reads the page, its qualities and the page's stream are listed together",
          lines == ["MP4 1280×720 — 9.5 MB", "M4A audio — 977 KB", "HLS stream"], str(lines))
    if "HLS stream" in lines and listed.get("at"):
        x, y = listed["at"][lines.index("HLS stream")]
        mouse(tab, "mouseMoved", x, y)
        mouse(tab, "mousePressed", x, y)
        mouse(tab, "mouseReleased", x, y)
    both = wait_for(lambda: next((r for r in queue_rows(engine_home)
                                  if r.get("url", "").endswith("/files/both.m3u8")
                                  and r["state"]["kind"] in ("completed", "failed")), None), 30) or {}
    check("and the stream's line hands dm the stream", both.get("state", {}).get("kind") == "completed",
          str(both.get("state")))

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
    check("on a site whose policy forbids WebAssembly the panel stays away, quietly",
          not (panel(tab) or {}).get("shown") and not [e for e in errors if "dm" in e],
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
    ext, profile = work / "extension", work / "profile"
    browser_dl, engine_home, engine_dl = work / "browser-downloads", work / "dm-home", work / "dm-downloads"
    for d in (profile / "Default", browser_dl, engine_home, engine_dl):
        d.mkdir(parents=True)
    shutil.copytree(EXT, ext)
    (profile / "Default" / "Preferences").write_text(json.dumps({
        "download": {"default_directory": str(browser_dl), "prompt_for_download": False,
                     "directory_upgrade": True},
        "savefile": {"default_directory": str(browser_dl)},
    }))

    # The app, copied out of the checkout, with a home and settings of its own.
    host = engine_home / "dm-desktop"
    shutil.copy2(BINARY, host)
    host.chmod(0o755)
    (engine_home / "settings.json").write_text(json.dumps({
        "download_dir": str(engine_dl), "categorize": False,
        "catch": {"enabled": True, "extensions": ["bin", "pdf"], "defaultsVersion": 1},
        # The dialog the app opens for a panel video is the window test's to
        # drive; here the panel's videos go straight to the folder.
        "ask_panel_downloads": False,
    }))
    # yt-dlp as it is, except that the player page, which has no real video
    # for it to read, is said to come in two qualities.
    # Found where it is usually installed, on any system: a Linux run fell
    # back to the Mac's /opt/homebrew/bin and the player-page check failed
    # though yt-dlp was in ~/.local/bin.
    real_ytdlp = shutil.which(
        "yt-dlp",
        path=os.pathsep.join([os.environ.get("PATH", ""), str(pathlib.Path.home() / ".local/bin"),
                              "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]),
    ) or "yt-dlp"
    stand_in = engine_home / "yt-dlp"
    stand_in.write_text(
        "#!/bin/sh\n"
        'case "$*" in *-J*player.html*|*-J*both.html*) cat <<\'EOF\'\n'
        + json.dumps({"formats": [
            {"ext": "m4a", "vcodec": "none", "acodec": "mp4a", "abr": 128, "filesize": 1_000_000},
            {"ext": "mp4", "vcodec": "avc1", "acodec": "none", "width": 1280, "height": 720,
             "filesize": 9_000_000},
        ]}) +
        "\nEOF\nexit 0;; esac\n"
        f'exec "{real_ytdlp}" "$@"\n'
    )
    stand_in.chmod(0o755)
    shim = engine_home / "host.sh"
    shim.write_text(
        "#!/bin/sh\n"
        f"export DM_HOME='{engine_home}'\n"
        f"export DM_YTDLP='{stand_in}'\n"
        f"exec '{host}' \"$@\" 2>>'{engine_home}/host-stderr.txt'\n"
    )
    shim.chmod(0o755)
    hosts = profile / "NativeMessagingHosts"
    hosts.mkdir()
    registration = hosts / "com.dm.engine.json"
    host_manifest = json.dumps({
        "name": "com.dm.engine", "description": "dm (chrome-check)", "path": str(shim),
        "type": "stdio", "allowed_origins": [f"chrome-extension://{expected_id}/"],
    })
    registration.write_text(host_manifest)
    # Brave reads host manifests only from its own folder in the real home,
    # whatever profile it runs: the folder `dm --register` writes. For the
    # length of the check dm's manifest there names this copy; whatever was
    # there is put back afterwards, as firefox-check does for Firefox.
    borrowed = None
    if "brave" in str(chrome).lower():
        support = (pathlib.Path.home() / "Library/Application Support" if sys.platform == "darwin"
                   else pathlib.Path.home() / ".config")
        borrowed = support / "BraveSoftware/Brave-Browser/NativeMessagingHosts/com.dm.engine.json"
        borrowed_was = borrowed.read_bytes() if borrowed.exists() else None
        borrowed.parent.mkdir(parents=True, exist_ok=True)
        borrowed.write_text(host_manifest)

    server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_address[1]}"

    args = [
        str(chrome), f"--user-data-dir={profile}", f"--load-extension={ext}",
        f"--disable-extensions-except={ext}", "--remote-debugging-port=0",
        "--no-first-run", "--no-default-browser-check", "--disable-gpu",
        # Brave upgrades http:// to https:// first, which a test server on
        # loopback never answers; Chrome ignores the names it does not know.
        "--disable-features=DisableLoadExtensionCommandLineSwitch,HttpsUpgrades,BraveHttpsByDefault",
        # Lets DevTools press the extension's toolbar button.
        "--enable-unsafe-extension-debugging",
        "--enable-logging=stderr", "--v=0", "about:blank",
    ]
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
    log = open(work / "chrome-stderr.txt", "wb")
    # Its own process group, so the whole browser can be stopped at the end,
    # but not its own session: on macOS a new session is a new Mach bootstrap
    # namespace, and Chrome's helper processes could not reach their parent
    # ("bootstrap_look_up … Permission denied"; the tab was killed at once).
    browser = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=log, preexec_fn=os.setpgrp)
    print(f"browser: {chrome}\nwork: {work}\n", flush=True)

    try:
        base = devtools(profile)
        # Chrome runs component extensions of its own; dm's is the one whose
        # worker is the loader.
        workers = lambda: [t for t in targets(base) if t["type"] == "service_worker"
                           and t["url"].endswith("/background.js")]
        found = wait_for(lambda: [t for t in workers() if expected_id in t["url"]], 30) or workers()
        if not check("the extension's service worker starts", bool(found)):
            return
        worker_url = found[0]["url"]
        ext_id = urllib.parse.urlparse(worker_url).hostname
        check("its id is the one the manifest key fixes", ext_id == expected_id,
              f"{ext_id} (expected {expected_id})")
        worker = Target(found[0]["webSocketDebuggerUrl"])
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

        # 2. A link click, caught and handed over.
        page = f"{origin}/page.html"
        click(base, origin, "caught.bin")
        landed = wait_for(lambda: whole(engine_dl, "caught.bin"), 60)
        check("a clicked download is fetched by dm into its own folder", landed,
              f"{engine_dl / 'caught.bin'}")
        dm_asked = [r for r in requests if r["path"] == "/files/caught.bin" and r["agent"].startswith("dm/")]
        check("dm fetched it with the page as referrer",
              bool(dm_asked) and all(r["referer"] == page for r in dm_asked),
              f"{len(dm_asked)} request(s) from dm, referer {sorted({r['referer'] for r in dm_asked})}")
        time.sleep(1.5)
        left = worker.evaluate("chrome.downloads.search({}).then(l => l.filter(d => d.url.endsWith('caught.bin')).map(d => d.state))")
        check("the browser's copy is cancelled and erased", left == [] and not (browser_dl / "caught.bin").exists(),
              f"browser list {left}, browser folder {sorted(p.name for p in browser_dl.iterdir())}")
        check("the app answered", "present" in (worker.evaluate("dmReady.then(m => m.app_status())") or ""),
              worker.evaluate("dmReady.then(m => m.app_status())"))

        # A name too long for the disk: Chromium would ask where to save it,
        # and its Save dialog stayed open over a download dm had taken.
        long_tab = click(base, origin, LONG)
        long_in_dm = lambda: [p.name for p in engine_dl.iterdir()
                              if p.name.startswith("verylong") and p.read_bytes() == body(LONG)]
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
        x, y = tab.evaluate("""(() => { const r = document.getElementById('small-shown.pdf')
          .getBoundingClientRect(); return [r.left + 5, r.top + r.height / 2]; })()""")
        click_at(tab, x, y)
        shown = wait_for(lambda: whole(engine_dl, "small-shown.pdf"), 60)
        stayed = tab.evaluate("location.pathname")
        check("a plain click on a link to a PDF the browser would show in the tab goes to dm",
              shown and stayed == "/page.html", f"tab at {stayed}")
        check("and the browser kept nothing of it", not (browser_dl / "small-shown.pdf").exists())

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
        # The page just opened is the active tab; without the tabs permission
        # dm cannot look tabs up by address, and does not need to.
        tab_id = worker.evaluate("chrome.tabs.query({active: true, lastFocusedWindow: true}).then(t => t[0].id)")
        worker.evaluate(
            f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-all-links', pageUrl: '{origin}/page.html',"
            f" frameId: 0}}, {{id: {tab_id}}}))")
        # Counted off the page rather than written down: a link added to the
        # page for another check must not fail this one.
        on_page = tab.evaluate("document.links.length")
        got = wait_for(lambda: len(queued_main()) - before >= on_page and len(queued_main()) - before, 20)
        check("'Download all links with dm' hands every link on the page to dm's queue",
              got == on_page, f"{len(queued_main()) - before} queued of {on_page} links")
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
        if borrowed is not None:
            borrowed.unlink(missing_ok=True)
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
        os.killpg(os.getpgid(browser.pid), signal.SIGTERM)
        try:
            browser.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(os.getpgid(browser.pid), signal.SIGKILL)
        if borrowed is not None:
            if borrowed_was is not None:
                borrowed.write_bytes(borrowed_was)
            else:
                borrowed.unlink(missing_ok=True)
        # The app the host started outlives the browser by design.
        subprocess.run(["pkill", "-f", str(host)], check=False)
        server.shutdown()
        stderr = (engine_home / "host-stderr.txt")
        if stderr.exists() and stderr.read_text().strip():
            print(f"\nhost stderr:\n{stderr.read_text()[:2000]}")

    failed = [r for r in results if not r[0]]
    print(f"\n{len(results) - len(failed)} of {len(results)} checks passed")
    if failed:
        print(f"kept for reading: {work}")
        sys.exit(1)
    shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
