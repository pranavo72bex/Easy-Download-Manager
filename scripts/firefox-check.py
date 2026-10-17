"""Loads the built extension into a real Firefox and reports what Firefox says.

    sh scripts/package.sh                         # builds dist/firefox
    python3 scripts/firefox-check.py              # installs it, reads its console
    python3 scripts/firefox-check.py native       # and watches a hand-off to dm

With no Firefox on the machine, the latest release is fetched into
~/.cache/dm-firefox (DM_FIREFOX_CACHE) and run from there; nothing is installed.

Firefox validates a manifest at install time and will not say what it
disliked from a zip on disk, so this drives the remote debugging protocol the
way `about:debugging` does: install the unpacked directory as a temporary
add-on, then read the background page's console and evaluate in it. No
web-ext, and no JavaScript of this project's: the extension is Rust compiled
to WebAssembly, and this asks the loader's `dmReady` what loaded.

`DM_REAL_INPUT=1` (with `native`, under X11 with xdotool) also right-clicks a
real link with a real mouse and picks "Download link with dm" with real
keys: Firefox runs with a window, and the parent process's console is only
read, to see which item the keys have reached. `scripts/linux-firefox-check.sh`
runs it in a Linux container.

`native` needs a built dm (desktop/target/debug/dm-desktop, or DM_BINARY).
Firefox reads host manifests only from the real home directory, so for the
length of the check dm's manifest there is replaced by one naming a copy of
the binary in a temporary DM_HOME, and the original — if there was one — is
put back afterwards. The download is served by the same local server
`chrome-check.py` uses; nothing else on the network is touched.
"""
import importlib.util, json, os, pathlib, shutil, socket, subprocess, sys, tempfile, threading, time

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXT = ROOT / "dist" / "firefox"
CACHE = pathlib.Path(os.environ.get("DM_FIREFOX_CACHE") or pathlib.Path.home() / ".cache/dm-firefox")


def fetch_firefox():
    """Firefox fetched into a cache, the way chrome-check fetches Chrome for
    Testing: nothing is installed, and deleting the folder undoes it."""
    if sys.platform == "darwin":
        app = CACHE / "Firefox.app/Contents/MacOS/firefox"
        if not app.exists():
            CACHE.mkdir(parents=True, exist_ok=True)
            dmg = CACHE / "firefox.dmg"
            print("fetching Firefox into", CACHE, flush=True)
            subprocess.run(["curl", "-sSLfo", str(dmg),
                            "https://download.mozilla.org/?product=firefox-latest-ssl&os=osx&lang=en-US"], check=True)
            mount = tempfile.mkdtemp(prefix="dm-firefox-dmg-")
            subprocess.run(["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", mount, str(dmg)],
                           check=True, stdout=subprocess.DEVNULL)
            try:
                shutil.copytree(pathlib.Path(mount) / "Firefox.app", CACHE / "Firefox.app", symlinks=True)
            finally:
                subprocess.run(["hdiutil", "detach", mount, "-quiet"], check=False)
                shutil.rmtree(mount, ignore_errors=True)
                dmg.unlink(missing_ok=True)
        return str(app)
    if sys.platform.startswith("linux"):
        binary = CACHE / "firefox/firefox"
        if not binary.exists():
            CACHE.mkdir(parents=True, exist_ok=True)
            arch = "linux64-aarch64" if os.uname().machine in ("aarch64", "arm64") else "linux64"
            print("fetching Firefox into", CACHE, flush=True)
            subprocess.run(f"curl -sSLf 'https://download.mozilla.org/?product=firefox-latest-ssl&os={arch}&lang=en-US'"
                           f" | tar -xJ -C '{CACHE}'", shell=True, check=True)
        return str(binary)
    return ""


FIREFOX = next(
    (p for p in (
        os.environ.get("FIREFOX", ""),
        "/Applications/Firefox.app/Contents/MacOS/firefox",
        "/usr/bin/firefox",
        "/snap/bin/firefox",
        str(CACHE / "Firefox.app/Contents/MacOS/firefox"),
        str(CACHE / "firefox/firefox"),
    ) if p and pathlib.Path(p).exists()),
    "",
)
PORT = 6100 + os.getpid() % 400
NATIVE = "native" in sys.argv
REAL = NATIVE and bool(os.environ.get("DM_REAL_INPUT"))

# The server, the files and the binary are chrome-check's, so the two
# browsers are checked against the same thing.
spec = importlib.util.spec_from_file_location("chrome_check", ROOT / "scripts" / "chrome-check.py")
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)


class Rdp:
    """Firefox's protocol is `<byte length>:<json>` over a plain socket."""

    def __init__(self, port):
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=30)
        self.buf = b""

    def recv(self):
        while True:
            if b":" in self.buf:
                head, rest = self.buf.split(b":", 1)
                if head.isdigit() and len(rest) >= int(head):
                    n = int(head)
                    self.buf = rest[n:]
                    return json.loads(rest[:n])
            chunk = self.sock.recv(65536)
            if not chunk:
                raise EOFError("Firefox closed the debugging connection")
            self.buf += chunk

    def send(self, **msg):
        body = json.dumps(msg).encode()
        self.sock.sendall(str(len(body)).encode() + b":" + body)

    def ask(self, to, type, **rest):
        self.send(to=to, type=type, **rest)
        while True:
            m = self.recv()
            if m.get("from") == to:
                return m


def collect(packet, into):
    """Pulls console and error messages out of whatever else is arriving."""
    for resource in packet.get("array", []) or []:
        for item in (resource[1] if isinstance(resource, list) and len(resource) > 1 else []):
            text = item.get("message") or item.get("errorMessage") or ""
            if isinstance(text, dict):
                text = text.get("preview", {}).get("message") or str(text)
            args = item.get("arguments") or []
            if args:
                text = text or " ".join(str(a) for a in args)
            if text:
                into.append(f"[{item.get('level', 'error')}] {text}")
    for item in packet.get("resources", []) or []:
        text = item.get("message") or item.get("errorMessage") or ""
        if text:
            into.append(f"[{item.get('level', 'error')}] {text}")


def profile(downloads):
    d = pathlib.Path(tempfile.mkdtemp(prefix="dm-ffprofile-"))
    (d / "user.js").write_text(
        'user_pref("devtools.debugger.remote-enabled", true);\n'
        'user_pref("devtools.debugger.prompt-connection", false);\n'
        'user_pref("devtools.chrome.enabled", true);\n'
        'user_pref("xpinstall.signatures.required", false);\n'
        'user_pref("browser.shell.checkDefaultBrowser", false);\n'
        'user_pref("datareporting.policy.dataSubmissionEnabled", false);\n'
        # Firefox's first-run welcome and terms dialog sits over every page
        # of a new profile and takes its clicks; a check is not a first run.
        'user_pref("termsofuse.bypassNotification", true);\n'
        'user_pref("termsofuse.acceptedVersion", 999);\n'
        'user_pref("termsofuse.acceptedDate", "1700000000000");\n'
        'user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);\n'
        'user_pref("browser.aboutwelcome.enabled", false);\n'
        'user_pref("trailhead.firstrun.didSeeAboutWelcome", true);\n'
        'user_pref("browser.startup.homepage_override.mstone", "ignore");\n'
        'user_pref("browser.download.useDownloadDir", true);\n'
        'user_pref("browser.download.folderList", 2);\n'
        f'user_pref("browser.download.dir", {json.dumps(str(downloads))});\n'
        'user_pref("browser.download.always_ask_before_handling_new_types", false);\n'
    )
    return d


def tab_console(rdp, ending):
    """The console of the open tab whose address ends so."""
    for _ in range(30):
        tabs = rdp.ask("root", "listTabs").get("tabs", [])
        found = [t for t in tabs if str(t.get("url", "")).endswith(ending)]
        if found:
            return rdp.ask(found[0]["actor"], "getTarget")["frame"]["consoleActor"]
        time.sleep(0.5)
    return None


def xdo(*args):
    return subprocess.run(["xdotool", *args], capture_output=True, text=True, timeout=20).stdout


# Which item of the page's context menu the keyboard has reached, read in
# Firefox's parent process. The keys and the click are real; this only looks.
ACTIVE = """(() => {
  const w = Services.wm.getMostRecentWindow('navigator:browser');
  const top = w.document.getElementById('contentAreaContextMenu');
  if (top.state !== 'open') return 'closed';
  const active = popup => popup && popup.querySelector(':scope > [_moz-menuactive]');
  const a = active(top);
  if (!a) return 'none';
  const inner = a.localName === 'menu' && a.menupopup.state === 'open' ? active(a.menupopup) : null;
  const holds = a.localName === 'menu'
    && [...a.querySelectorAll('menuitem')].some(i => i.label === 'Download link with dm');
  return JSON.stringify({label: a.label || '', holds, inner: inner ? inner.label : null});
})()"""


def real_menu_click(rdp, evaluate, resolve, origin, engine_dl):
    """A real right-click on a link, and "Download link with dm" picked with
    real keys, as a person picks it."""
    print("\n=== a real right-click on a link ===")
    proc = rdp.ask("root", "getProcess", id=0)
    parent = rdp.ask(proc["processDescriptor"]["actor"], "getTarget")["process"]["consoleActor"]

    def look():
        return (evaluate(ACTIVE, parent) or {}).get("result")

    def active():
        seen = look() or ""
        return json.loads(seen) if seen.startswith("{") else None

    # Loaded into the tab in front, which is the one a person right-clicks in.
    resolve("the page with links, in front",
            f"browser.tabs.update({{url: '{origin}/page.html'}}).then(() => 'yes')")
    page = tab_console(rdp, "/page.html")
    time.sleep(1)
    where = page and common.wait_for(lambda: (evaluate(
        "(() => { const a = document.getElementById('kept.zip'); if (!a) return 0;"
        " const r = a.getBoundingClientRect();"
        " return Math.round(mozInnerScreenX + r.left + r.width / 2) + ','"
        " + Math.round(mozInnerScreenY + r.top + r.height / 2); })()", page) or {}).get("result") or None, 20)
    if not where:
        print(f"  {'the link is on screen':<40} NO")
        return
    window = (xdo("search", "--onlyvisible", "--class", "firefox").split() or [""])[-1]
    if window:
        xdo("windowactivate", "--sync", window)
    x, y = where.split(",")
    xdo("mousemove", "--sync", x, y)
    xdo("click", "3")
    opened = common.wait_for(lambda: look() not in (None, "closed"), 10)
    print(f"  {'right-clicking it opens the menu':<40} {'yes' if opened else 'NO'}"
          + ("" if opened else f" (clicked at {where}; the menu reads {look()!r})"))
    if not opened:
        return
    reached = None
    for _ in range(40):
        xdo("key", "Down")
        time.sleep(0.15)
        now = active()
        if now and (now["label"] == "Download link with dm" or now["holds"]):
            reached = now
            break
    if reached and reached["holds"]:
        # Several items from one extension are put in a submenu of its own.
        xdo("key", "Right")
        time.sleep(0.4)
        for _ in range(10):
            now = active()
            if now and now["inner"] == "Download link with dm":
                break
            xdo("key", "Down")
            time.sleep(0.15)
        else:
            reached = None
    print(f"  {'the keys reach Download link with dm':<40} {'yes' if reached else 'NO'}")
    if not reached:
        xdo("key", "Escape")
        return
    xdo("key", "Return")
    landed = common.wait_for(lambda: common.whole(engine_dl, "kept.zip"), 60)
    print(f"  {'and dm fetched the link':<40} {'yes' if landed else 'NO'}")


def live_panel(rdp, evaluate, resolve, url, engine_dl):
    """The panel on a real site's player (DM_LIVE_PAGE), as a person uses it:
    the real pointer (xdotool) moves over the playing video, clicks the
    panel and, where a list of qualities opens, clicks the smallest. The
    panel takes only a person's input, so nothing here can be a click made
    by script. Says why when the site gives no video to hover."""
    print(f"\n=== the panel on {url} ===")
    if not REAL:
        print(f"  {'the live page':<40} not checked: needs real input (DM_REAL_INPUT=1)")
        return
    resolve("the live page", f"browser.tabs.update({{url: {json.dumps(url)}}}).then(() => 'opened')")
    # The tab in front, wherever the site sent it: a consent page or a
    # check that the visitor is human is said as such, not missed.
    def active():
        tabs = rdp.ask("root", "listTabs").get("tabs", [])
        front = [t for t in tabs if t.get("selected")] or tabs[-1:]
        return rdp.ask(front[0]["actor"], "getTarget")["frame"]["consoleActor"] if front else None
    page = None
    for _ in range(60):
        page = active()
        if page and (evaluate("!!document.querySelector('dm-panel') && document.readyState === 'complete'", page) or {}).get("result") is True:
            break
        time.sleep(0.5)
    def in_page(source):
        return (evaluate(source, page) or {}).get("result") if page else None
    where_now = lambda: in_page("location.href + ' — ' + document.title")
    if not in_page("!!document.querySelector('dm-panel')"):
        print(f"  {'the page script loads':<40} NO, at {where_now()}")
        return
    # Every video, including those inside web components' open shadow roots
    # (the Internet Archive's player keeps its video in one).
    video = ("(() => { const out = []; const walk = root => { for (const e of root.querySelectorAll('*')) {"
             " if (e.tagName === 'VIDEO') out.push(e); if (e.shadowRoot) walk(e.shadowRoot); } };"
             " walk(document); return out; })().sort((a, b) => b.clientWidth - a.clientWidth)[0]")
    # A player built by a component makes its video after the page loads.
    if not common.wait_for(lambda: in_page(f"!!{video}") is True, 20):
        print(f"  {'a video on the page':<40} NO, at {where_now()}")
        return
    in_page(f"(() => {{ const v = {video}; v.scrollIntoView({{block: 'center'}}); v.muted = true; v.play().catch(() => 0); return 1; }})()")
    time.sleep(2)
    panel = "document.querySelector('dm-panel').shadowRoot"
    state = (f"(() => {{ const h = document.querySelector('dm-panel'); const r = {panel};"
             " return getComputedStyle(h).display + '|' + r.querySelector('.title').textContent + '|'"
             " + [...r.querySelectorAll('.item')].map(i => i.textContent).join(';'); })()")
    on_screen = ("(() => {{ const e = {0}; if (!e) return null; const r = e.getBoundingClientRect();"
                 " return Math.round(mozInnerScreenX + r.left + r.width / 2) + ','"
                 " + Math.round(mozInnerScreenY + r.top + r.height / 2); }})()")

    def real_click(element):
        where = in_page(on_screen.format(element))
        if not where:
            return False
        x, y = where.split(",")
        xdo("mousemove", "--sync", x, y)
        time.sleep(0.25)
        xdo("click", "1")
        return True

    window = (xdo("search", "--onlyvisible", "--class", "firefox").split() or [""])[-1]
    if window:
        xdo("windowactivate", "--sync", window)
    shown = None
    for step in range(20):
        where = in_page(on_screen.format(video))
        if where:
            x, y = where.split(",")
            # A small step each time: a move, not a jump.
            xdo("mousemove", "--sync", str(int(x) + step % 5), y)
        time.sleep(0.3)
        value = in_page(state) or ""
        if value.startswith("block"):
            shown = value
            break
    print(f"  {'hovering the playing video shows it':<40} {shown or 'NO: ' + str(in_page(state))}")
    if not shown:
        return
    before = {p.name for p in engine_dl.rglob("*") if p.is_file()}
    real_click(f"{panel}.querySelector('.go')")
    listed = None
    for _ in range(60):
        value = in_page(state) or ""
        if value.split("|")[-1] or "Sent to dm" in value or "browser" in value:
            listed = value
            break
        time.sleep(1)
    lines = (listed or "").split("|")[-1]
    print(f"  {'clicking it':<40} {('lists ' + lines) if lines else (listed or 'NO answer')}")
    if lines:
        smallest = (f"[...{panel}.querySelectorAll('.item')].sort((a, b) => {{"
                    " const units = {B: 1, KB: 1024, MB: 1048576, GB: 1073741824};"
                    " const size = t => { const m = t.match(/— ([\\d.]+) (\\w+)$/); return m ? m[1] * (units[m[2]] || 1e15) : 1e18; };"
                    " return size(a.textContent) - size(b.textContent); })[0]")
        real_click(smallest)
    landed = common.wait_for(lambda: [p.name for p in engine_dl.rglob("*")
                                      if p.is_file() and p.name not in before and p.stat().st_size > 0], 240)
    print(f"  {'and the video lands in dm':<40} {landed or 'NO'}")


def mozilla_hosts():
    home = pathlib.Path.home()
    if sys.platform == "darwin":
        return home / "Library/Application Support/Mozilla/NativeMessagingHosts"
    return home / ".mozilla/native-messaging-hosts"


def main():
    global FIREFOX
    if not FIREFOX:
        FIREFOX = fetch_firefox()
    if not FIREFOX:
        print("no Firefox found, and none can be fetched for this platform — nothing was checked")
        return
    if not (EXT / "pkg" / "dm_catcher_bg.wasm").exists():
        sys.exit("no built extension at dist/firefox — run: sh scripts/package.sh")

    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-firefox-check-"))
    ext, browser_dl = work / "ext", work / "browser-downloads"
    shutil.copytree(EXT, ext)
    browser_dl.mkdir()
    prof = profile(browser_dl)

    registration, kept, host = None, None, None
    if NATIVE:
        if not common.BINARY.exists():
            sys.exit(f"no dm binary at {common.BINARY}")
        engine_home, engine_dl = work / "dm-home", work / "dm-downloads"
        engine_home.mkdir()
        engine_dl.mkdir()
        host = engine_home / "dm-desktop"
        shutil.copy2(common.BINARY, host)
        (engine_home / "settings.json").write_text(json.dumps({
            "download_dir": str(engine_dl), "categorize": False,
            "catch": {"enabled": True, "extensions": ["bin"]},
            # The dialog the app opens for a panel video is the window test's.
            "ask_panel_downloads": False,
        }))
        shim = engine_home / "host.sh"
        shim.write_text(f"#!/bin/sh\nexport DM_HOME='{engine_home}'\nexec '{host}' \"$@\"\n")
        shim.chmod(0o755)
        mozilla_hosts().mkdir(parents=True, exist_ok=True)
        registration = mozilla_hosts() / "com.dm.engine.json"
        if registration.exists():
            kept = registration.read_bytes()
        registration.write_text(json.dumps({
            "name": "com.dm.engine", "description": "dm (firefox-check)", "path": str(shim),
            "type": "stdio", "allowed_extensions": ["easy-download-manager@pranavo72bex.github.io"],
        }))
        server = common.Server(("127.0.0.1", 0), common.Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        origin = f"http://127.0.0.1:{server.server_address[1]}"

    firefox = subprocess.Popen(
        [FIREFOX, *([] if REAL else ["--headless"]), "--no-remote", "--profile", str(prof),
         "--start-debugger-server", str(PORT), "about:blank"],
        # A process group of its own, not a session: see chrome-check.py.
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, preexec_fn=os.setpgrp,
    )
    try:
        rdp = None
        for _ in range(60):
            try:
                rdp = Rdp(PORT)
                break
            except OSError:
                time.sleep(0.5)
        if rdp is None:
            sys.exit("Firefox never opened the debugging port")

        rdp.recv()
        addons = rdp.ask("root", "getRoot")["addonsActor"]
        reply = rdp.ask(addons, "installTemporaryAddon", addonPath=str(ext))
        print("=== install ===")
        if "error" in reply:
            print(json.dumps(reply, indent=2)[:2000])
            sys.exit("Firefox REFUSED the extension")
        addon_id = reply["addon"]["id"]
        mine = [a for a in rdp.ask("root", "listAddons").get("addons", []) if a.get("id") == addon_id]
        print(f"  accepted as {addon_id}; warnings: {mine[0].get('warnings') if mine else '?'}")

        watcher = rdp.ask(mine[0]["actor"], "getWatcher", isServerTargetSwitchingEnabled=True)["actor"]
        rdp.send(to=watcher, type="watchResources", resourceTypes=["console-message", "error-message"])
        rdp.send(to=watcher, type="watchTargets", targetType="frame")
        targets, logged = [], []
        rdp.sock.settimeout(1)
        deadline = time.time() + 12
        while time.time() < deadline:
            try:
                m = rdp.recv()
            except (socket.timeout, TimeoutError):
                continue
            if m.get("type") == "target-available-form":
                targets.append(m["target"])
            collect(m, logged)
        rdp.sock.settimeout(30)
        real = [t for t in targets if str(t.get("url", "")).startswith("moz-extension://")]
        if not real:
            sys.exit("Firefox never attached the background context")
        console = real[0]["consoleActor"]

        def evaluate(source, actor=None):
            # The reply names a resultID, and the result arrives later under
            # it; a result of an earlier evaluation that timed out may come
            # first, and must not be taken for this one's.
            to = actor or console
            rdp.send(to=to, type="evaluateJSAsync", text=source)
            wanted = None
            deadline = time.time() + 25
            while time.time() < deadline:
                try:
                    m = rdp.recv()
                except TimeoutError:
                    # A busy page (a video site's) answering slowly: no
                    # answer this time; its late result is told apart above.
                    break
                collect(m, logged)
                if wanted is None and m.get("from") == to and "resultID" in m and m.get("type") != "evaluationResult":
                    wanted = m["resultID"]
                elif m.get("type") == "evaluationResult" and m.get("resultID") == wanted:
                    return m
            return {"exception": "timed out"}

        def resolve(label, source, seconds=60):
            """This console hands back a promise rather than its value, so the
            value is parked on a global and read back."""
            evaluate("globalThis.__dm = undefined; Promise.resolve().then(() => (" + source + "))"
                     ".then(v => globalThis.__dm = String(v))"
                     ".catch(e => globalThis.__dm = 'threw: ' + (e && e.message || e)); 0")
            deadline = time.time() + seconds
            while time.time() < deadline:
                value = evaluate("globalThis.__dm === undefined ? 0 : globalThis.__dm").get("result")
                if isinstance(value, str) and value:
                    print(f"  {label:<40} {value}")
                    return value
                time.sleep(1)
            print(f"  {label:<40} (no answer in {seconds}s)")
            return None

        print("\n=== the WebAssembly ===")
        resolve("the loader ran", "typeof dmReady")
        resolve("handlers", "dmReady.then(m => Object.keys(m).filter(k => k.startsWith('on_')).join(','))")

        if NATIVE:
            print("\n=== the hand-off to dm ===")
            # A navigation download, which the browser starts and the
            # extension did not — so it is caught, unlike one from
            # downloads.download().
            resolve("a download is caught",
                    f"browser.tabs.create({{url: '{origin}/files/caught.bin'}}).then(() => 'opened')")
            landed = common.wait_for(lambda: common.whole(engine_dl, "caught.bin"), 60)
            print(f"  {'dm fetched it into its folder':<40} {'yes' if landed else 'NO'}")
            resolve("the app", "dmReady.then(m => m.app_status())")
            resolve("the browser's list",
                    "browser.downloads.search({}).then(l => JSON.stringify(l.map(d => [d.url, d.state])))")
            resolve("'Download link with dm'",
                    f"dmReady.then(m => m.on_menu_clicked({{menuItemId: 'dm-link', "
                    f"linkUrl: '{origin}/files/menu.bin', pageUrl: '{origin}/page.html'}}, {{}})).then(() => 'sent')")
            landed = common.wait_for(lambda: common.whole(engine_dl, "menu.bin"), 60)
            print(f"  {'and dm fetched it':<40} {'yes' if landed else 'NO'}")

            if REAL:
                real_menu_click(rdp, evaluate, resolve, origin, engine_dl)

            print("\n=== streams a page's player loads ===")
            resolve("a page whose player loads an HLS playlist",
                    f"browser.tabs.create({{url: '{origin}/stream.html'}}).then(() => 'opened')")
            seen = None
            for _ in range(20):
                seen = resolve("the background noted it, for the panel's list",
                               "browser.storage.session.get(null).then(o => Object.entries(o)"
                               ".filter(([k]) => k.startsWith('dm.streams.'))"
                               ".map(([, v]) => v.map(s => s.label + ' ' + s.url.split('/').pop()).join(', ')).join('; ') || '')",
                               seconds=5)
                if seen and "stream.m3u8" in seen:
                    break
                time.sleep(1)
            print(f"  {'Firefox reports the stream to dm':<40} {'yes' if seen and 'stream.m3u8' in seen else 'NO'}")

            print("\n=== the 'Download video' panel ===")
            resolve("a page with a video", f"browser.tabs.create({{url: '{origin}/video.html'}}).then(() => 'opened')")
            page = None
            for _ in range(30):
                tabs = rdp.ask("root", "listTabs").get("tabs", [])
                found = [t for t in tabs if str(t.get("url", "")).endswith("/video.html")]
                if found:
                    page = rdp.ask(found[0]["actor"], "getTarget")["frame"]["consoleActor"]
                    break
                time.sleep(0.5)

            def in_page(source):
                return (evaluate(source, page) or {}).get("result")

            loaded = page and common.wait_for(lambda: in_page("!!document.querySelector('dm-panel')") is True, 20)
            print(f"  {'the page script loads':<40} {'yes' if loaded else 'NO'}")
            if loaded:
                # The panel acts only on a person's input (a page's script
                # cannot press it), and Firefox's remote protocol has no
                # input of its own: the real pointer, through xdotool, moves
                # over the video and clicks the panel.
                state = ("(() => { const h = document.querySelector('dm-panel');"
                         " return getComputedStyle(h).display + '|' + h.shadowRoot.querySelector('.title').textContent + '|' + h.shadowRoot.querySelector('.sub').textContent; })()")
                on_screen = ("(() => {{ const r = {0}.getBoundingClientRect();"
                             " return Math.round(mozInnerScreenX + r.left + r.width / 2) + ','"
                             " + Math.round(mozInnerScreenY + r.top + r.height / 2); }})()")
                if not REAL:
                    print(f"  {'hovering the video shows it':<40} not checked: needs real input (DM_REAL_INPUT=1)")
                else:
                    window = (xdo("search", "--onlyvisible", "--class", "firefox").split() or [""])[-1]
                    if window:
                        xdo("windowactivate", "--sync", window)
                    shown = None
                    for step in range(10):
                        where = in_page(on_screen.format("document.getElementById('v')"))
                        if where:
                            x, y = where.split(",")
                            # A small step each time: a move, not a jump.
                            xdo("mousemove", "--sync", str(int(x) + step % 3), y)
                        time.sleep(0.3)
                        value = in_page(state) or ""
                        if value.startswith("block"):
                            shown = value
                            break
                    print(f"  {'hovering the video shows it':<40} {shown or 'NO: ' + str(in_page(state))}")
                    go = shown and in_page(on_screen.format("document.querySelector('dm-panel').shadowRoot.querySelector('.go')"))
                    if go:
                        x, y = go.split(",")
                        xdo("mousemove", "--sync", x, y)
                        time.sleep(0.2)
                        xdo("click", "1")
                    landed = go and common.wait_for(lambda: common.whole(engine_dl, "clip.mp4"), 60)
                    print(f"  {'its click lands the file in dm':<40} {'yes' if landed else 'NO'}")
                    print(f"  {'and it says':<40} {in_page(state)}")
                    # A page's own script pressing the panel hands nothing over.
                    in_page("document.querySelector('dm-panel').shadowRoot.querySelector('.go').click(), 1")
                    time.sleep(2)
                    again = len([p for p in engine_dl.rglob("clip*.mp4")])
                    print(f"  {'a click a page script makes does nothing':<40} {'yes' if again <= 1 else 'NO: ' + str(again)}")

            # One or more real pages, separated by spaces.
            for live in os.environ.get("DM_LIVE_PAGE", "").split():
                live_panel(rdp, evaluate, resolve, live, engine_dl)

        print("\n=== what the background logged ===")
        print("\n".join(f"  {line[:200]}" for line in logged[:40]) or "  (nothing)")
    finally:
        try:
            os.killpg(os.getpgid(firefox.pid), 15)
            firefox.wait(timeout=10)
        except Exception:
            os.killpg(os.getpgid(firefox.pid), 9)
        if registration is not None:
            if kept is not None:
                registration.write_bytes(kept)
            else:
                registration.unlink(missing_ok=True)
        if host is not None:
            subprocess.run(["pkill", "-f", str(host)], check=False)
        # The profile, the copied extension and dm's folders were the
        # check's own; nothing of them is wanted afterwards.
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(prof, ignore_errors=True)


main()
