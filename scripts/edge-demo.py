#!/usr/bin/env python3
"""dm in Microsoft Edge, run the way a person tests it, slowly and in sight.

    python3 scripts/edge-demo.py            # watch Edge and the dm window
    python3 scripts/edge-demo.py --fast     # the same, with short pauses

Opens a real Edge window (its own throwaway profile, with dm's extension
loaded, labelled as a test window) on a page of links. A pointer moves to
each link and clicks it, and what is happening is written on the page and in
the terminal. The downloads go to the dm that is running on this computer,
whose window shows them arrive; the script then looks at what Edge itself
kept. Needs `sh scripts/package.sh` run and dm installed and registered for
Edge (`dm-desktop --register`).

What it shows, in order:
  1. a small zip from a fast server: dm takes it, Edge keeps nothing;
  2. 3 MB from a server that holds each connection to 1 MB/s: dm opens
     helping connections and takes about a second, not three;
  3. a PDF, which Edge would open in the tab: dm takes it, the tab stays;
  4. Alt held while clicking: Edge keeps its own download, as asked;
  5. any other type (a .txt and a .md): dm takes them too, Edge keeps nothing;
  6. the extension's popup, with its switches.
"""
import glob
import importlib.util
import json
import os
import pathlib
import shutil
import socketserver
import http.server
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("chrome_check", ROOT / "scripts" / "chrome-check.py")
cc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cc)

FAST = "--fast" in sys.argv
PAUSE = 0.8 if FAST else 2.6
MB = 1024 * 1024
EDGE = os.environ.get("CHROME_PATH") or "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"
EXT = ROOT / "dist" / "chrome"
DM_DOWNLOADS = pathlib.Path.home() / "Downloads" / "dm"

FILES = {
    "plain.zip": (3 * MB // 2, "application/zip", True, False),
    "slow.bin": (3 * MB, "application/octet-stream", True, True),
    "manual.pdf": (MB, "application/pdf", False, False),
    "keepme.bin": (MB, "application/octet-stream", True, False),
    "notes.txt": (200_000, "text/plain", True, False),
    "readme.md": (50_000, "text/markdown", True, False),
}
log = []


def content(name, start, end):
    """Bytes start..end of the file: the same byte at the same offset on every connection."""
    unit = (name.encode() * 4096)[:4096]
    shift = start % 4096
    return (unit[shift:] + unit * ((end - start) // 4096 + 1))[:end - start + 1]


PAGE = """<!doctype html><meta charset=utf-8><title>dm test window (not your Edge profile)</title>
<style>
 body{font:17px -apple-system,Segoe UI,sans-serif;margin:0;padding:96px 48px 48px;background:#f6f7f9;color:#1b1b1b}
 #bar{position:fixed;top:0;left:0;right:0;background:#0f6cbd;color:#fff;padding:12px 24px;z-index:9;box-shadow:0 2px 8px #0004}
 #bar b{display:block;font-size:13px;opacity:.85;letter-spacing:.04em;text-transform:uppercase}
 #say{font-size:19px;margin-top:2px}
 a.l{display:block;margin:14px 0;padding:16px 20px;background:#fff;border:1px solid #d0d0d0;border-radius:8px;
     text-decoration:none;color:#0f6cbd;max-width:640px}
 a.l small{display:block;color:#555;margin-top:4px}
 a.l.now{outline:3px solid #0f6cbd;background:#eaf3fc}
</style>
<div id=bar><b>dm test window &mdash; a separate Edge profile, not yours</b><div id=say>Starting&hellip;</div></div>
<a class=l id=plain.zip href="/files/plain.zip">1 &middot; A small zip (1.5 MB) <small>from a fast server</small></a>
<a class=l id=slow.bin href="/files/slow.bin">2 &middot; A 3 MB file <small>from a server that holds each connection to 1 MB/s</small></a>
<a class=l id=manual.pdf href="/files/manual.pdf">3 &middot; A PDF <small>Edge would open this in the tab</small></a>
<a class=l id=keepme.bin href="/files/keepme.bin">4 &middot; Alt + click <small>asks Edge to keep the download itself</small></a>
<a class=l id=notes.txt href="/files/notes.txt">5 &middot; A text file <small>any type is dm's, not only archives and videos</small></a>
<a class=l id=readme.md href="/files/readme.md">6 &middot; A Markdown file <small>the same</small></a>
"""


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        if path == "/":
            page = PAGE.encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        name = path.rsplit("/", 1)[-1]
        if name not in FILES:
            self.send_error(404)
            return
        size, kind, attach, paced = FILES[name]
        start, end = 0, size - 1
        ranged = self.headers.get("Range", "")
        if ranged.startswith("bytes="):
            first, _, last = ranged[6:].partition("-")
            start = int(first or 0)
            end = min(int(last), end) if last else end
        log.append((time.monotonic(), self.client_address[1], self.headers.get("User-Agent", "")[:3], name))
        self.send_response(206 if ranged else 200)
        self.send_header("Content-Type", kind)
        if attach:
            self.send_header("Content-Disposition", f'attachment; filename="{name}"')
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        if ranged:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        try:
            for at in range(start, end + 1, 32 * 1024):
                self.wfile.write(content(name, at, min(at + 32 * 1024, end + 1) - 1))
                if paced:
                    time.sleep(0.03125)
        except (BrokenPipeError, ConnectionResetError):
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        pass


def say(tab, text, link=None, quiet=False):
    """Written on the page where the person watching is looking, and here."""
    if not quiet:
        print(f"  {text}", flush=True)
    try:
        tab.evaluate(
            "(() => { document.getElementById('say').textContent = %s;"
            " document.querySelectorAll('a.l').forEach(a => a.classList.toggle('now', a.id === %s)); })()"
            % (json.dumps(text), json.dumps(link or "")))
    except RuntimeError:
        pass


def glide(tab, frm, to, steps=26):
    """The pointer travels, as a hand moves it, instead of appearing."""
    for i in range(1, steps + 1):
        t = i / steps
        ease = t * t * (3 - 2 * t)
        cc.mouse(tab, "mouseMoved", frm[0] + (to[0] - frm[0]) * ease, frm[1] + (to[1] - frm[1]) * ease)
        time.sleep(0.012)


pointer = [60.0, 60.0]


def press(tab, link, alt=False):
    x, y = tab.evaluate("""(() => { const el = document.getElementById(%s); el.scrollIntoView({block: 'center'});
        const r = el.getBoundingClientRect(); return [r.left + 40, r.top + r.height / 2]; })()""" % json.dumps(link))
    glide(tab, pointer, (x, y))
    pointer[:] = [x, y]
    time.sleep(0.5)
    mods = 1 if alt else 0
    tab.call("Input.dispatchMouseEvent", type="mousePressed", x=x, y=y, button="left", clickCount=1, modifiers=mods)
    tab.call("Input.dispatchMouseEvent", type="mouseReleased", x=x, y=y, button="left", clickCount=1, modifiers=mods)


def dm_has(name, size):
    for found in glob.glob(str(DM_DOWNLOADS / "*" / name)):
        if os.path.getsize(found) == size:
            return found
    return None


def edge_has(name, size):
    p = pathlib.Path.home() / "Downloads" / name
    return p if p.exists() and p.stat().st_size == size else None


def main():
    if not EXT.exists():
        sys.exit("no dist/chrome: run  sh scripts/package.sh")
    mark_dm = set(glob.glob(str(DM_DOWNLOADS / "*" / "*")))
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-edge-demo-"))
    server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_address[1]}"
    # dm's own window to the front: a second launch hands over and shows it.
    dm = pathlib.Path.home() / ".local/dm-test/dm-desktop"
    if dm.exists():
        subprocess.Popen([str(dm)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print("\nWatch two windows: Microsoft Edge (a test profile, blue banner) and dm.\n", flush=True)
    time.sleep(2.0)
    args = [EDGE, f"--user-data-dir={work / 'profile'}", f"--load-extension={EXT}",
            f"--disable-extensions-except={EXT}", "--remote-debugging-port=0",
            "--no-first-run", "--no-default-browser-check", "--window-size=1100,800",
            "--window-position=30,40",
            "--disable-features=DisableLoadExtensionCommandLineSwitch,HttpsUpgrades",
            "--enable-unsafe-extension-debugging", "about:blank"]
    browser = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, preexec_fn=os.setpgrp)
    results = []
    try:
        base = cc.devtools(work / "profile")
        workers = lambda: [t for t in cc.targets(base) if t["type"] == "service_worker" and t["url"].endswith("/background.js")]
        found = cc.wait_for(workers, 30)
        if not found:
            sys.exit("the extension's worker never started in this Edge")
        worker = cc.Target(found[0]["webSocketDebuggerUrl"])
        worker.call("Runtime.enable")
        cc.wait_for(lambda: worker.evaluate("typeof dmReady") == "object", 15)
        ext_id = urllib_host(found[0]["url"])
        tab = cc.open_tab(base, origin + "/")
        cc.loaded(tab, "/", "#plain\\.zip")
        time.sleep(1.5)
        say(tab, "Ready. A pointer will click each link, and dm's window shows what it takes.")
        time.sleep(PAUSE)

        def edge_kept(name):
            return worker.evaluate(
                "chrome.downloads.search({}).then(l => l.filter(d => d.url.endsWith(%s) && d.state === 'complete').length)"
                % json.dumps("/" + name))

        # 1 ------------------------------------------------------------
        name, size = "plain.zip", FILES["plain.zip"][0]
        print("\n1. A small zip from a fast server", flush=True)
        say(tab, "Step 1 of 5: clicking a small zip (fast server)", name)
        t0 = time.monotonic()
        press(tab, name)
        ok = cc.wait_for(lambda: dm_has(name, size), 30)
        took = time.monotonic() - t0
        kept = edge_kept(name)
        say(tab, f"dm downloaded {name} in {took:.1f} s. Edge kept {kept} copies." if ok else f"dm did NOT take {name}", name)
        results.append((name, bool(ok) and not kept and not edge_has(name, size), f"{took:.1f} s in dm, Edge kept {kept}"))
        shot(tab, work, 1)
        time.sleep(PAUSE)

        # 2 ------------------------------------------------------------
        name, size = "slow.bin", FILES["slow.bin"][0]
        print("\n2. 3 MB from a server that holds each connection to 1 MB/s", flush=True)
        say(tab, "Step 2 of 5: 3 MB from a slow server. One connection would take 3+ seconds.", name)
        mark = len(log)
        t0 = time.monotonic()
        press(tab, name)
        ok = cc.wait_for(lambda: dm_has(name, size), 40)
        took = time.monotonic() - t0
        ports = {p for (_, p, ua, n) in log[mark:] if n == name and ua.startswith("dm/")}
        say(tab, f"dm took {name} in {took:.1f} s on {len(ports)} connections (one connection: 3+ s)." if ok else "dm did NOT take it", name)
        results.append((name, bool(ok) and took < 2.8 and len(ports) >= 4 and not edge_kept(name), f"{took:.1f} s on {len(ports)} connections"))
        shot(tab, work, 2)
        time.sleep(PAUSE)

        # 3 ------------------------------------------------------------
        name, size = "manual.pdf", FILES["manual.pdf"][0]
        print("\n3. A PDF, which Edge opens in the tab", flush=True)
        say(tab, "Step 3 of 5: a PDF. Edge would show it in this tab; dm should take it instead.", name)
        t0 = time.monotonic()
        press(tab, name)
        ok = cc.wait_for(lambda: dm_has(name, size), 30)
        took = time.monotonic() - t0
        still = tab.evaluate("location.pathname") == "/"
        say(tab, f"dm took {name} in {took:.1f} s; this tab stayed on the page." if ok and still else "not as expected", name)
        results.append((name, bool(ok) and still, f"{took:.1f} s in dm, tab {'stayed' if still else 'MOVED'}"))
        shot(tab, work, 3)
        time.sleep(PAUSE)

        # 4 ------------------------------------------------------------
        name, size = "keepme.bin", FILES["keepme.bin"][0]
        print("\n4. Alt + click: leave it with Edge", flush=True)
        say(tab, "Step 4 of 5: Alt + click. This one is meant to stay in Edge.", name)
        press(tab, name, alt=True)
        ok = cc.wait_for(lambda: edge_kept(name) or edge_has(name, size), 30)
        say(tab, "Edge kept it, as asked. dm did not take it." if ok and not dm_has(name, size) else "not as expected", name)
        results.append((name, bool(ok) and not dm_has(name, size), "Edge kept it; dm did not"))
        shot(tab, work, 4)
        time.sleep(PAUSE)

        # 5 ------------------------------------------------------------
        print("\n5. Any other type: dm takes it", flush=True)
        for name in ("notes.txt", "readme.md"):
            size = FILES[name][0]
            say(tab, f"Step 5 of 5: a {name.rsplit('.', 1)[1]} file. Every type is dm's.", name)
            press(tab, name)
            ok = cc.wait_for(lambda: dm_has(name, size), 30)
            kept = edge_kept(name) or edge_has(name, size)
            say(tab, f"dm took {name}; Edge kept nothing." if ok and not kept else "not as expected", name)
            results.append((name, bool(ok) and not kept, "dm took it; Edge kept nothing"))
            shot(tab, work, 5)
            time.sleep(PAUSE)

        # 6 ------------------------------------------------------------
        print("\n6. The extension's popup", flush=True)
        popup = cc.open_tab(base, f"chrome-extension://{ext_id}/popup.html")
        cc.loaded(popup, "/popup.html", "#catching")
        time.sleep(1.0)
        rows = popup.evaluate("[...document.querySelectorAll('label.row span')].map(s => s.textContent)")
        for row in rows:
            print(f"   - {row}", flush=True)
        results.append(("popup", "Catch downloads" in rows, f"{len(rows)} switches"))
        shot(popup, work, 6)
        time.sleep(PAUSE * 1.5)

        print("\n" + "=" * 60)
        for name, ok, detail in results:
            print(f"  {'PASS' if ok else 'FAIL'}  {name:<12} {detail}")
        print(f"\n  {sum(1 for r in results if r[1])} of {len(results)} as a person would expect.")
        print(f"  Pictures of Edge at each step: {work}\n", flush=True)
        say(tab, "Done. This window closes in a few seconds.", quiet=True)
        time.sleep(6 if not FAST else 1)
    finally:
        try:
            os.killpg(browser.pid, 15)
        except ProcessLookupError:
            pass
        # What the demo put on disk, and nothing else.
        for found in set(glob.glob(str(DM_DOWNLOADS / "*" / "*"))) - mark_dm:
            if os.path.basename(found) in FILES:
                os.remove(found)
        for name in FILES:
            p = pathlib.Path.home() / "Downloads" / name
            if p.exists() and p.stat().st_size == FILES[name][0]:
                p.unlink()
        server.shutdown()
    sys.exit(0 if all(r[1] for r in results) else 1)


def urllib_host(url):
    return urllib.parse.urlparse(url).hostname


def shot(tab, work, n):
    import base64
    try:
        data = tab.call("Page.captureScreenshot", format="png")["data"]
        (work / f"step-{n}.png").write_bytes(base64.b64decode(data))
    except RuntimeError:
        pass


if __name__ == "__main__":
    main()
