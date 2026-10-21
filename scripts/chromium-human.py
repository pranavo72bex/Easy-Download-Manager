#!/usr/bin/env python3
"""A person's session with Chromium and dm, with a real pointer and real keys.

    sh scripts/linux-chromium-human.sh           # runs this in a Linux container

Runs under X11 with a window manager, xdotool and the accessibility bus.
dm is opened as a person opens it, with its window (not --headless), and
registered with `dm-desktop --register`; Chromium is started with dist/chrome
loaded unpacked into a profile of its own. Then, each an X11 event:

  1. a link to a .zip is clicked: dm fetches it byte-exact, the browser's
     list stays empty, and dm's window shows the row Complete; then a PDF
     served at full speed, which Chromium finishes before it can be held
  2. another link is right-clicked and "Download link with dm" picked from
     the browser's own context menu: the same
  3. the pointer rests on a playing video until "Download video" shows over
     it, and that is clicked: the video lands in dm's folder
  4. a link Alt-clicked stays with the browser
  5. dm's window: a slow download's row clicked, Stop, Resume; then the
     finished row from step 1 selected and Delete clicked
  6. the toolbar popup opened from the Extensions button and "Catch
     downloads" switched off: a link clicked afterwards stays in the browser

Nothing here acts through DevTools or dm's control protocol. They are read:
where a link is on the page, what the browser's download list holds, which
rows dm shows and where, and screenshots. Controls the page does not own —
the context menu, the toolbar, dm's buttons — are found over AT-SPI, where a
screen reader finds them. Screenshots of the screen, the page and dm's
window go into DM_SHOTS at every step.

The server, the CDP client and the file bodies are chrome-check's.
"""
import base64, http.server, importlib.util, json, os, pathlib, shutil, socket, socketserver
import subprocess, sys, tempfile, threading, time, urllib.parse

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXT = ROOT / "dist" / "chrome"
BINARY = pathlib.Path(os.environ.get("DM_BINARY", "/target/debug/dm-desktop"))
CHROMIUM = os.environ.get("CHROMIUM", "/usr/bin/chromium")
SHOTS = pathlib.Path(os.environ.get("DM_SHOTS") or tempfile.mkdtemp(prefix="dm-human-shots-"))
# Microsoft Edge (DM_DISTRO=edge) is Chromium under another name: its
# profile is ~/.config/microsoft-edge (where `dm-desktop --register` puts
# Edge's host manifest), its windows' class is microsoft-edge, and it is
# "Microsoft Edge" on the accessibility bus.
EDGE = "microsoft-edge" in CHROMIUM
NAME = "Edge" if EDGE else "Chromium"
PROFILE_DIR = "microsoft-edge" if EDGE else "chromium"
APP = "edge" if EDGE else "chromium"
# DM_STORE=1: the extension as its store publishes it, not dist/chrome.
# The browser is given none on the command line: a managed policy, written
# before this runs, has it install the listing's build from the store
# (ExtensionInstallForcelist), which keeps the store's own id.
STORE = os.environ.get("DM_STORE") == "1"

spec = importlib.util.spec_from_file_location("chrome_check", ROOT / "scripts" / "chrome-check.py")
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
wait_for = common.wait_for

RESULTS = []
ORACLE = None
NOTED = []


def check(name, ok, detail=""):
    RESULTS.append((bool(ok), name))
    print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f" — {detail}" if detail and not ok else ""), flush=True)
    return bool(ok)


def x(*args, timeout=30):
    return subprocess.run([str(a) for a in args], capture_output=True, text=True, timeout=timeout)


# ------------------------------------------------------------ the server


SLOW = 16 * 1024 * 1024
VIDEO = {"bytes": b""}


def content(name):
    if name == "clip.webm":
        return VIDEO["bytes"]
    if name == "slow.zip":
        seed = common.body(name)
        return (seed * (SLOW // len(seed) + 1))[:SLOW]
    return common.body(name)


LINKS = ("grab.zip", "report.pdf", "menu.zip", "alt.zip", "slow.zip", "off.zip")
PAGE = ("<!doctype html><title>dm, by hand</title>"
        "<body style='font: 22px sans-serif; margin: 40px'><h1>Files</h1>"
        + "".join(f'<p><a id="{n}" href="/files/{n}">{n}</a></p>' for n in LINKS) + "</body>")
PLAYER = ("<!doctype html><title>a video</title><body style='margin: 40px'><h1>A video</h1>"
          '<video id="v" src="/files/clip.webm" width="640" height="360" autoplay muted loop playsinline></video>'
          "</body>")


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        self.do_GET(head=True)

    def do_GET(self, head=False):
        path = urllib.parse.urlparse(self.path).path
        if path in ("/page.html", "/video.html"):
            page = (PAGE if path == "/page.html" else PLAYER).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            if not head:
                self.wfile.write(page)
            return
        if not path.startswith("/files/"):
            self.send_error(404)
            return
        name = path.rsplit("/", 1)[1]
        data = content(name)
        start, end = 0, len(data) - 1
        ranged = self.headers.get("Range", "")
        if ranged.startswith("bytes="):
            first, _, last = ranged[6:].partition("-")
            start = int(first or 0)
            end = min(int(last), end) if last else end
        self.send_response(206 if ranged else 200)
        if name.endswith(".webm"):
            self.send_header("Content-Type", "video/webm")
        else:
            self.send_header("Content-Type", "application/pdf" if name.endswith(".pdf") else "application/zip")
            self.send_header("Content-Disposition", f'attachment; filename="{name}"')
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        if ranged:
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(data)}")
        self.end_headers()
        if head:
            return
        # slow.zip arrives slowly for everyone, dm too, so it is still
        # running when Stop is reached for; the rest only for the browser,
        # as chrome-check serves them. report.pdf arrives at full speed, which
        # Chromium finishes before the extension can hold it (gap 103).
        slow = name == "slow.zip" or not (self.headers.get("User-Agent", "").startswith("dm/")
                                          or name == "report.pdf")
        pause = 0.25 if name == "slow.zip" else 0.05
        try:
            for at in range(start, end + 1, 32 * 1024):
                self.wfile.write(data[at:min(at + 32 * 1024, end + 1)])
                if slow and not name.endswith(".webm"):
                    time.sleep(pause)
        except (BrokenPipeError, ConnectionResetError):
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        if not isinstance(sys.exc_info()[1], (ConnectionResetError, BrokenPipeError)):
            super().handle_error(request, client_address)


# ------------------------------------------- what a screen reader finds


def screen_reader_on():
    """Says on the accessibility bus that a screen reader is running, as Orca
    does: dm's window and Chromium's own controls are published over AT-SPI
    only then."""
    for prop in ("IsEnabled", "ScreenReaderEnabled"):
        x("gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus",
          "--method", "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", prop, "<true>")


# In a process of its own, as gui.py does, so nothing stays on the bus. Web
# pages are skipped: what is looked for here is the browser's own controls,
# and a page's tree is large.
WHERE = r"""
import json, sys, pyatspi
app_name, role, name, prefix, dump = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] == "1", sys.argv[5] == "1"
desktop = pyatspi.Registry.getDesktop(0)
found, apps, seen = [], [], []
for i in range(desktop.childCount):
    app = desktop[i]
    if not app:
        continue
    apps.append(app.name)
    if app_name.lower() not in (app.name or "").lower():
        continue
    def walk(node, depth=0):
        if node is None or depth > 60 or found:
            return
        try:
            r, n = node.getRoleName(), node.name or ""
        except Exception:
            return
        # A newer at-spi2-core (2.54 on Fedora 41, Arch's) names the role
        # "button"; Debian 12's (2.46) says "push button". The same control.
        if r == "button":
            r = "push button"
        if dump and n:
            seen.append(f"{r}: {n}")
        if r in ("document web", "document frame") and role not in ("document web", "*doc"):
            return
        if (role == "*" or r == role) and (n == name or (prefix and n.startswith(name))):
            try:
                e = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                if e.width > 0 and e.height > 0:
                    found.append([e.x + e.width // 2, e.y + e.height // 2])
                    return
            except Exception:
                pass
        for j in range(node.childCount):
            try:
                walk(node.getChildAtIndex(j), depth + 1)
            except Exception:
                pass
    walk(app)
print(json.dumps({"at": found[0] if found else None, "apps": apps, "seen": seen[:400]}))
"""


def accessible(app, role, name, prefix=False, dump=False):
    out = x(sys.executable, "-c", WHERE, app, role, name, "1" if prefix else "0", "1" if dump else "0",
            timeout=60 * common.SLOW).stdout.strip().splitlines()
    try:
        return json.loads(out[-1]) if out else {}
    except ValueError:
        return {}


def where(app, role, name, prefix=False):
    return accessible(app, role, name, prefix).get("at")


def press(at, button="1"):
    """The real pointer moved onto a point and clicked there."""
    x("xdotool", "mousemove", at[0], at[1])
    time.sleep(0.3)
    x("xdotool", "click", button)
    time.sleep(0.4)


def click_control(app, role, name, seconds=10, prefix=False):
    at = wait_for(lambda: where(app, role, name, prefix), seconds)
    if at:
        press(at)
    return at


# ---------------------------------------------------------- the windows


def windows(*query):
    return x("xdotool", "search", "--onlyvisible", *query).stdout.split()


def geometry(window):
    g = {}
    for line in x("xdotool", "getwindowgeometry", "--shell", window).stdout.split():
        k, _, v = line.partition("=")
        if k in ("X", "Y", "WIDTH", "HEIGHT"):
            g[k] = int(v)
    return g


def dm_window():
    found = windows("--name", "^dm$")
    return found[0] if found else None


def chromium_window():
    """The browser's window: the largest of Chromium's visible ones (its
    menus and bubbles are windows too)."""
    found = [(geometry(w), w) for w in windows("--class", PROFILE_DIR)]
    found = [(g.get("WIDTH", 0) * g.get("HEIGHT", 0), w) for g, w in found]
    return max(found)[1] if found else None


def front(window):
    if window:
        x("xdotool", "windowactivate", "--sync", window)
        time.sleep(0.4)


def oracle(home):
    """dm's control protocol, for reading what its window shows."""
    port_file = home / "control.port"
    if not wait_for(lambda: port_file.exists() and port_file.read_text().strip(), 30):
        return None
    lock = threading.Lock()
    link = {}

    def connect():
        sock = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=60)
        link["sock"], link["lines"] = sock, sock.makefile("r")
        ask("auth", token=(home / "control.token").read_text().strip())

    def ask(method, **params):
        link["sock"].sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        line = link["lines"].readline()
        if not line:
            raise BrokenPipeError("dm closed the connection")
        return json.loads(line)

    def call(method, **params):
        with lock:
            try:
                answer = ask(method, **params)
            except OSError:
                # dm ends a connection left idle five minutes, which a
                # store install's first start can take: connected afresh.
                connect()
                answer = ask(method, **params)
        if not answer.get("ok", True):
            raise RuntimeError(f"{method}: {answer.get('error')}")
        return answer.get("result")

    with lock:
        connect()
    return call


def dm_rows():
    try:
        return (ORACLE("ui.inspect") or {}).get("rows") or []
    except (OSError, RuntimeError, ValueError) as e:
        print(f"        (dm did not answer: {e})")
        return []


def dm_row(name):
    return next((r for r in dm_rows() if r.get("name") == name), None)


def row_point(name):
    """A row's centre on screen: dm's own rectangle for it plus the window's
    origin, as gui.py finds rows (egui reports table cells to AT-SPI at the
    wrong place)."""
    row, window = dm_row(name), dm_window()
    if not row or not row.get("rect") or not window:
        return None
    g, r = geometry(window), row["rect"]
    # On its name, where a person clicks: the row's middle is off the window
    # when the columns run past it.
    return [g["X"] + int(r[0] + min(80, (r[2] - r[0]) / 2)), g["Y"] + int((r[1] + r[3]) / 2)]


# ------------------------------------------------------------ the page


# Where the page's viewport is on screen. The window's own numbers give a
# first guess; the pointer, moved there for real, says where it landed in
# the page, which settles it. This listener only watches.
WATCH = """(() => { if (!window.__dmPointer) { window.__dmPointer = null;
  addEventListener('mousemove', e => { window.__dmPointer = [e.clientX, e.clientY]; }, {capture: true, passive: true}); }
  return [screenX, screenY, outerWidth - innerWidth, outerHeight - innerHeight]; })()"""


def rect_of(selector, shadow=None):
    inner = f".shadowRoot.querySelector({json.dumps(shadow)})" if shadow else ""
    return (f"(() => {{ const e = document.querySelector({json.dumps(selector)}){inner}; if (!e) return null;"
            " const r = e.getBoundingClientRect(); return {x: r.left, y: r.top, w: r.width, h: r.height}; })()")


def pointer_to(tab, rect_js, dx=0.5, dy=0.5):
    """Moves the real pointer onto an element of the page. Returns the screen
    point, or None when the element is not there."""
    sx0, sy0, chrome_x, chrome_y = tab.evaluate(WATCH)
    r = tab.evaluate(rect_js)
    if not r:
        return None
    cx, cy = r["x"] + r["w"] * dx, r["y"] + r["h"] * dy
    sx, sy = int(sx0 + chrome_x / 2 + cx), int(sy0 + chrome_y + cy)
    for _ in range(4):
        # A step off and back, so the pointer moves even if it is already there.
        x("xdotool", "mousemove", sx + 3, sy + 3)
        time.sleep(0.1)
        x("xdotool", "mousemove", sx, sy)
        seen = wait_for(lambda: (tab.evaluate("window.__dmPointer") or [None])[0] is not None
                        and tab.evaluate("window.__dmPointer"), 2)
        tab.evaluate("window.__dmPointer = null, 1")
        if not seen:
            break
        ex, ey = cx - seen[0], cy - seen[1]
        if abs(ex) < 1.5 and abs(ey) < 1.5:
            break
        sx, sy = int(sx + ex), int(sy + ey)
    return [sx, sy]


def address_bar(url):
    """A person's way to a page: Ctrl+L, the address typed, Enter."""
    front(chromium_window())
    x("xdotool", "key", "--clearmodifiers", "ctrl+l")
    time.sleep(0.3)
    x("xdotool", "type", "--delay", "10", url)
    x("xdotool", "key", "Return")


# ------------------------------------------------------------ pictures


SHOT_N = [0]


def shoot(label, tab=None):
    """The whole screen, the page (DevTools) and dm's window (its control
    protocol), each into DM_SHOTS."""
    SHOT_N[0] += 1
    stem = f"{SHOT_N[0]:02d}-{label}"
    x("ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size", "2400x1200",
      "-i", os.environ.get("DISPLAY", ":0"), "-frames:v", "1", SHOTS / f"{stem}-screen.png")
    if tab is not None:
        try:
            data = tab.call("Page.captureScreenshot", format="png")["data"]
            (SHOTS / f"{stem}-page.png").write_bytes(base64.b64decode(data))
        except (RuntimeError, OSError, EOFError):
            pass
    if ORACLE:
        try:
            made = pathlib.Path(DM_HOME) / "screenshots" / f"{stem}.png"
            ORACLE("ui.screenshot", name=stem)
            if wait_for(lambda: made.exists(), 5):
                shutil.copy(made, SHOTS / f"{stem}-dm.png")
        except (OSError, RuntimeError, ValueError):
            pass


# ------------------------------------------------------------- the check


DM_HOME = ""


def whole(folder, name):
    path = folder / name
    return path.exists() and path.read_bytes() == content(name)


def make_video(work):
    """A real, playable video for the player: ten seconds of test card."""
    out = work / "clip.webm"
    x("ffmpeg", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=640x360:rate=25", "-t", "10",
      "-c:v", "libvpx", "-b:v", "300k", out, timeout=120)
    VIDEO["bytes"] = out.read_bytes() if out.exists() else common.body("clip.webm")


def main():
    global ORACLE, DM_HOME
    if not STORE and not (EXT / "pkg" / "dm_catcher_bg.wasm").exists():
        sys.exit("no built extension at dist/chrome — run: sh scripts/package.sh")
    SHOTS.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-human-"))
    home, dm_home = work / "home", work / "dm-home"
    profile = home / ".config" / PROFILE_DIR
    dm_dl, browser_dl = work / "dm-downloads", work / "browser-downloads"
    for d in (profile / "Default", dm_home, dm_dl, browser_dl):
        d.mkdir(parents=True)
    DM_HOME = str(dm_home)
    make_video(work)
    (profile / "Default" / "Preferences").write_text(json.dumps({
        "download": {"default_directory": str(browser_dl), "prompt_for_download": False,
                     "directory_upgrade": True},
        "savefile": {"default_directory": str(browser_dl)},
    }))
    (dm_home / "settings.json").write_text(json.dumps({
        "download_dir": str(dm_dl), "categorize": False, "watch_clipboard": False,
        # The progress and completion windows would stand over the window
        # being checked; gui.py drives them.
        "show_progress": False, "show_complete": False,
    }))
    env = dict(os.environ, HOME=str(home), DM_HOME=str(dm_home), ACCESSIBILITY_ENABLED="1")

    server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_address[1]}"
    screen_reader_on()

    print("dm, opened with its window, and registered with the browser")
    registered = subprocess.run([str(BINARY), "--register"], env=env, capture_output=True, text=True, timeout=30)
    manifest = profile / "NativeMessagingHosts" / "com.dm.engine.json"
    check(f"`dm-desktop --register` writes {NAME}'s host manifest", manifest.exists(),
          (registered.stdout + registered.stderr).strip()[:300])
    app = subprocess.Popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL,
                           stderr=open(work / "dm-stderr.txt", "wb"))
    browser = None
    try:
        check("dm opens its window", wait_for(dm_window, 30))
        ORACLE = oracle(dm_home)
        x("xdotool", "windowmove", dm_window(), 1230, 40)
        x("xdotool", "windowsize", dm_window(), 1150, 760)

        loaded = [] if STORE else [f"--load-extension={EXT}", f"--disable-extensions-except={EXT}"]
        args = [CHROMIUM, f"--user-data-dir={profile}", *loaded, "--remote-debugging-port=0",
                "--no-first-run", "--no-default-browser-check", "--disable-gpu", "--lang=en-US",
                "--disable-features=DisableLoadExtensionCommandLineSwitch,HttpsUpgrades",
                "--enable-unsafe-extension-debugging", "--force-renderer-accessibility",
                "--window-position=0,0", "--window-size=1200,1100", "--password-store=basic",
                "about:blank"]
        if os.geteuid() == 0:
            args[1:1] = ["--no-sandbox", "--disable-dev-shm-usage"]
        browser = subprocess.Popen(args, env=env, stdout=subprocess.DEVNULL,
                                   stderr=open(work / "chromium-stderr.txt", "wb"), preexec_fn=os.setpgrp)
        try:
            if STORE:
                # A first start installs the store's copy before it is up.
                wait_for(lambda: (profile / "DevToolsActivePort").exists(), 120)
            base = common.devtools(profile)
        except SystemExit:
            print(f"{NAME} is {'still running' if browser.poll() is None else f'gone (exit {browser.poll()})'}; "
                  f"profile holds {sorted(p.name for p in profile.iterdir())}", flush=True)
            # What the browser said as it failed to start, which is all
            # there is to go on from a CI run.
            said = (work / "chromium-stderr.txt").read_text(errors="replace")
            print(f"{NAME} said on stderr:\n" + "\n".join(said.splitlines()[-40:]), flush=True)
            raise
        worker_t = wait_for(lambda: next((t for t in common.targets(base) if t["type"] == "service_worker"
                                          and t["url"].endswith("/background.js")), None),
                            # A store install is fetched first.
                            180 if STORE else 30)
        if not check(f"{NAME} starts with the extension's worker running", worker_t):
            return
        worker = common.Target(worker_t["webSocketDebuggerUrl"])
        def ready():
            try:
                return worker.evaluate("typeof dmReady") == "object"
            except OSError:
                return False
        # The store's build may not carry the test hook `dmReady`: its
        # absence is not a failure there, the checks below are the proof.
        if not wait_for(ready, 15) and not STORE:
            sys.exit("the worker never said it was ready")
        page_t = wait_for(lambda: next((t for t in common.targets(base) if t["type"] == "page"), None), 15)
        tab = common.Target(page_t["webSocketDebuggerUrl"])
        wait_for(chromium_window, 15)
        time.sleep(1)
        shoot("start", tab)

        def ask_worker(expression):
            """The worker's answer, asking a fresh one where the worker went
            idle and its first connection died with it (a store build's
            service worker sleeps between events)."""
            nonlocal worker
            for attempt in range(4):
                try:
                    return worker.evaluate(expression)
                except OSError:
                    time.sleep(1)
                    again = next((t for t in common.targets(base) if t["type"] == "service_worker"
                                  and t["url"].endswith("/background.js")), None)
                    if again:
                        worker = common.Target(again["webSocketDebuggerUrl"])
            if STORE:
                # A policy-installed copy does not answer the debugger:
                # what the browser's list holds cannot be read, and the
                # checks that lean on it look at its download folder only.
                if not NOTED:
                    NOTED.append(1)
                    print("  note: the store copy's worker does not answer the debugger; "
                          "the browser's downloads list is not read, only its folder", flush=True)
                return None
            raise OSError("the extension's worker did not answer")

        def browser_list(name):
            if STORE and NOTED:
                return []
            got = ask_worker(f"chrome.downloads.search({{}}).then(l => l.filter(d => d.url.endsWith('/{name}'))"
                             ".map(d => d.state))")
            return [] if got is None else got

        def open_page(path, selector):
            address_bar(origin + path)
            common.loaded(tab, path, selector)
            time.sleep(0.5)

        # 1. A link, clicked.
        print("\n1. a link clicked")
        open_page("/page.html", "[id='grab.zip']")
        at = pointer_to(tab, rect_of("[id='grab.zip']"))
        if at:
            x("xdotool", "click", "1")
        landed = wait_for(lambda: whole(dm_dl, "grab.zip"), 60)
        check("1. the clicked .zip lands byte-exact in dm's folder", landed, str(sorted(p.name for p in dm_dl.iterdir())))
        time.sleep(2)
        left = browser_list("grab.zip")
        check("1. the browser's own downloads list is empty of it, and its folder too",
              left == [] and not (browser_dl / "grab.zip").exists(),
              f"list {left}, folder {sorted(p.name for p in browser_dl.iterdir())}")
        row = wait_for(lambda: (dm_row("grab.zip") or {}).get("status") == "Complete" and dm_row("grab.zip"), 15)
        check("1. dm's window shows the row Complete", row, str(dm_row("grab.zip")))
        shoot("1-clicked-link", tab)

        # 1b. A PDF served fast: Chromium has it whole before it can be held.
        print("\n1b. a PDF clicked, served at full speed")
        front(chromium_window())
        at = pointer_to(tab, rect_of("[id='report.pdf']"))
        if at:
            x("xdotool", "click", "1")
        landed = wait_for(lambda: whole(dm_dl, "report.pdf"), 60)
        check("1b. the clicked PDF lands byte-exact in dm's folder", landed, str(sorted(p.name for p in dm_dl.iterdir())))
        time.sleep(2)
        left = browser_list("report.pdf")
        check("1b. and the browser's copy, finished before it could be held, is removed",
              left == [] and not (browser_dl / "report.pdf").exists(),
              f"list {left}, folder {sorted(p.name for p in browser_dl.iterdir())}")
        shoot("1b-clicked-pdf", tab)

        # 2. The context menu.
        print("\n2. a link right-clicked, 'Download link with dm' picked")
        front(chromium_window())
        at = pointer_to(tab, rect_of("[id='menu.zip']"))
        if at:
            x("xdotool", "click", "3")
        item = wait_for(lambda: where(APP, "menu item", "Download link with dm"), 10)
        shoot("2-context-menu", tab)
        if not item:
            seen = accessible(APP, "menu item", "-", dump=True)
            print(f"        AT-SPI apps {seen.get('apps')}; menu items seen: "
                  f"{[s for s in seen.get('seen', []) if s.startswith('menu')][:30]}")
        check("2. the browser's context menu offers 'Download link with dm'", item)
        if item:
            press(item)
        else:
            x("xdotool", "key", "Escape")
        landed = wait_for(lambda: whole(dm_dl, "menu.zip"), 60)
        check("2. picked, the link lands byte-exact in dm's folder", landed)
        time.sleep(1)
        check("2. and nothing of it is in the browser's list or folder",
              browser_list("menu.zip") == [] and not (browser_dl / "menu.zip").exists(), str(browser_list("menu.zip")))
        row = wait_for(lambda: (dm_row("menu.zip") or {}).get("status") == "Complete" and dm_row("menu.zip"), 15)
        check("2. dm's window shows its row Complete", row, str(dm_row("menu.zip")))
        shoot("2-menu-done", tab)

        # 3. The video's panel.
        print("\n3. the pointer on a playing video, 'Download video' clicked")
        open_page("/video.html", "#v")
        wait_for(lambda: tab.evaluate("!!document.querySelector('dm-panel')"), 15)
        playing = wait_for(lambda: tab.evaluate("(() => { const v = document.getElementById('v');"
                                                " return !v.paused && v.currentTime > 0.3; })()"), 10)
        check("3. the video is playing", playing, tab.evaluate("document.getElementById('v').error && "
                                                              "document.getElementById('v').error.message"))
        shown = None
        for step in range(12):
            pointer_to(tab, rect_of("#v"), 0.5 + step % 4 * 0.02, 0.5)
            time.sleep(0.4)
            state = common.panel(tab) or {}
            if state.get("shown"):
                shown = state
                break
        shoot("3-panel", tab)
        check("3. resting on it raises 'Download video'", shown and shown.get("title") == "Download video",
              str(common.panel(tab)))
        if shown:
            pointer_to(tab, rect_of("dm-panel", ".go"))
            time.sleep(0.3)
            x("xdotool", "click", "1")
        # dm asks where a panel video goes (Download details) unless told not
        # to; a person answers Start now.
        start = wait_for(lambda: where("dm-desktop", "push button", "Start now"), 15)
        if start:
            front(dm_window())
            shoot("3-download-details")
            start = click_control("dm-desktop", "push button", "Start now")
        check("3. dm asks, and its Start now is clicked", start)
        landed = wait_for(lambda: whole(dm_dl, "clip.webm"), 60)
        check("3. the video lands byte-exact in dm's folder", landed, str(sorted(p.name for p in dm_dl.iterdir())))
        shoot("3-video-done", tab)

        # 4. Alt-click.
        print("\n4. a link Alt-clicked")
        open_page("/page.html", "[id='alt.zip']")
        at = pointer_to(tab, rect_of("[id='alt.zip']"))
        if at:
            x("xdotool", "keydown", "alt")
            time.sleep(0.2)
            x("xdotool", "click", "1")
            time.sleep(0.2)
            x("xdotool", "keyup", "alt")
        kept = wait_for(lambda: whole(browser_dl, "alt.zip"), 60)
        check("4. an Alt-clicked .zip finishes in the browser, not dm",
              kept and not (dm_dl / "alt.zip").exists() and not dm_row("alt.zip"),
              f"browser folder {sorted(p.name for p in browser_dl.iterdir())}; dm row {dm_row('alt.zip')}")
        shoot("4-alt-click", tab)
        x("xdotool", "key", "Escape")

        # 5. dm's own buttons.
        print("\n5. dm's window: Stop, Resume and Delete clicked")
        front(chromium_window())
        at = pointer_to(tab, rect_of("[id='slow.zip']"))
        if at:
            x("xdotool", "click", "1")
        running = wait_for(lambda: (dm_row("slow.zip") or {}).get("status", "").endswith("%") and dm_row("slow.zip"), 30)
        check("5. a slow .zip clicked in the browser is running in dm", running, str(dm_row("slow.zip")))
        front(dm_window())
        at = wait_for(lambda: row_point("slow.zip"), 10)
        if at:
            press(at)
        selected = wait_for(lambda: (ORACLE("ui.inspect") or {}).get("selected"), 5)
        check("5. its row, clicked, is selected", selected and (dm_row("slow.zip") or {}).get("id") in selected,
              str(selected))
        stopped = click_control("dm-desktop", "push button", "Stop") and \
            wait_for(lambda: (dm_row("slow.zip") or {}).get("status", "").startswith("Stopped"), 15)
        shoot("5-stopped")
        check("5. Stop, clicked, stops it", stopped, str(dm_row("slow.zip")))
        held = (dm_row("slow.zip") or {}).get("status") or ""
        time.sleep(2)
        check("5. and it stays stopped", held.startswith("Stopped") and (dm_row("slow.zip") or {}).get("status", "").startswith("Stopped"))
        resumed = click_control("dm-desktop", "push button", "Resume") and \
            wait_for(lambda: (dm_row("slow.zip") or {}).get("status", "") != "" and not (dm_row("slow.zip") or {}).get("status", "").startswith("Stopped"), 15)
        check("5. Resume, clicked, carries on", resumed, str(dm_row("slow.zip")))
        shoot("5-resumed")
        done = wait_for(lambda: whole(dm_dl, "slow.zip"), 120)
        check("5. and the slow .zip lands whole and byte-exact", done,
              f"{dm_row('slow.zip')}; on disk {(dm_dl / 'slow.zip').stat().st_size if (dm_dl / 'slow.zip').exists() else 0}")
        at = wait_for(lambda: row_point("grab.zip"), 10)
        if at:
            press(at)
        picked = (ORACLE("ui.inspect") or {}).get("selected") or []
        check("5. the finished row from step 1, clicked, is selected",
              (dm_row("grab.zip") or {}).get("id") in picked, str(picked))
        click_control("dm-desktop", "push button", "Remove")
        # A finished row is taken off the list ("Remove it"); one still to
        # finish has its parts deleted too ("Yes, remove"). Either asks.
        confirm = wait_for(lambda: where("dm-desktop", "push button", "Remove it")
                           or where("dm-desktop", "push button", "Yes, remove"), 5)
        if confirm:
            press(confirm)
        gone = wait_for(lambda: dm_row("grab.zip") is None, 10)
        shoot("5-deleted")
        check("5. Delete, clicked, removes that row and only that one",
              gone and dm_row("menu.zip") and dm_row("slow.zip"), str([r.get("name") for r in dm_rows()]))
        print(f"        the finished file after Delete: {'kept' if (dm_dl / 'grab.zip').exists() else 'removed'}")

        # 6. The toolbar popup.
        print("\n6. the toolbar popup: 'Catch downloads' switched off")
        front(chromium_window())
        x("xdotool", "key", "Escape")
        button = click_control(APP, "push button", "Extensions", seconds=10)
        if not button:
            seen = accessible(APP, "push button", "-", dump=True)
            print(f"        AT-SPI apps {seen.get('apps')}; buttons seen: "
                  f"{[s for s in seen.get('seen', []) if 'button' in s][:40]}")
        entry = button and wait_for(lambda: where(APP, "*", "dm — Download Manager"), 10)
        shoot("6-extensions-menu", tab)
        if entry:
            press(entry)
        popup_t = entry and wait_for(lambda: next((t for t in common.targets(base)
                                                  if t["url"].endswith("/popup.html")), None), 10)
        check("6. the Extensions button, then dm, opens the popup", popup_t,
              "no Extensions button found" if not button else "no dm entry" if not entry else "no popup")
        if popup_t:
            popup = common.Target(popup_t["webSocketDebuggerUrl"])
            wait_for(lambda: popup.evaluate("!!document.getElementById('catching')"), 10)
            time.sleep(0.5)
            shoot("6-popup", tab)
            at = pointer_to(popup, rect_of("#catching"))
            if at:
                x("xdotool", "click", "1")
            off = wait_for(lambda: ask_worker(
                "chrome.storage.local.get('dm.catching').then(r => r['dm.catching'] === false)"), 5)
            check("6. its 'Catch downloads', clicked, is switched off", off,
                  str(popup.evaluate("document.getElementById('catching').checked")))
            shoot("6-catching-off", tab)
            x("xdotool", "key", "Escape")
            time.sleep(0.5)
            front(chromium_window())
            at = pointer_to(tab, rect_of("[id='off.zip']"))
            if at:
                x("xdotool", "click", "1")
            kept = wait_for(lambda: whole(browser_dl, "off.zip"), 60)
            check("6. then a clicked .zip stays with the browser",
                  kept and not dm_row("off.zip") and not (dm_dl / "off.zip").exists(),
                  f"browser folder {sorted(p.name for p in browser_dl.iterdir())}; dm row {dm_row('off.zip')}")
            shoot("6-off-click", tab)
    finally:
        if browser:
            try:
                os.killpg(os.getpgid(browser.pid), 15)
                browser.wait(10)
            except (OSError, subprocess.TimeoutExpired):
                pass
        if app.poll() is None:
            app.terminate()
            try:
                app.wait(10)
            except subprocess.TimeoutExpired:
                app.kill()
        server.shutdown()
        for log in ("dm-stderr.txt", "chromium-stderr.txt"):
            if (work / log).exists():
                shutil.copy(work / log, SHOTS / log)

    failed = [name for ok, name in RESULTS if not ok]
    print(f"\n{len(RESULTS) - len(failed)} of {len(RESULTS)} checks passed, with real input; screenshots in {SHOTS}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
