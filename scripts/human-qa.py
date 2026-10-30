#!/usr/bin/env python3
"""Every row of docs/behaviour-contract.md, checked as a person checks it.

    python3 scripts/human-qa.py                    # the whole list, in sight
    python3 scripts/human-qa.py --save-baseline    # the window's pictures, once approved

dm's real window is shown and driven through its own buttons and dialogs
(Add URL, Download details, Stop, Resume, Remove ...), slowly enough to
watch, against a server on 127.0.0.1 that logs every connection and request.
Then Microsoft Edge, in a window of its own with dm's extension loaded, runs
the browser checks and a real YouTube video (`youtube-check.py`). Never
headless: the point is to see what a person sees.

The run fails if any row of the contract was not checked, so the list is
gone through whole every time; a row the script cannot reach on this
computer (no Edge, no network) is a failure, not a skip.

Needs a debug build (`cd desktop && cargo build -p dm-desktop`), the
extension (`sh scripts/package.sh`) and Microsoft Edge. DM_BINARY and
DM_QA_EDGE name others. The download folder and dm's home are made under
/tmp/dm-human-qa and removed at the end. Standard library only.
"""
import hashlib, json, os, pathlib, re, shutil, signal, socket, struct, subprocess, sys, threading, time, zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTRACT = ROOT / "docs" / "behaviour-contract.md"
BASELINE = ROOT / "docs" / "behaviour-baseline"
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")
EDGE = os.environ.get("DM_QA_EDGE") or {
    "darwin": "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "linux": "/usr/bin/microsoft-edge",
    "win32": r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
}.get(sys.platform, "")
VIDEO = os.environ.get("DM_QA_VIDEO", "https://www.youtube.com/watch?v=jNQXAC9IVRw")
PACE = float(os.environ.get("DM_QA_PACE", "1.0"))   # how long each step stays on screen
SAVE_BASELINE = "--save-baseline" in sys.argv
MiB = 1024 * 1024
WORK = pathlib.Path("/tmp/dm-human-qa")


def pause(seconds):
    time.sleep(seconds * PACE)


# ---------------------------------------------------------------- results

RESULTS = {}


def check(row, what, ok, detail=""):
    RESULTS.setdefault(row, []).append((bool(ok), what, detail))
    print(f"  {'PASS' if ok else 'FAIL'}  {row}  {what}" + (f" — {detail}" if detail else ""), flush=True)
    return ok


# ---------------------------------------------------------------- the server

def body(name, size):
    seed = int(hashlib.sha256(name.encode()).hexdigest()[:8], 16)
    block = bytes((seed + i * 131) & 255 for i in range(65536))
    return (block * (size // 65536 + 1))[:size]


# name: (size, bytes a second per connection or 0, kind)
FILES = {
    "big.bin": (96 * MiB, 1 * MiB, "plain"),
    "fast.bin": (32 * MiB, 0, "plain"),
    "slowconn.bin": (32 * MiB, 2 * MiB, "plain"),
    "limited.bin": (32 * MiB, 0, "limited"),      # two at once; more get 503
    "norange.bin": (8 * MiB, 0, "norange"),
    "lie.bin": (4 * MiB, 0, "lie"),               # says 1 MiB more than it sends
    "target.bin": (6 * MiB, 0, "plain"),          # reached through /go/target
    "report": (3 * MiB, 0, "named"),              # "Report 2026.pdf"
    "again.bin": (4 * MiB, 0, "plain"),           # added twice
    "later.bin": (4 * MiB, 0, "plain"),           # Add for later, then Start queue
    "limit.bin": (3 * MiB, 0, "plain"),           # fetched under Low speed
    "crash.bin": (64 * MiB, 512 * 1024, "plain"),
    "stopme.bin": (64 * MiB, 512 * 1024, "plain"),    # stopped, then Remove
    **{f"q{i}.bin": (8 * MiB, 256 * 1024, "plain") for i in range(1, 6)},
}
DATA = {n: body(n, s) for n, (s, _, _) in FILES.items()}
LOG, LOCK, ACTIVE, CONNS = [], threading.Lock(), {}, [0]
T0 = time.time()


def log(**k):
    k["t"] = round(time.time() - T0, 3)
    with LOCK:
        LOG.append(k)


def serve(c, n):
    try:
        while True:
            head = b""
            while b"\r\n\r\n" not in head:
                b = c.recv(1)
                if not b:
                    return
                head += b
            lines = head.decode("latin1").split("\r\n")
            method, path, _ = lines[0].split(" ", 2)
            hdr = {l.split(":", 1)[0].lower(): l.split(":", 1)[1].strip() for l in lines[1:] if ":" in l}
            name = path.split("?", 1)[0].rsplit("/", 1)[-1]
            rng = hdr.get("range")
            log(ev="req", conn=n, name=name, method=method, range=rng)
            if path == "/go/target":
                c.sendall(b"HTTP/1.1 302 Found\r\nLocation: /files/target.bin\r\nContent-Length: 0\r\n\r\n")
                continue
            if name not in FILES:
                c.sendall(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                continue
            size, rate, kind = FILES[name]
            counted = kind == "limited" and rng != "bytes=0-0" and method != "HEAD"
            if counted:
                with LOCK:
                    busy = ACTIVE.get(name, 0)
                    if busy < 2:
                        ACTIVE[name] = busy + 1
                if busy >= 2:
                    log(ev="refused", conn=n, name=name)
                    c.sendall(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n")
                    continue
            if kind == "norange":
                rng = None
            first, last = 0, size - 1
            if rng and rng.startswith("bytes="):
                a, b2 = rng[6:].split("-", 1)
                first = int(a or 0)
                last = min(int(b2), size - 1) if b2 else size - 1
            stated = last - first + 1 + (MiB if kind == "lie" and not rng else 0)
            h = (f"HTTP/1.1 {'206 Partial Content' if rng else '200 OK'}\r\n"
                 f"Content-Type: application/octet-stream\r\nContent-Length: {stated}\r\n")
            if kind != "norange":
                h += "Accept-Ranges: bytes\r\n"
            if rng:
                h += f"Content-Range: bytes {first}-{last}/{size}\r\n"
            if kind == "named":
                h += 'Content-Disposition: attachment; filename="Report 2026.pdf"\r\n'
            try:
                c.sendall((h + "\r\n").encode())
                if method == "HEAD":
                    continue
                pos, sent, start = first, 0, time.time()
                while pos <= last:
                    chunk = DATA[name][pos:min(pos + 65536, last + 1)]
                    c.sendall(chunk)
                    pos += len(chunk)
                    sent += len(chunk)
                    if rate:
                        ahead = sent / rate - (time.time() - start)
                        if ahead > 0:
                            time.sleep(ahead)
            finally:
                if counted:
                    with LOCK:
                        ACTIVE[name] -= 1
            log(ev="done", conn=n, name=name, bytes=sent)
            if kind == "lie" and not rng:
                c.close()
                return
    except OSError:
        pass


def start_server():
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(128)

    def accept():
        while True:
            c, _ = srv.accept()
            with LOCK:
                CONNS[0] += 1
                n = CONNS[0]
            log(ev="accept", conn=n)
            threading.Thread(target=serve, args=(c, n), daemon=True).start()

    threading.Thread(target=accept, daemon=True).start()
    return f"http://127.0.0.1:{srv.getsockname()[1]}"


def requests(name, since=0):
    return [e for e in LOG[since:] if e.get("name") == name and e["ev"] == "req"]


def connections(name):
    return {e["conn"] for e in requests(name)}


# ---------------------------------------------------------------- dm, driven

class Dm:
    def __init__(self, home):
        self.home, self.proc, self.conn, self.replies = home, None, None, None

    def start(self):
        (self.home / "control.port").unlink(missing_ok=True)
        self.proc = subprocess.Popen([str(BINARY), "--control", "0"], env=dict(os.environ, DM_HOME=str(self.home)),
                                     stdout=subprocess.DEVNULL, stderr=open(WORK / "dm-stderr.txt", "a"))
        port = self.home / "control.port"
        for _ in range(200):
            if port.exists() and port.read_text().strip():
                break
            time.sleep(0.1)
        self.conn = socket.create_connection(("127.0.0.1", int(port.read_text())), timeout=60)
        self.replies = self.conn.makefile("r")
        self.call("auth", token=(self.home / "control.token").read_text().strip())
        pause(1.5)

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(self.replies.readline())

    def quit(self):
        self.proc.terminate()
        self.proc.wait(20)

    def kill(self):
        os.kill(self.proc.pid, signal.SIGKILL)
        self.proc.wait()

    def rows(self):
        return self.call("queue.list").get("result") or []

    def row(self, part):
        return next((r for r in self.rows() if part in r.get("url", "")), None)

    def wait(self, part, states, seconds):
        end = time.time() + seconds
        while time.time() < end:
            r = self.row(part)
            if r and r["state"]["kind"] in states:
                return r
            time.sleep(0.25)
        return self.row(part)

    def dialogs(self):
        return self.call("ui.inspect").get("result", {}).get("dialogs") or []

    def texts(self):
        return [t["text"] for t in self.call("ui.texts").get("result", {}).get("texts", [])]

    def click(self, text):
        reply = self.call("ui.click", text=text)
        pause(0.6)
        return reply.get("ok")

    def picture(self, name):
        shot = self.call("ui.screenshot", name=name)
        if shot.get("ok"):
            target = WORK / "pictures" / f"{name}.png"
            target.parent.mkdir(exist_ok=True)
            shutil.copy(shot["result"]["path"], target)
            return target

    def open_add_url(self):
        self.click("Add URL")
        for _ in range(40):
            if "Add URL" in self.dialogs():
                break
            time.sleep(0.1)
        pause(0.6)

    def add(self, url, then="Start now"):
        """Add URL, type the address, OK, then Download details' `then`."""
        self.open_add_url()
        for _ in range(3):
            self.call("ui.type", text=url)
            pause(0.4)
            if any(url in t for t in self.texts()):
                break
        self.click("OK")
        for _ in range(40):
            if "Download details" in self.dialogs():
                pause(0.5)
                self.click(then)
                return True
            time.sleep(0.25)
        return False


def whole(row, name):
    """The row's file, compared with what the server holds."""
    path = pathlib.Path(row.get("path") or "") if row else None
    if not path or not path.is_file():
        return False, f"no file for {name}"
    same = hashlib.sha256(path.read_bytes()).hexdigest() == hashlib.sha256(DATA[name]).hexdigest()
    return same, f"{path.parent.name}/{path.name}, {'byte-exact' if same else 'DIFFERENT BYTES'}"


# ---------------------------------------------------------------- pictures

def png_pixels(path):
    """Width, height and RGB rows of an 8-bit PNG, by the standard library."""
    data = pathlib.Path(path).read_bytes()
    pos, idat, width = 8, b"", 0
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        chunk = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", chunk[:10])
        elif kind == b"IDAT":
            idat += chunk
        pos += 12 + length
    channels = {2: 3, 6: 4}[colour]
    raw, stride, rows, prev = zlib.decompress(idat), width * channels, [], bytearray(width * channels)
    for y in range(height):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b, c = prev[i], prev[i - channels] if i >= channels else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line)); prev = line
    return width, height, channels, rows


def differing(a, b):
    """The share of pixels that differ visibly between two pictures, or None
    where their sizes differ."""
    wa, ha, ca, ra = png_pixels(a)
    wb, hb, cb, rb = png_pixels(b)
    if (wa, ha) != (wb, hb):
        return None
    differ = 0
    for la, lb in zip(ra, rb):
        for x in range(0, wa, 2):   # every other pixel: fast enough, and no less telling
            pa, pb = la[x * ca:x * ca + 3], lb[x * cb:x * cb + 3]
            if max(abs(pa[i] - pb[i]) for i in range(3)) > 24:
                differ += 1
    return differ / (ha * ((wa + 1) // 2))


# ---------------------------------------------------------------- the walk

def walk(dm, origin):
    files = f"{origin}/files"

    print("\nAdding a download", flush=True)
    dm.picture("empty-window")
    dm.open_add_url()
    dm.picture("add-url")
    dm.click("OK")
    pause(0.8)
    shown = dm.texts()
    check("B01", "OK with nothing typed asks for an address and adds nothing",
          "Type or paste the address of what to download." in shown and not dm.rows(),
          f"{len(dm.rows())} rows")
    dm.click("Cancel")
    dm.open_add_url()
    dm.call("ui.type", text=f"{files}/big.bin")
    pause(0.4)
    dm.click("OK")
    for _ in range(40):
        if "Download details" in dm.dialogs():
            break
        time.sleep(0.25)
    pause(1.0)
    dm.picture("download-details")
    shown = dm.texts()
    wanted = ["Category", "Save to folder", "Save as", "Size", "Start now", "Add for later", "Cancel"]
    check("B02", "OK opens Download details with its fields and buttons",
          all(w in shown for w in wanted) and "big.bin" in shown, f"missing: {[w for w in wanted if w not in shown]}")
    dm.click("Start now")
    r = dm.wait("big.bin", {"running"}, 15)
    pause(3)
    r = dm.row("big.bin")
    check("B03", "Start now starts it and the row shows progress",
          r and r["state"]["kind"] == "running" and r["progress"]["downloaded"] > 0,
          f"{r['state']['kind'] if r else None}, {r['progress']['downloaded'] if r else 0} bytes")

    print("\nStopping and resuming", flush=True)
    dm.call("ui.select", ids=[r["id"]])
    pause(0.5)
    dm.click("Stop")
    r = dm.wait("big.bin", {"paused"}, 10)
    pause(1)
    status = next((x.get("status", "") for x in dm.call("ui.inspect")["result"].get("rows", []) if x.get("id") == r["id"]), "")
    check("B04", "Stop: 'Stopped · N%' and its bytes kept",
          r["state"]["kind"] == "paused" and r["progress"]["downloaded"] > 0 and status.startswith("Stopped") and "%" in status,
          f"{status}, {r['progress']['downloaded']} bytes")
    mark = len(LOG)
    dm.click("Resume")
    r = dm.wait("big.bin", {"completed", "failed"}, 180)
    starts = sorted(int(e["range"][6:].split("-")[0]) for e in requests("big.bin", mark) if e["range"] and e["range"] != "bytes=0-0")
    ok, d = whole(r, "big.bin")
    check("B05", "Resume goes on from what it had and the file is whole", starts and starts[0] > 0 and ok,
          f"first range after Resume at byte {starts[0] if starts else None}; {d}")
    halves = [e["range"] for e in requests("big.bin") if e["range"] and e["range"] != "bytes=0-0"]
    check("B06", "several connections, the file split while it runs", len(connections("big.bin")) >= 4,
          f"{len(connections('big.bin'))} connections, {len(halves)} ranges")

    print("\nHow a file is fetched", flush=True)
    for name in ("fast.bin", "slowconn.bin", "limited.bin", "norange.bin", "lie.bin"):
        dm.add(f"{files}/{name}")
    dm.add(f"{origin}/go/target")
    dm.add(f"{files}/report")
    dm.add(f"{files}/missing.bin")
    pause(2)
    done = {}
    for name in ("fast.bin", "slowconn.bin", "limited.bin", "norange.bin", "lie.bin"):
        done[name] = dm.wait(name, {"completed", "failed"}, 180)
    fast = requests("fast.bin")
    check("B07", "finished connections take the next piece", len(fast) > len(connections("fast.bin")),
          f"{len(fast)} requests on {len(connections('fast.bin'))} connections")
    slow = [e["t"] for e in LOG if e.get("name") == "slowconn.bin" and e["ev"] in ("req", "done")]
    took = max(slow) - min(slow) if slow else 99
    check("B08", "a slow server's connections are all used", done["slowconn.bin"]["state"]["kind"] == "completed" and took < 8,
          f"{took:.1f} s on {len(connections('slowconn.bin'))} connections")
    refused = sum(1 for e in LOG if e.get("name") == "limited.bin" and e["ev"] == "refused")
    ok, d = whole(done["limited.bin"], "limited.bin")
    check("B09", "a server allowing two at once: refusals absorbed, file complete", ok,
          f"{refused} connections refused with 503; {d}")
    ranged = [e for e in requests("norange.bin") if e["range"] not in (None, "bytes=0-0")]
    ok, d = whole(done["norange.bin"], "norange.bin")
    check("B10", "no ranges: one connection, whole", ok and not ranged, d)
    ok, d = whole(done["lie.bin"], "lie.bin")
    check("B11", "an overstated length still completes", done["lie.bin"]["state"]["kind"] == "completed" and ok, d)
    r = dm.wait("go/target", {"completed", "failed"}, 60)
    ok, d = whole(r, "target.bin")
    check("B12", "a redirect is followed and named by its target", ok and pathlib.Path(r["path"]).name == "target.bin", d)
    report = dm.wait("files/report", {"completed", "failed"}, 60)
    ok, d = whole(report, "report")
    check("B13", "the server's name is used", ok and pathlib.Path(report["path"]).name == "Report 2026.pdf", d)
    r = dm.wait("missing.bin", {"completed", "failed"}, 60)
    status = next((x.get("status", "") for x in dm.call("ui.inspect")["result"].get("rows", []) if x.get("id") == (r or {}).get("id")), "")
    check("B14", "a 404 fails with words a person reads",
          r and r["state"]["kind"] == "failed" and "404" in r["state"].get("reason", "") and "no longer works" in status,
          f"{status} / {r['state'].get('reason', '') if r else ''}")
    finished = [x for x in dm.rows() if x["state"]["kind"] == "completed"]
    exact = []
    for x in finished:
        name = x["url"].rsplit("/", 1)[-1]
        name = "target.bin" if name == "target" else name
        exact.append(whole(x, name)[0])
    check("B15", "every completed file is byte-exact", exact and all(exact), f"{sum(exact)} of {len(exact)}")

    print("\nWhere files go", flush=True)
    big = dm.row("big.bin")
    check("B16", "a PDF lands in Documents, a .bin in Other",
          pathlib.Path(report["path"]).parent.name == "Documents" and pathlib.Path(big["path"]).parent.name == "Other",
          f"{pathlib.Path(report['path']).parent.name}/Report 2026.pdf, {pathlib.Path(big['path']).parent.name}/big.bin")
    dm.add(f"{files}/again.bin")
    first = dm.wait("again.bin", {"completed"}, 60)

    def add_again():
        dm.open_add_url()
        dm.call("ui.type", text=f"{files}/again.bin")
        pause(0.4)
        dm.click("OK")
        for _ in range(40):
            if "Already in the list" in dm.dialogs():
                break
            time.sleep(0.25)
        pause(0.8)
        return dm.dialogs(), dm.texts()

    dialogs, shown = add_again()
    dm.click("Cancel")
    pause(0.8)
    copies = [x for x in dm.rows() if x["url"].endswith("/again.bin")]
    check("B17", "the same address again: dm asks first, Cancel adds nothing",
          "Already in the list" in dialogs and "This address is already in the list (Complete)." in shown
          and all(b in shown for b in ("Download again", "Show it", "Cancel")) and len(copies) == 1 and not dm.dialogs(),
          f"dialogs {dialogs}; {len(copies)} row(s)")
    dialogs, shown = add_again()
    dm.click("Download again")
    for _ in range(40):
        if "Download details" in dm.dialogs():
            pause(0.5)
            dm.click("Start now")
            break
        time.sleep(0.25)
    end = time.time() + 60
    while time.time() < end:
        copies = [x for x in dm.rows() if x["url"].endswith("/again.bin")]
        if len(copies) == 2 and all(x["state"]["kind"] == "completed" for x in copies):
            break
        time.sleep(0.25)
    names = sorted(pathlib.Path(x.get("path") or "").name for x in copies)
    check("B17", "Download again saves a second copy beside the first as 'name (1)'",
          names == ["again (1).bin", "again.bin"] and not dm.dialogs(), f"{names}")

    print("\nThe queue and speed", flush=True)
    dm.call("downloads.add", urls=[f"{files}/q{i}.bin" for i in range(1, 6)])
    most, end = 0, time.time() + 120
    while time.time() < end:
        qs = [x for x in dm.rows() if re.search(r"/q\d\.bin$", x["url"])]
        most = max(most, sum(1 for x in qs if x["state"]["kind"] == "running"))
        if len(qs) == 5 and all(x["state"]["kind"] in ("completed", "failed") for x in qs):
            break
        time.sleep(0.2)
    qs = [x for x in dm.rows() if re.search(r"/q\d\.bin$", x["url"])]
    check("B18", "five at once: no more than three run, all complete",
          most <= 3 and all(x["state"]["kind"] == "completed" for x in qs), f"at most {most} running")
    dm.add(f"{files}/later.bin", then="Add for later")
    pause(2)
    waiting = dm.row("later.bin")
    still = waiting and waiting["state"]["kind"] not in ("running", "completed")
    dm.click("Start queue")
    r = dm.wait("later.bin", {"completed", "failed"}, 60)
    check("B19", "Add for later waits until Start queue, then completes",
          still and r["state"]["kind"] == "completed", f"before: {waiting['state']['kind'] if waiting else None}")
    # Timed as dm sees it, from added to complete: the server's own clock
    # stops early, as the connection's buffers hold most of a small file.
    said = dm.call("command.run", id="downloads.speed.low").get("result", {}).get("said", "")
    pause(1)
    began = time.time()
    dm.call("downloads.add", urls=[f"{files}/limit.bin"])
    r = dm.wait("limit.bin", {"completed", "failed"}, 60)
    rate = FILES["limit.bin"][0] / 1024 / (time.time() - began)
    dm.call("command.run", id="downloads.speed.unlimited")
    pause(1)
    began = time.time()
    dm.call("downloads.add", urls=[f"{files}/again.bin?unlimited"])
    free = dm.wait("again.bin?unlimited", {"completed", "failed"}, 60)
    free_took = time.time() - began
    check("B20", "Low speed keeps to about 256 KB/s, Unlimited lifts it",
          said == "Speed limiter on: 256 KB/s." and 180 <= rate <= 320 and r["state"]["kind"] == "completed"
          and free and free["state"]["kind"] == "completed" and free_took < 4,
          f"'{said}'; {rate:.0f} KB/s under it, {free_took:.1f} s for 4 MiB without")

    print("\nWhen things go wrong", flush=True)
    dm.add(f"{files}/crash.bin")
    dm.wait("crash.bin", {"running"}, 15)
    pause(6)
    had = dm.row("crash.bin")["progress"]["downloaded"]
    dm.kill()
    pause(1.5)
    mark = len(LOG)
    dm.start()
    r = dm.row("crash.bin")
    if r and r["state"]["kind"] != "running":
        dm.call("ui.select", ids=[r["id"]])
        dm.click("Resume")
    r = dm.wait("crash.bin", {"completed", "failed"}, 180)
    starts = sorted(int(e["range"][6:].split("-")[0]) for e in requests("crash.bin", mark) if e["range"] and e["range"] != "bytes=0-0")
    ok, d = whole(r, "crash.bin")
    check("B21", "killed mid-download, it goes on after reopening and is whole",
          0 < had < FILES["crash.bin"][0] and starts and starts[0] > 0 and ok, f"killed at {had} bytes; {d}")
    listed = {x["url"]: x["state"]["kind"] for x in dm.rows()}
    dm.quit()
    pause(1)
    dm.start()
    again = {x["url"]: x["state"]["kind"] for x in dm.rows()}
    check("B22", "quit and reopened, every row is as it was", listed == again, f"{len(again)} rows")

    print("\nTidying the list", flush=True)
    r = dm.row("lie.bin")
    kept = pathlib.Path(r["path"])
    dm.call("ui.select", ids=[r["id"]])
    pause(0.5)
    dm.click("Remove")
    pause(0.8)
    said = dm.call("ui.inspect")["result"].get("said", "")
    gone = dm.row("lie.bin") is None
    dm.click("Undo")
    pause(0.8)
    back = dm.row("lie.bin") is not None
    check("B23", "a finished download leaves the list at once, its file stays, Undo brings it back",
          not dm.dialogs() and gone and said == "Removed 1 download from the list." and kept.is_file() and back,
          f"'{said}'; gone: {gone}; file kept: {kept.is_file()}; back after Undo: {back}")
    dm.add(f"{files}/stopme.bin")
    r = dm.wait("stopme.bin", {"running"}, 15)
    pause(3)
    dm.call("ui.select", ids=[r["id"]])
    dm.click("Stop")
    dm.wait("stopme.bin", {"paused"}, 10)
    dm.call("ui.select", ids=[r["id"]])
    dm.click("Remove")
    pause(0.8)
    asked = dm.texts()
    dm.click("Keep it")
    pause(0.6)
    still = dm.row("stopme.bin")
    dm.call("ui.select", ids=[r["id"]])
    dm.click("Remove")
    pause(0.8)
    dm.click("Yes, remove")
    pause(0.8)
    check("B38", "an unfinished download: Remove asks, Keep it keeps it, Yes, remove takes it off",
          # The sentence wraps into two runs of text in the window.
          "Remove an unfinished download" in asked and any(t.startswith("This download isn't finished.") for t in asked)
          and "Yes, remove" in asked and "Keep it" in asked and still and still["state"]["kind"] == "paused"
          and dm.row("stopme.bin") is None,
          f"asked: {'This download isn' + chr(39) + 't finished' in ' '.join(asked)}; kept after Keep it: {bool(still)}")
    paths = [pathlib.Path(x["path"]) for x in dm.rows() if x["state"]["kind"] == "completed"]
    dm.click("Clear finished")
    pause(1)
    said = dm.call("ui.inspect")["result"].get("said", "")
    left = [x for x in dm.rows() if x["state"]["kind"] == "completed"]
    check("B24", "Clear finished empties the finished rows, every file stays",
          not left and said.startswith("Deleted") and all(p.is_file() for p in paths), f"'{said}'")

    print("\nHow the window looks", flush=True)
    for name in ("empty-window", "add-url", "download-details"):
        now = WORK / "pictures" / f"{name}.png"
        if SAVE_BASELINE:
            BASELINE.mkdir(exist_ok=True)
            shutil.copy(now, BASELINE / f"{name}.png")
            check("B25", f"{name}: saved as the baseline", True)
            continue
        was = BASELINE / f"{name}.png"
        share = differing(was, now) if was.exists() and now.exists() else None
        check("B25", f"{name} looks as the baseline", share is not None and share <= 0.01,
              "no baseline or another screen size" if share is None else f"{share * 100:.2f}% of pixels differ")


BROWSER_ROWS = {
    "B26": ["a clicked download is fetched by dm into its own folder", "the browser's copy is cancelled and erased"],
    "B27": ["a plain click on a link to a PDF the browser would show in the tab goes to dm"],
    "B28": ["an Alt-clicked link is left with the browser"],
    "B29": ["'Download link with dm' hands the link to dm"],
    "B30": ["'Download all links with dm' hands every link on the page to dm's queue"],
    "B31": ["with 'Catch downloads' off, a download dm would catch stays in the browser"],
    "B32": ["with no host, a download it would catch finishes in the browser"],
    "B33": ["YouTube: hovering the playing player shows 'Download video'", "YouTube: it sits in the player's top corner",
            "YouTube: it stays while the pointer rests on the playing video"],
    "B34": ["YouTube: its click lists the video's qualities with their sizes"],
    "B35": ["YouTube: its click is taken by the app", "YouTube: the video lands in dm's folder, fetched by yt-dlp",
            "YouTube: the downloaded video is whole (media_check)"],
}


def edge(script, *args):
    """A check script of this repository in Microsoft Edge: its PASS and FAIL
    lines by name, and whether it ended well."""
    run = subprocess.run([sys.executable, "-u", str(ROOT / "scripts" / script), *args],
                         env=dict(os.environ, CHROME_PATH=EDGE, DM_BINARY=str(BINARY)),
                         capture_output=True, text=True)
    (WORK / f"edge-{script}.txt").write_text(run.stdout + run.stderr)
    said = {}
    for line in run.stdout.splitlines():
        m = re.match(r"\s+(PASS|FAIL)\s+(.*?)(?: — (.*))?$", line)
        if m:
            said[m.group(2).strip()] = (m.group(1) == "PASS", m.group(3) or "")
    return said, run.returncode


def browser():
    print("\nThe browser and a video, in Microsoft Edge", flush=True)
    if not EDGE or not pathlib.Path(EDGE).exists():
        print(f"  Microsoft Edge is not at {EDGE!r} (DM_QA_EDGE names it)", flush=True)
        return
    said, code = edge("youtube-check.py", VIDEO)
    for row, names in BROWSER_ROWS.items():
        for name in names:
            ok, detail = said.get(name, (False, "not run"))
            check(row, name, ok, detail[:100])
    failed = [n for n, (ok, _) in said.items() if not ok]
    check("B36", "every browser check passes", code == 0 and said and not failed,
          f"{len(said) - len(failed)} of {len(said)}" + (f"; failed: {failed[:3]}" if failed else ""))

    print("\nEvery kind of file, with dm set as installed, in Microsoft Edge", flush=True)
    said, code = edge("every-type-check.py")
    kinds = {n: v for n, v in said.items() if n.startswith("a click on ")}
    for name, (ok, detail) in kinds.items():
        check("B37", name, ok, detail[:100])
    check("B37", "every kind was tried", len(kinds) >= 14, f"{len(kinds)} kinds")
    failed = [n for n, (ok, _) in said.items() if not ok]
    check("B36", "every browser check passes, run again with dm's own settings", code == 0 and said and not failed,
          f"{len(said) - len(failed)} of {len(said)}" + (f"; failed: {failed[:3]}" if failed else ""))


def clipboard_held():
    """Empties the clipboard, and returns what it held, on macOS; None elsewhere."""
    if sys.platform != "darwin" or not shutil.which("pbpaste"):
        return None
    try:
        held = subprocess.run(["pbpaste"], capture_output=True, timeout=5).stdout
        subprocess.run(["pbcopy"], input=b"", timeout=5)
        return held
    except (OSError, subprocess.SubprocessError):
        return None


def clipboard_restore(held):
    if held:
        try:
            subprocess.run(["pbcopy"], input=held, timeout=5)
        except (OSError, subprocess.SubprocessError):
            pass


def main():
    if not BINARY.exists():
        sys.exit(f"no dm at {BINARY}: cd desktop && cargo build -p dm-desktop")
    rows = re.findall(r"^\| (B\d\d) \|", CONTRACT.read_text(), re.M)
    shutil.rmtree(WORK, ignore_errors=True)
    home = WORK / "home"
    (home / "downloads").mkdir(parents=True)
    (home / "settings.json").write_text(json.dumps({"download_dir": str(home / "downloads"), "theme": "dark"}))
    origin = start_server()
    print(f"dm: {BINARY}\nthe contract: {len(rows)} rows\nserver: {origin}", flush=True)
    # An address on the clipboard is offered in Add URL (as it should be), so
    # a run that follows a copied link saw "OK with nothing typed" with an
    # address typed. The clipboard is emptied for the walk and put back after.
    held = clipboard_held()
    dm = Dm(home)
    dm.start()
    try:
        walk(dm, origin)
    except Exception as e:   # a step that broke is a failure of its row, and the rest still run
        check("WALK", "the walk through the window ran to its end", False, repr(e))
    finally:
        try:
            dm.quit()
        except Exception:
            dm.proc.kill()
        clipboard_restore(held)
    browser()

    print("\nThe contract, row by row", flush=True)
    failed = []
    for row in rows:
        got = RESULTS.get(row)
        if not got:
            failed.append(row)
            print(f"  NOT CHECKED  {row}", flush=True)
        elif not all(ok for ok, _, _ in got):
            failed.append(row)
            print(f"  FAIL         {row}  " + "; ".join(w for ok, w, _ in got if not ok), flush=True)
    if "WALK" in RESULTS:
        failed.append("WALK")
    (WORK / "result.json").write_text(json.dumps({r: RESULTS.get(r, []) for r in rows + ["WALK"]}, indent=1))
    print(f"\n{len(rows) - len([f for f in failed if f != 'WALK'])} of {len(rows)} rows hold"
          f"{'' if not failed else '; failing or not checked: ' + ', '.join(failed)}", flush=True)
    print(f"pictures and the full record: {WORK}", flush=True)
    sys.exit(1 if failed else 0)


main()
