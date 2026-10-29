"""Loads dm's extension into Brave and looks at what a page with a video gets.

Run by ext-check.sh inside the image. For each manifest variant ("old" keeps
use_dynamic_url, "fixed" is the one in the repository) it opens a page with a
<video> from a local server, moves the pointer over it, and reports whether
the panel's WebAssembly loaded (a <dm-panel> element appears) and every
console message about the extension's address.
"""
import base64, http.server, json, os, pathlib, shutil, socket, socketserver, struct, subprocess, sys, threading, time, urllib.request, hashlib

def say(*a): print(*a, flush=True)

# --- a minimal websocket client (the image has no websocket library)
class WS:
    def __init__(s, url):
        host, rest = url[5:].split("/", 1); h, p = host.split(":")
        s.k = socket.create_connection((h, int(p)), timeout=30)
        key = base64.b64encode(os.urandom(16)).decode()
        s.k.sendall(f"GET /{rest} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode())
        buf = b""
        while b"\r\n\r\n" not in buf: buf += s.k.recv(1)
        s.buf = b""; s.id = 0; s.events = []
    def send(s, method, params=None, session=None):
        s.id += 1; m = {"id": s.id, "method": method, "params": params or {}}
        if session: m["sessionId"] = session
        d = json.dumps(m).encode(); mask = os.urandom(4)
        n = len(d); hdr = b"\x81" + (bytes([0x80 | n]) if n < 126 else b"\xfe" + struct.pack(">H", n) if n < 65536 else b"\xff" + struct.pack(">Q", n))
        s.k.sendall(hdr + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(d)))
        return s.id
    def _read(s, n):
        while len(s.buf) < n:
            c = s.k.recv(65536)
            if not c: raise EOFError
            s.buf += c
        r, s.buf = s.buf[:n], s.buf[n:]; return r
    def recv(s):
        while True:
            b1, b2 = s._read(2); n = b2 & 127
            if n == 126: n = struct.unpack(">H", s._read(2))[0]
            elif n == 127: n = struct.unpack(">Q", s._read(8))[0]
            d = s._read(n)
            if b1 & 15 == 1: return json.loads(d)
    def call(s, method, params=None, session=None, wait=20):
        i = s.send(method, params, session); end = time.time() + wait
        s.k.settimeout(wait)
        while time.time() < end:
            m = s.recv()
            if m.get("id") == i: return m
            s.events.append(m)
    def drain(s, secs):
        end = time.time() + secs
        while time.time() < end:
            s.k.settimeout(max(0.1, end - time.time()))
            try: s.events.append(s.recv())
            except (socket.timeout, TimeoutError): break

os.makedirs("/srv", exist_ok=True)
pathlib.Path("/srv/page.html").write_text('<!doctype html><title>A video</title><body style="margin:0"><video id=v src="/clip.mp4" width=640 height=360 controls></video>')
pathlib.Path("/srv/bare.html").write_text('<!doctype html><title>No video yet</title><body style="margin:0;height:400px">')
pathlib.Path("/srv/clip.mp4").write_bytes(bytes((i * 7) & 255 for i in range(600_000)))
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(s, *a, **k): super().__init__(*a, directory="/srv", **k)
    def log_message(s, *a): pass
    def end_headers(s):
        # headers YouTube's watch page sends
        s.send_header("Content-Security-Policy", "script-src 'nonce-x' 'unsafe-inline' 'unsafe-eval' 'strict-dynamic' https: http:; require-trusted-types-for 'script'; object-src 'none'")
        s.send_header("Cross-Origin-Opener-Policy", "same-origin-allow-popups")
        s.send_header("Cross-Origin-Resource-Policy", "same-site")
        super().end_headers()
import ssl
subprocess.run("openssl req -x509 -newkey rsa:2048 -nodes -keyout /root/k.pem -out /root/c.pem -days 2 -subj /CN=www.youtube.com -addext subjectAltName=DNS:www.youtube.com", shell=True, check=True, capture_output=True)
srv = socketserver.ThreadingTCPServer(("127.0.0.1", 8443), H); srv.daemon_threads = True
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain("/root/c.pem", "/root/k.pem")
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
threading.Thread(target=srv.serve_forever, daemon=True).start()
# the page is on a youtube.com address over https, as the person's was
page = "https://www.youtube.com:8443/page.html"
REAL = "https://www.youtube.com/watch?v=pTq51r85KkM"

def variant(name):
    d = pathlib.Path(f"/root/ext-{name}"); shutil.rmtree(d, ignore_errors=True); shutil.copytree("/ext", d)
    m = json.loads((d / "manifest.json").read_text())
    for r in m["web_accessible_resources"]:
        if name == "old": r["use_dynamic_url"] = True
        else: r.pop("use_dynamic_url", None)
    (d / "manifest.json").write_text(json.dumps(m, indent=2))
    return d

def run(name, scenario="plain"):
    ext = variant(name); prof = f"/root/prof-{name}"; shutil.rmtree(prof, ignore_errors=True)
    env = dict(os.environ, HOME="/root/h"); os.makedirs("/root/h", exist_ok=True)
    target = REAL if scenario == "real" else page
    def launch(*extra):
        return subprocess.Popen(["xvfb-run", "-a", "/opt/brave.com/brave/brave", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
            f"--user-data-dir={prof}", f"--load-extension={ext}", "--disable-features=DisableLoadExtensionCommandLineSwitch",
            "--remote-debugging-port=9333", "--ignore-certificate-errors", "--no-first-run", "--no-default-browser-check", "--window-size=1000,700"]
            + ([] if scenario == "real" else ["--host-resolver-rules=MAP www.youtube.com 127.0.0.1"]) + list(extra),
            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    def debugger():
        for _ in range(90):
            try: return json.load(urllib.request.urlopen("http://127.0.0.1:9333/json/version", timeout=2))
            except Exception: time.sleep(1)
    if scenario == "private":
        # dm allowed in Private windows (a setting in its Details page), the video opened in one
        b = launch("about:blank"); ver = debugger(); ws = WS(ver["webSocketDebuggerUrl"]); time.sleep(5)
        try: ws.call("Browser.close", wait=10)
        except Exception: pass
        b.wait(30); subprocess.run("pkill -f brave", shell=True); time.sleep(2)
        pf = pathlib.Path(prof) / "Default/Preferences"; p = json.loads(pf.read_text())
        for k, v in p.get("extensions", {}).get("settings", {}).items():
            if "dm" in json.dumps(v.get("manifest", {}).get("name", "")) or v.get("path", "").startswith("/root/ext"): v["incognito"] = True
        pf.write_text(json.dumps(p))
        sp = pathlib.Path(prof) / "Default/Secure Preferences"
        if sp.exists():
            q = json.loads(sp.read_text())
            for k, v in q.get("extensions", {}).get("settings", {}).items():
                if v.get("path", "").startswith("/root/ext"): v["incognito"] = True
            sp.write_text(json.dumps(q))
        b = launch("--incognito", target)
    elif scenario == "restart":
        # the tab was open when Brave last closed; Brave restores it on the next start
        b = launch(target); ver = debugger(); ws = WS(ver["webSocketDebuggerUrl"]); time.sleep(8)
        try: ws.call("Browser.close", wait=10)
        except Exception: pass
        b.wait(30); subprocess.run("pkill -f brave", shell=True); time.sleep(2)
        b = launch("--restore-last-session")
    else:
        b = launch("about:blank")
    try:
        ver = debugger()
        if not ver: return None, ["Brave never opened its debugging port"], False
        ws = WS(ver["webSocketDebuggerUrl"]); msgs_pre = []
        # is the extension there at all?
        t = ws.call("Target.getTargets")["result"]["targetInfos"]
        loaded = any(x["url"].startswith("chrome-extension://") for x in t)
        pages = [x for x in t if x["type"] == "page" and "youtube" in x["url"]]
        r = pages[0]["targetId"] if scenario in ("restart", "private") and pages else ws.call("Target.createTarget", {"url": "about:blank"})["result"]["targetId"]
        s = ws.call("Target.attachToTarget", {"targetId": r, "flatten": True})["result"]["sessionId"]
        for dom in ("Runtime", "Log", "Page", "DOM", "Network"): ws.call(f"{dom}.enable", session=s)
        if scenario == "reload":
            # a tab open with no video, the extension reloaded under it, then a video appears
            ws.call("Page.navigate", {"url": page.replace("page.html", "bare.html")}, session=s)
            time.sleep(4)
            sw = [x for x in ws.call("Target.getTargets")["result"]["targetInfos"] if x["type"] == "service_worker"]
            ss = ws.call("Target.attachToTarget", {"targetId": sw[0]["targetId"], "flatten": True})["result"]["sessionId"]
            ws.send("Runtime.evaluate", {"expression": "chrome.runtime.reload()"}, ss)
            time.sleep(5)
            ws.call("Runtime.evaluate", {"expression": "(()=>{const v=document.createElement('video');v.src='/clip.mp4';v.width=640;v.height=360;v.controls=true;document.body.appendChild(v)})()"}, session=s)
        elif scenario in ("restart", "private"):
            if not pages: msgs_pre.append("restored tab not found")
        else:
            ws.call("Page.navigate", {"url": target}, session=s)
        time.sleep(15 if scenario == "real" else 4)
        found = False; msgs = list(msgs_pre)
        for _ in range(12):
            ws.call("Input.dispatchMouseEvent", {"type": "mouseMoved", "x": 300 + _ * 5, "y": 200}, session=s)
            ws.drain(1)
            v = ws.call("Runtime.evaluate", {"expression": "document.querySelector('dm-panel') !== null"}, session=s)
            if v and v.get("result", {}).get("result", {}).get("value"): found = True; break
        for e in ws.events:
            p = e.get("params", {})
            if e.get("method") == "Network.requestWillBeSent" and p["request"]["url"].startswith("chrome-extension://"): msgs.append("request " + p["request"]["url"])
            txt = p.get("entry", {}).get("text") or " ".join(str(a.get("value", a.get("description", ""))) for a in p.get("args", []))
            if txt and (os.environ.get("ALL") or "Unsafe attempt" in txt or "chrome-extension" in txt or "dm:" in txt): msgs.append(txt[:200])
        return loaded, msgs, found
    finally:
        b.terminate()
        try: b.wait(10)
        except Exception: b.kill()
        subprocess.run("pkill -f brave", shell=True)

say("== Brave", subprocess.run(["/opt/brave.com/brave/brave", "--version"], capture_output=True, text=True).stdout.strip())
bad = 0
scn = os.environ.get("SCENARIO", "plain")
for name, want in (("old", False), ("fixed", True)):
    loaded, msgs, found = run(name, scn)
    say(f"== [{scn}] manifest {name}: panel appeared: {found} | console: {sorted(set(msgs))[:3]}")
    if name == "fixed" and not (found and not [m for m in msgs if not m.startswith("request ")] if scn == "plain" else True): bad = 1
    if name == "old" and found: say("   (control did not fail: Brave here accepts the dynamic address)")
say("RESULT:", "PASS" if not bad else "FAIL")
sys.exit(bad)
