"""Stress run: one Brave session, real YouTube, many rounds of what a person
does, looking for "Unsafe attempt to load URL" from any tab. Run by
stress.sh for the old (use_dynamic_url) and fixed manifests.

ROUNDS (default 40) rounds; each picks one action at random:
open a watch page in a new tab, click a related video (YouTube's own
navigation), reload, go back, open an embed, stop the extension's service
worker, close a tab, raise the panel by moving over the player.
"""
import base64, socket, struct, json, os, pathlib, random, shutil, subprocess, sys, time, urllib.request
src = pathlib.Path("/scripts/ext-check.py").read_text()
exec(src[src.index("class WS"):src.index("os.makedirs(\"/srv\"")])  # the websocket client
def say(*a): print(*a, flush=True)
import http.server, socketserver, threading
LOCAL = os.environ.get("LOCAL")
if LOCAL:
    # the local page, at a youtube.com address over https, as ext-check.py serves it
    exec(src[src.index("os.makedirs(\"/srv\""):src.index("def variant")])

VIDEOS = ["pTq51r85KkM", "dQw4w9WgXcQ", "jNQXAC9IVRw", "9bZkp7q19f0", "kJQP7kiw5Fk", "M7lc1UVf-VE"]
name = os.environ["VARIANT"]; rounds = int(os.environ.get("ROUNDS", 40)); random.seed(int(os.environ.get("SEED", 1)))
# "fixed" is the repository's extension as it is; "old" the one in /ext-old
ext = pathlib.Path(f"/root/ext-{name}"); shutil.rmtree(ext, ignore_errors=True)
shutil.copytree("/ext-old" if name == "old" else "/ext", ext)
prof = f"/root/prof-{name}"
b = subprocess.Popen(["xvfb-run", "-a", "/opt/brave.com/brave/brave", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
    f"--user-data-dir={prof}", f"--load-extension={ext}", "--disable-features=DisableLoadExtensionCommandLineSwitch",
    "--remote-debugging-port=9333"] + (["--ignore-certificate-errors", "--host-resolver-rules=MAP www.youtube.com 127.0.0.1"] if LOCAL else []) + ["--no-first-run", "--no-default-browser-check", "--window-size=1200,800", "--autoplay-policy=no-user-gesture-required", "about:blank"],
    env=dict(os.environ, HOME="/root/h"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for _ in range(90):
    try: ver = json.load(urllib.request.urlopen("http://127.0.0.1:9333/json/version", timeout=2)); break
    except Exception: time.sleep(1)
ws = WS(ver["webSocketDebuggerUrl"])
# every tab and frame, including ones opened later, reports its log here
ws.call("Target.setDiscoverTargets", {"discover": True})
ws.call("Target.setAutoAttach", {"autoAttach": True, "waitForDebuggerOnStart": False, "flatten": True})
tabs = {}  # targetId -> sessionId
def attach(tid):
    s = ws.call("Target.attachToTarget", {"targetId": tid, "flatten": True})["result"]["sessionId"]
    for d in ("Runtime", "Log", "Network", "Page"): ws.call(f"{d}.enable", session=s)
    ws.call("Target.setAutoAttach", {"autoAttach": True, "waitForDebuggerOnStart": False, "flatten": True}, session=s)
    tabs[tid] = s; return s
def watch(v): return page + "?v=" + v if LOCAL else f"https://www.youtube.com/watch?v={v}"
def new_tab(url):
    tid = ws.call("Target.createTarget", {"url": "about:blank"})["result"]["targetId"]
    s = attach(tid); ws.call("Page.navigate", {"url": url}, session=s); return tid
def ev(s, js):
    r = ws.call("Runtime.evaluate", {"expression": js, "returnByValue": True, "awaitPromise": True}, session=s, wait=20)
    return (r or {}).get("result", {}).get("result", {}).get("value")
def hover(s):
    for i in range(4): ws.call("Input.dispatchMouseEvent", {"type": "mouseMoved", "x": 300 + 20 * i, "y": 250}, session=s)

hits = []; panels = 0; checks = 0
where = {}  # sessionId -> what it is attached to
new_tab(watch(VIDEOS[0])); ws.drain(10)
for n in range(rounds):
    if not tabs: new_tab(watch(random.choice(VIDEOS)))
    tid = random.choice(list(tabs)); s = tabs[tid]
    act = random.choice(["open", "related", "reload", "back", "embed", "worker", "close", "hover", "hover"])
    try:
        if act == "open": tid = new_tab(watch(random.choice(VIDEOS))); s = tabs[tid]
        elif act == "related": ev(s, "(()=>{const a=[...document.querySelectorAll('a#thumbnail[href*=\"watch\"]')];if(a.length){a[Math.floor(Math.random()*a.length)].click();return a.length}return 0})()")
        elif act == "reload": ws.call("Page.reload", {}, session=s)
        elif act == "back": ev(s, "history.back()")
        elif act == "embed": ws.call("Page.navigate", {"url": f"https://www.youtube.com/embed/{random.choice(VIDEOS)}"}, session=s)
        elif act == "worker":
            for t in ws.call("Target.getTargets")["result"]["targetInfos"]:
                if t["type"] == "service_worker" and t["url"].startswith("chrome-extension://"):
                    ws.call("Target.closeTarget", {"targetId": t["targetId"]})
        elif act == "close" and len(tabs) > 1:
            ws.call("Target.closeTarget", {"targetId": tid}); tabs.pop(tid); s = None
        ws.drain(6)
        if s:
            hover(s); ws.drain(3); checks += 1
            if ev(s, "document.querySelector('dm-panel') !== null"): panels += 1
            else: say("   no panel:", ev(s, "location.href + ' | ' + document.title + ' | videos=' + document.querySelectorAll('video').length"))
    except Exception as e:
        say(f"round {n} {act}: harness error {e!r}"); ws.drain(2)
    for e in ws.events:
        if e.get("method") == "Target.attachedToTarget":
            ti = e["params"]["targetInfo"]; where[e["params"]["sessionId"]] = f'{ti["type"]} {ti["url"][:80]}'
    for e in ws.events:
        p = e.get("params", {}); txt = p.get("entry", {}).get("text") or ""
        if e.get("method") == "Runtime.consoleAPICalled":
            txt = " ".join(str(a.get("value", a.get("description", ""))) for a in p.get("args", []))
        if "Unsafe attempt" in txt or txt.startswith("dm:"):
            src = where.get(e.get("sessionId"), "tab" if e.get("sessionId") in tabs.values() else e.get("sessionId"))
            hits.append((n, act, txt[:300])); say(f"round {n} after {act} [{src}] {p.get('entry', {}).get('source', '')}: {txt[:160]}")
    ws.events.clear()
    say(f"round {n:3} {act:8} tabs={len(tabs)}")
say(f"== {name}: {rounds} rounds, panel seen {panels}/{checks} checks, {len(hits)} errors")
for h in hits[:10]: say("  ", h)
b.terminate(); subprocess.run("pkill -f brave", shell=True)
sys.exit(1 if hits else 0)
