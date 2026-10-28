import http.server, json, os, pathlib, socket, socketserver, struct, subprocess, sys, threading, time
HOME = pathlib.Path("/root/h"); HOME.mkdir(exist_ok=True)
env = dict(os.environ, HOME=str(HOME))
env.pop("DM_HOME", None)
def say(*a): print(*a, flush=True)

# --- the page the person is on: a video in it, and the video itself
os.makedirs("/srv", exist_ok=True)
pathlib.Path("/srv/page.html").write_text('<!doctype html><title>A video</title><video src="/clip.mp4" controls></video>')
pathlib.Path("/srv/clip.mp4").write_bytes(bytes((i * 7) & 255 for i in range(600_000)))
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(s, *a, **k): super().__init__(*a, directory="/srv", **k)
    def log_message(s, *a): pass
    def end_headers(s):
        s.send_header("Accept-Ranges", "bytes"); super().end_headers()
srv = socketserver.ThreadingTCPServer(("127.0.0.1", 0), H); srv.daemon_threads = True
threading.Thread(target=srv.serve_forever, daemon=True).start()
page = f"http://127.0.0.1:{srv.server_address[1]}/page.html"

say("== /tmp:", subprocess.run("findmnt -no OPTIONS /tmp", shell=True, capture_output=True, text=True).stdout.strip(), "| TMPDIR:", os.environ.get("TMPDIR", "(unset)"))
# --- Brave opens the page (and so has a profile, which is where dm registers)
b = subprocess.run(["/opt/brave.com/brave/brave", "--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
                    "--dump-dom", page], env=env, capture_output=True, text=True, timeout=180)
say("== Brave", subprocess.run(["/opt/brave.com/brave/brave", "--version"], capture_output=True, text=True).stdout.strip(),
    "| loads the page:", "<video" in b.stdout)
# --- dm connects itself to the installed browsers
r = subprocess.run(["/opt/dm/AppRun", "--register"], env=env, capture_output=True, text=True, timeout=60)
say("== dm --register:", (r.stdout + r.stderr).strip().replace("\n", " | ")[:300])
found = subprocess.run(f"find {HOME}/.config -name '*.json' -path '*NativeMessagingHosts*' 2>/dev/null | head -5", shell=True, capture_output=True, text=True).stdout.split()
say("   registered for:", [p.split(".config/")[1].split("/NativeMessagingHosts")[0] for p in found])

# --- dm, as the AppImage starts it, on a display of its own
app = subprocess.Popen(["xvfb-run", "-a", "/opt/dm/AppRun", "--control", "0"], env=env, stdout=open("/tmp/dm.out", "w") if False else subprocess.DEVNULL, stderr=subprocess.PIPE)
data = HOME / ".local/share/dm-engine"
for _ in range(300):
    if (data / "control.port").exists() and (data / "control.port").read_text().strip(): break
    time.sleep(0.5)
else:
    say("dm never opened its control port:", app.stderr.read(1500).decode(errors="replace")); sys.exit(1)
c = socket.create_connection(("127.0.0.1", int((data / "control.port").read_text())), timeout=120); rd = c.makefile("r")
def call(m, **k):
    c.sendall((json.dumps({"id": 1, "method": m, "params": k}) + "\n").encode()); return json.loads(rd.readline())
call("auth", token=(data / "control.token").read_text().strip())
# What the extension's "Download video" panel sends over the native host Brave starts
# for it (dm_protocol::Start): the page, the page again as its referrer, a quality.
hello = json.dumps({"protocolVersion": 1, "type": "engine.hello"}).encode()
start = json.dumps({"protocolVersion": 1, "type": "download.start", "id": "panel-1", "url": page,
                    "referrer": page, "quality": "best"}).encode()
host = subprocess.run(["/opt/dm/AppRun", "chrome-extension://fojpndfjodldkfbamkofdmnfedgjjejo/"], env=env,
                      input=b"".join(struct.pack("<I", len(f)) + f for f in (hello, start)),
                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)
answers = []; buf = host.stdout
while len(buf) >= 4:
    n = struct.unpack("<I", buf[:4])[0]; answers.append(json.loads(buf[4:4 + n])); buf = buf[4 + n:]
answer = answers[-1] if answers else {"no answer": host.stderr.decode()[:200]}
say("== the extension hands the page to dm (native host answers):", json.dumps(answer)[:200])
last = None; t0 = time.time()
while time.time() - t0 < 300:
    rows = call("queue.list").get("result") or []
    if rows:
        k = rows[0]["state"]["kind"]
        if k != last: say(f"   [{time.time()-t0:5.1f}s] {k}"); last = k
        if k in ("completed", "failed", "cancelled"): break
    time.sleep(1)
row = (call("queue.list").get("result") or [{}])[0]
ui = [r for r in (call("ui.inspect").get("result") or {}).get("rows", [])]
say("== what dm recorded:", json.dumps(row.get("state"))[:700])
say("== what the window says in the Status column:", ui[0]["status"] if ui else None)
say("== yt-dlp dm fetched:", subprocess.run(f"ls {data}/tools 2>/dev/null | tr '\\n' ' '", shell=True, capture_output=True, text=True).stdout)
if row.get("state", {}).get("kind") == "completed": say("== the file:", row.get("filename"), row.get("progress", {}).get("downloaded"), "bytes")
app.terminate()
