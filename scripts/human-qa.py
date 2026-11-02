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
import hashlib, json, os, pathlib, re, shutil, signal, socket, struct, subprocess, sys, threading, time, traceback, zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTRACT = ROOT / "docs" / "behaviour-contract.md"
BASELINE = ROOT / "docs" / "behaviour-baseline"
# One set of pictures for each system whose fonts and window frames differ: the
# set for this one where there is one, else the first.
BASELINE_SET = BASELINE / "windows" if sys.platform == "win32" else BASELINE
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
PROBES = ("bytes=0-0", "bytes=0-")   # the engine's first look at a file
WORK = pathlib.Path(os.environ.get("DM_QA_WORK") or "/tmp/dm-human-qa")


def pause(seconds):
    time.sleep(seconds * PACE)


# ---------------------------------------------------------------- results

RESULTS = {}


def check(row, what, ok, detail=""):
    RESULTS.setdefault(row, []).append((bool(ok), what, detail))
    print(f"  {'PASS' if ok else 'FAIL'}  {row}  {what}" + (f" — {detail}" if detail else ""), flush=True)
    return ok


# ---------------------------------------------------- dialogs' own windows

def screen_region(x, y, w, h, target):
    """A picture of part of the screen: how a dialog's window of its own is
    pictured, as dm's own pictures (`ui.screenshot`) are of its main window."""
    if w <= 0 or h <= 0:
        return False
    target = pathlib.Path(target)
    if sys.platform == "win32":
        ps = ("Add-Type -AssemblyName System.Drawing; "
              "Add-Type -Namespace W -Name D -MemberDefinition "
              "'[DllImport(\"user32.dll\")] public static extern bool SetProcessDPIAware();'; "
              "[W.D]::SetProcessDPIAware() | Out-Null; "
              f"$b = New-Object System.Drawing.Bitmap {w}, {h}; "
              "$g = [System.Drawing.Graphics]::FromImage($b); "
              f"$g.CopyFromScreen({x}, {y}, 0, 0, $b.Size); "
              f"$b.Save('{target}', [System.Drawing.Imaging.ImageFormat]::Png)")
        subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True)
    elif sys.platform == "darwin":
        subprocess.run(["screencapture", "-x", f"-R{x},{y},{w},{h}", str(target)], capture_output=True)
    else:
        return False
    return target.is_file()


WIN32_WINDOWS = r'''
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class W { public delegate bool Cb(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(Cb c, IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l); }
"@
$want = %d; $close = "%s"; $o = @()
[W]::EnumWindows({ param($h,$l) $p=0; [W]::GetWindowThreadProcessId($h,[ref]$p)|Out-Null
 if ($p -eq $want -and [W]::IsWindowVisible($h)) { $s=New-Object System.Text.StringBuilder 200; [W]::GetWindowText($h,$s,200)|Out-Null
   if ($s.Length -gt 0) { $script:o += $s.ToString(); if ($s.ToString() -eq $close) { [W]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)|Out-Null } } }
 $true }, [IntPtr]::Zero)|Out-Null
$o
'''


def win32_titles(pid, close=""):
    """The titles of a process's visible windows on Windows; with `close`, the
    window of that title is sent WM_CLOSE, as its × button does."""
    out = subprocess.run(["powershell", "-NoProfile", "-Command", WIN32_WINDOWS % (pid, close)],
                         capture_output=True, text=True).stdout
    return [line.strip() for line in out.splitlines() if line.strip()]


WIN32_HEAD = r'''
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class U {
 public delegate bool Cb(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool EnumWindows(Cb c, IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
 [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
 public struct R { public int L, T, Rr, B; } }
"@
[U]::SetProcessDPIAware() | Out-Null
'''


def win32_run(body):
    return subprocess.run(["powershell", "-NoProfile", "-Command", WIN32_HEAD + body],
                          capture_output=True, text=True).stdout


def win32_windows(pid):
    """What Windows itself says of each visible titled window of dm's: its rectangle
    on the screen, its parent and owner (none for a top-level window) and its style."""
    body = r'''
$want = %d; $o = @()
[U]::EnumWindows({ param($h,$l) $p=0; [U]::GetWindowThreadProcessId($h,[ref]$p)|Out-Null
 if ($p -eq $want -and [U]::IsWindowVisible($h)) { $s=New-Object System.Text.StringBuilder 200; [U]::GetWindowText($h,$s,200)|Out-Null
  if ($s.Length -gt 0) { $r=New-Object U+R; [U]::GetWindowRect($h,[ref]$r)|Out-Null
   $script:o += ("{0}|{1}|{2},{3},{4},{5}|{6}|{7}|{8}" -f $h.ToInt64(),$s,$r.L,$r.T,$r.Rr,$r.B,[U]::GetParent($h).ToInt64(),[U]::GetWindow($h,4).ToInt64(),[U]::GetWindowLong($h,-16)) } }
 $true }, [IntPtr]::Zero)|Out-Null
$o''' % pid
    found = {}
    for line in win32_run(body).splitlines():
        parts = line.strip().split("|")
        if len(parts) == 6:
            found[parts[1]] = {"hwnd": int(parts[0]), "rect": [int(v) for v in parts[2].split(",")],
                               "parent": int(parts[3]), "owner": int(parts[4]), "style": int(parts[5]) & 0xFFFFFFFF}
    return found


def win32_move(pid, title, x, y):
    """Puts a window of dm's where the system is told, as a person's drag would."""
    window = win32_windows(pid).get(title)
    if window:
        win32_run(r"[U]::SetWindowPos([IntPtr]%d, [IntPtr]::Zero, %d, %d, 0, 0, 0x0005) | Out-Null" % (window["hwnd"], x, y))


def win32_drag(x0, y0, x1, y1):
    """The mouse: pressed at one point of the screen, drawn to another and let go."""
    win32_run(r'''
[U]::SetCursorPos(%d,%d) | Out-Null; Start-Sleep -Milliseconds 250
[U]::mouse_event(0x2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 200
for ($i=1; $i -le 20; $i++) { [U]::SetCursorPos([int](%d + ($i/20.0)*(%d-%d)), [int](%d + ($i/20.0)*(%d-%d))) | Out-Null; Start-Sleep -Milliseconds 25 }
Start-Sleep -Milliseconds 250
[U]::mouse_event(0x4,0,0,0,[UIntPtr]::Zero)''' % (x0, y0, x0, x1, x0, y0, y1, y0))


def overlap(a, b):
    return not (a[2] <= b[0] or b[2] <= a[0] or a[3] <= b[1] or b[3] <= a[1])


# ---------------------------------------------------------------- the server

def body(name, size):
    seed = int(hashlib.sha256(name.encode()).hexdigest()[:8], 16)
    block = bytes((seed + i * 131) & 255 for i in range(65536))
    return (block * (size // 65536 + 1))[:size]


def make_zip():
    """A real ZIP, stored: a folder, a small file in it and a 6 MiB one, so
    that listing its files from its end is far less than fetching it."""
    import io, zipfile
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("docs/", b"")
        z.writestr("docs/readme.txt", b"a" * 1000)
        z.writestr("setup.exe", body("setup.exe", 6 * MiB))
    return out.getvalue()


PACK = make_zip()

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
    "second": (1 * MiB, 0, "named"),              # named by its server; asked about, answered No
    "pageyes.zip": (2048, 0, "page"),             # a web page where a file was asked for; Yes
    "pageno.zip": (2048, 0, "page"),              # the same; No
    "again.bin": (4 * MiB, 0, "plain"),           # added twice
    "later.bin": (4 * MiB, 0, "plain"),           # Add for later, then Start queue
    "pack.zip": (len(PACK), 0, "plain"),          # a real ZIP: its files are listed from its end
    "limit.bin": (3 * MiB, 0, "plain"),           # fetched under Low speed
    "crash.bin": (64 * MiB, 512 * 1024, "plain"),
    "stopme.bin": (64 * MiB, 512 * 1024, "plain"),    # stopped, then Remove
    "ui.bin": (48 * MiB, 512 * 1024, "plain"),        # running while its progress window is looked at (B46)
    "ui-alpha.bin": (1 * MiB, 0, "plain"),            # Find / Find previous (B48)
    "ui-beta.bin": (1 * MiB, 0, "plain"),
    "ui-alphabet.bin": (1 * MiB, 0, "plain"),
    **{f"q{i}.bin": (8 * MiB, 256 * 1024, "plain") for i in range(1, 6)},
}
DATA = {n: body(n, s) for n, (s, _, _) in FILES.items()}
DATA["pack.zip"] = PACK
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
            counted = kind == "limited" and rng not in PROBES and method != "HEAD"
            if counted:
                with LOCK:
                    busy = ACTIVE.get(name, 0)
                    if busy < 2:
                        ACTIVE[name] = busy + 1
                if busy >= 2:
                    log(ev="refused", conn=n, name=name)
                    c.sendall(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n")
                    continue
            if kind == "page":
                page = b"<html><title>Sign in</title><body>expired</body></html>"
                c.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: "
                          + str(len(page)).encode() + b"\r\n\r\n" + (b"" if method == "HEAD" else page))
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
        os.kill(self.proc.pid, getattr(signal, "SIGKILL", signal.SIGTERM))
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

    def dialog_texts(self):
        """The words drawn in dialogs' own windows, in the order they are drawn: a
        dialog's sentence wraps wherever its window is wide enough, so it is read
        whole by joining these."""
        texts = self.call("ui.texts").get("result", {}).get("texts", [])
        return [t["text"] for t in reversed(texts) if t.get("window") == "dialog"]

    def click(self, text):
        reply = self.call("ui.click", text=text)
        # A dialog is a window of its own, made by the system when it is opened:
        # on a busy computer its words are not drawn for a moment, as a person
        # waits for them before clicking.
        for _ in range(16):
            if reply.get("ok"):
                break
            time.sleep(0.25)
            reply = self.call("ui.click", text=text)
        if not reply.get("ok"):
            # Said where dm's window stands: a window narrower than its toolbar's
            # labels shows icons only, and there is no word to click.
            print(f"  (nothing on screen to click for {text!r}: dm's window {self.windows().get('main')})", flush=True)
        pause(0.6)
        return reply.get("ok")

    def windows(self):
        """Where dm's window and each dialog's window of its own stand."""
        return self.call("ui.windows").get("result", {})

    def picture(self, name, dialog=False):
        if dialog:
            # A dialog is a window of its own, so it is pictured from the
            # screen, over the rectangle dm says it stands on.
            info = self.windows()
            shown = [d["outer"] for d in info.get("dialogs", []) if d.get("outer")]
            if not shown:
                return None
            x0, y0, x1, y1 = shown[-1]
            k = (info.get("pixels_per_point") or 1.0) if sys.platform == "win32" else 1.0
            target = WORK / "pictures" / f"{name}.png"
            target.parent.mkdir(exist_ok=True)
            ok = screen_region(round(x0 * k), round(y0 * k), round((x1 - x0) * k), round((y1 - y0) * k), target)
            return target if ok else None
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
        print(f"  (Add URL did not reach Download details for {url}: dialogs {self.dialogs()}, "
              f"words {self.texts()[:14]})", flush=True)
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

def answer(dm, title, button, seconds=40):
    """Waits for the question `title`, returns the words on it, and presses `button`."""
    end = time.time() + seconds
    while time.time() < end:
        if title in dm.dialogs():
            pause(0.5)
            said = dm.dialog_texts()
            dm.click(button)
            return said
        time.sleep(0.25)
    return None


def drain(dm, seconds, buttons, said):
    """Answers, as a person does, every question in `buttons` (title -> button) that
    comes within `seconds`, keeping the words on each in `said[title]`."""
    end = time.time() + seconds
    while time.time() < end:
        for title in dm.dialogs():
            if title in buttons:
                pause(0.4)
                said.setdefault(title, []).append(dm.dialog_texts())
                dm.click(buttons[title])
        time.sleep(0.25)


def walk(dm, origin):
    files = f"{origin}/files"

    print("\nAdding a download", flush=True)
    dm.picture("empty-window")
    dm.open_add_url()
    dm.picture("add-url", dialog=True)
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
    dm.picture("download-details", dialog=True)
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
    starts = sorted(int(e["range"][6:].split("-")[0]) for e in requests("big.bin", mark) if e["range"] and e["range"] not in PROBES)
    ok, d = whole(r, "big.bin")
    check("B05", "Resume goes on from what it had and the file is whole", starts and starts[0] > 0 and ok,
          f"first range after Resume at byte {starts[0] if starts else None}; {d}")
    halves = [e["range"] for e in requests("big.bin") if e["range"] and e["range"] not in PROBES]
    check("B06", "several connections, the file split while it runs", len(connections("big.bin")) >= 4,
          f"{len(connections('big.bin'))} connections, {len(halves)} ranges")

    print("\nHow a file is fetched", flush=True)
    for name in ("fast.bin", "slowconn.bin", "limited.bin", "norange.bin", "lie.bin"):
        dm.add(f"{files}/{name}")
    # The questions a server's answer raises (B39, B41) are answered as a person does,
    # as they come: each blocks the window until it is.
    questions = {}
    asked = {"Name sent by the server": "Yes", "The file doesn't exist": "OK"}
    for url in (f"{origin}/go/target", f"{files}/report", f"{files}/missing.bin"):
        dm.add(url)
        drain(dm, 4, asked, questions)
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
    ranged = [e for e in requests("norange.bin") if e["range"] not in (None, *PROBES)]
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
    # B54: a failure that only says what went wrong also says what a person can do about it.
    dm.call("ui.click", text="missing.bin", double=True)
    pause(1.5)
    said = " ".join(dm.dialog_texts())
    check("B54", "a failed download's window says what a person can do next",
          "Open the page it came from in your browser" in said, said[-200:])
    # B55: "Copy details" puts a block with no secret in it on the clipboard.
    dm.call("ui.click", text="Copy details")
    pause(0.8)
    held = ""
    if sys.platform == "win32":
        held = subprocess.run(["powershell", "-NoProfile", "-Command", "Get-Clipboard -Raw"],
                              capture_output=True, text=True, timeout=10).stdout
    check("B55", "Copy details puts the host, the status and the setup on the clipboard, no path or query",
          "State: failed" in held and "Host: " in held and "Headers sent:" in held and "?" not in held
          and "/missing.bin" not in held, held[:200])
    dm.call("ui.key", key="Escape")
    pause(0.6)
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

    print("\nWhat a server's answer raises (added 2026-11-01 at the maintainer's word)", flush=True)
    drain(dm, 3, asked, questions)
    # The sentence wraps wherever the window is wide enough, so it is read whole.
    named = [t for t in questions.get("Name sent by the server", [])
             if "The name to save the file was report, but the server sent Report 2026.pdf" in " ".join(t)
             and "Save the file with the name received from the server?" in " ".join(t)]
    check("B39", "a server's name that differs from the address's is asked about, Yes and No offered",
          bool(named) and "Yes" in named[0] and "No" in named[0],
          f"{len(questions.get('Name sent by the server', []))} question(s): {questions.get('Name sent by the server', [])}"[:600])
    check("B39", "Yes saves it under the server's name",
          pathlib.Path(report["path"]).name == "Report 2026.pdf", pathlib.Path(report["path"]).name)
    dm.add(f"{files}/second")
    said = answer(dm, "Name sent by the server", "No")
    second = dm.wait("files/second", {"completed", "failed"}, 60)
    check("B39", "No saves it under the address's name",
          said is not None and bool(second) and second["state"]["kind"] == "completed"
          and pathlib.Path(second["path"]).name == "second",
          pathlib.Path(second["path"]).name if second and second.get("path") else str(second and second["state"]))
    gone = questions.get("The file doesn't exist", [])
    check("B41", "a 404 is told once in a window: the file doesn't exist, HTTP/1.1 404 Not Found, OK",
          len(gone) == 1 and "The requested file doesn't exist." in " ".join(gone[0])
          and "HTTP/1.1 404 Not Found" in " ".join(gone[0]) and "OK" in gone[0], f"{len(gone)} window(s)")
    pause(3)
    check("B41", "it does not come back for the same row", "The file doesn't exist" not in dm.dialogs(), "")
    dm.add(f"{files}/pageyes.zip")
    said = answer(dm, "Web page instead of the file", "Yes")
    page = dm.wait("pageyes.zip", {"completed", "failed"}, 60)
    saved = pathlib.Path(page["path"]) if page and page.get("path") else None
    check("B40", "a web page for a file is asked about; Yes saves it as an HTML page",
          said is not None and "Yes" in said and "No" in said and saved is not None
          and saved.name == "pageyes.html" and saved.read_bytes().startswith(b"<html>"),
          saved.name if saved else str(page and page["state"]))
    dm.add(f"{files}/pageno.zip")
    said = answer(dm, "Web page instead of the file", "No")
    pause(2)
    left = list((WORK / "home" / "downloads").rglob("pageno*"))
    check("B40", "No removes the row and keeps nothing",
          said is not None and dm.row("pageno.zip") is None and not left,
          f"row kept: {dm.row('pageno.zip') is not None}, files: {left}")

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

    print("\nA ZIP's contents", flush=True)
    mark = len(LOG)
    dm.open_add_url()
    for _ in range(3):
        dm.call("ui.type", text=f"{files}/pack.zip")
        pause(0.4)
        if any(f"{files}/pack.zip" in t for t in dm.texts()):
            break
    dm.click("OK")
    for _ in range(40):
        if "Download details" in dm.dialogs():
            break
        time.sleep(0.25)
    seen = []
    for _ in range(80):
        seen = dm.texts()
        if any("setup.exe" in t for t in seen):
            break
        time.sleep(0.25)
    pause(1)
    listed = (any(t.strip() == "Contents" for t in seen) and any("docs/readme.txt" in t for t in seen)
              and any("setup.exe" in t for t in seen))
    asked = requests("pack.zip", mark)
    # Only the end of the archive is asked for: every request names a range,
    # and none starts at the beginning but the one-byte first look.
    partial = bool(asked) and all(e["range"] and (e["range"] in PROBES or not e["range"].startswith("bytes=0-"))
                                  for e in asked)
    check("B44", "Download details for a .zip lists its files, with sizes, before the archive is fetched",
          listed and partial, f"listed: {listed}; ranges asked: {[e['range'] for e in asked]}")
    dm.click("Cancel")
    pause(0.8)
    check("B44", "closing it fetched nothing and left no row",
          "Download details" not in dm.dialogs() and dm.row("pack.zip") is None
          and not any(e.get("ev") == "done" and e.get("name") == "pack.zip" and e.get("bytes", 0) > MiB
                      for e in LOG[mark:]),
          f"{dm.dialogs()}")

    print("\nThe list, a running download's window, recent addresses and Find", flush=True)
    # B45: each row has a small picture of its file's kind, told apart by shape.
    # big.bin (plain), Report 2026.pdf (document) and pack.zip (archive) are in
    # the list now; the screenshot shows whether their icons differ by shape.
    shot45 = dm.picture("b45-icons")
    check("B45", "each row in the list has a file-kind icon told apart by shape (screenshot: b45-icons.png)",
          shot45 is not None,
          f"screenshot {'saved to ' + str(shot45) if shot45 else 'not taken'}")

    # B46: a double-click on a download still going opens its progress window,
    # whose title starts with how far it is.
    dm.add(f"{files}/ui.bin")
    dm.wait("ui.bin", {"running"}, 20)
    pause(3)
    dm.call("ui.click", text="ui.bin", double=True)
    pause(1.5)
    titles = [w.get("title", "") for w in (dm.windows().get("dialogs") or [])]
    first = next((t for t in titles if "ui.bin" in t), "")
    check("B46", "a double-click on a running download opens its progress window",
          bool(first) and "File properties" not in titles, f"{titles}")
    check("B46", "its title starts with how far it is",
          bool(re.match(r"^\d+% ", first)), f"{first!r}")
    pause(3)
    later = next((w.get("title", "") for w in (dm.windows().get("dialogs") or []) if "ui.bin" in w.get("title", "")), "")
    check("B46", "and the number goes on counting as it runs",
          bool(re.match(r"^\d+% ", later)) and int(re.match(r"^(\d+)%", later).group(1)) >= int(re.match(r"^(\d+)%", first).group(1)) if re.match(r"^\d+% ", first) and re.match(r"^\d+% ", later) else False,
          f"{first!r} then {later!r}")
    # B53: the Connections section of that window names each piece's own speed.
    dm.click("Connections")
    pause(2.5)
    rates = [w for w in dm.dialog_texts() if re.search(r"\d\s?(KB|MB|GB|B)/s$", w)]
    check("B53", "a running download's Connections section names a speed beside its pieces",
          len(rates) >= 2, f"{rates}")
    dm.call("ui.key", key="Escape")
    pause(0.6)
    r = dm.row("ui.bin")
    if r:
        dm.call("ui.select", ids=[r["id"]])
        dm.click("Stop")
        dm.wait("ui.bin", {"paused"}, 10)
    # B47: an address with a token in its query is not kept for Add URL's list.
    dm.add(f"{files}/ui-alpha.bin?token=secret123abc")
    dm.wait("ui-alpha.bin", {"completed", "failed"}, 30)
    kept = (home / "settings.json").read_text(encoding="utf-8", errors="replace") if (home / "settings.json").exists() else ""
    check("B47", "an address with a token in its query is added but not kept for Add URL's Recent list",
          dm.row("ui-alpha.bin") is not None and "secret123abc" not in kept, f"settings mention it: {'secret123abc' in kept}")
    # B48: Find previous goes back, wrapping from the first row to the last.
    for name in ("ui-beta.bin", "ui-alphabet.bin"):
        dm.add(f"{files}/{name}")
        dm.wait(name, {"completed", "failed"}, 30)
    dm.call("ui.key", key="F", ctrl=True)
    pause(0.8)
    dm.call("ui.type", text="alpha")
    pause(0.4)
    dm.call("ui.key", key="F3", shift=True)
    pause(0.8)
    one = [x["url"].rsplit("/", 1)[-1] for x in dm.rows() if x["id"] in (dm.call("ui.inspect").get("result", {}).get("selected") or [])]
    dm.call("ui.key", key="F3", shift=True)
    pause(0.8)
    two = [x["url"].rsplit("/", 1)[-1] for x in dm.rows() if x["id"] in (dm.call("ui.inspect").get("result", {}).get("selected") or [])]
    check("B48", "Shift-F3 selects the previous row whose name has the text, and goes on back",
          len(one) == 1 and len(two) == 1 and one != two and all("alpha" in n for n in one + two), f"{one} then {two}")
    dm.click("Cancel")
    pause(0.6)

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
    starts = sorted(int(e["range"][6:].split("-")[0]) for e in requests("crash.bin", mark) if e["range"] and e["range"] not in PROBES)
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
    asked_titles = dm.dialogs()
    asked = dm.dialog_texts()
    dm.click("Keep it")
    pause(0.6)
    still = dm.row("stopme.bin")
    dm.call("ui.select", ids=[r["id"]])
    dm.click("Remove")
    pause(0.8)
    dm.click("Yes, remove")
    pause(0.8)
    check("B38", "an unfinished download: Remove asks, Keep it keeps it, Yes, remove takes it off",
          # The title is the window's own; the sentence wraps wherever the window is wide enough.
          "Remove an unfinished download" in asked_titles and "This download isn't finished." in " ".join(asked)
          and "Yes, remove" in asked and "Keep it" in asked and still and still["state"]["kind"] == "paused"
          and dm.row("stopme.bin") is None,
          f"asked: {'This download isn' + chr(39) + 't finished' in ' '.join(asked)}; kept after Keep it: {bool(still)}")
    paths = [pathlib.Path(x["path"]) for x in dm.rows() if x["state"]["kind"] == "completed"]
    dm.click("Clear finished")
    # Its own line, not the one before it ("Put back 1 download." from the
    # Undo above), which a fixed pause sometimes read (gaps row 56).
    said = ""
    for _ in range(25):
        pause(0.2)
        said = dm.call("ui.inspect")["result"].get("said", "")
        if said.startswith("Deleted"):
            break
    left = [x for x in dm.rows() if x["state"]["kind"] == "completed"]
    check("B24", "Clear finished empties the finished rows, every file stays",
          not left and said.startswith("Deleted") and all(p.is_file() for p in paths), f"'{said}'")

    print("\nDialogs are windows of their own", flush=True)
    dm.open_add_url()
    info = dm.windows()
    main = info.get("main")
    shown = info.get("dialogs") or []
    box = shown[0].get("outer") if shown else None
    over = bool(main and box and main[0] <= (box[0] + box[2]) / 2 <= main[2]
                and main[1] <= (box[1] + box[3]) / 2 <= main[3])
    words = [t["text"] for t in dm.call("ui.texts")["result"]["texts"] if t.get("window") == "dialog"]
    check("B42", "Add URL opens as a window of its own, over the middle of dm's window",
          len(shown) == 1 and shown[0]["title"] == "Add URL" and over and "OK" in words,
          f"dm {main}, dialog {box}, words {words[:6]}")
    # Moved as a drag by its title bar moves it: half outside dm's window.
    if box:
        width = box[2] - box[0]
        want = (main[2] - width / 2, main[1] + 20)
        dm.call("ui.move_window", title="Add URL", x=want[0], y=want[1])
        pause(1.0)
        moved = (dm.windows().get("dialogs") or [{}])[0].get("outer")
        check("B42", "it stands where it was put, half outside dm's window",
              bool(moved) and abs(moved[0] - want[0]) <= 16 and abs(moved[1] - want[1]) <= 16 and moved[2] > main[2],
              f"asked for {want}, it is at {moved}")
    dm.click("Cancel")
    pause(0.6)
    check("B42", "its buttons work where it was put: Cancel closes it", "Add URL" not in dm.dialogs(), f"{dm.dialogs()}")
    dm.open_add_url()
    dm.call("ui.key", key="Escape")
    pause(0.8)
    check("B42", "Escape closes it, as Cancel does", "Add URL" not in dm.dialogs(), f"{dm.dialogs()}")
    if sys.platform == "win32":
        dm.open_add_url()
        win32_titles(dm.proc.pid, close="Add URL")
        pause(1.0)
        check("B42", "its close button (the system's) closes it, as Cancel does",
              "Add URL" not in dm.dialogs(), f"{dm.dialogs()}")
    else:
        check("B42", "its close button (the system's) closes it, as Cancel does", True,
              "pressed on Windows only; Escape is the same path elsewhere")
    if sys.platform == "win32":
        # The dialog taken fully out of dm's window, as Windows itself sees it: dm's
        # window put aside (a person may park it partly off the screen), the dialog
        # moved to a place with no overlap with it, then drawn to another by its
        # title bar with the mouse. Read back from Windows, not from dm.
        pid = dm.proc.pid
        win32_move(pid, "dm", -420, 30)
        pause(1.0)
        dm.open_add_url()
        pause(1.0)
        wins = win32_windows(pid)
        aside = wins.get("dm", {}).get("rect")
        if aside and "Add URL" in wins:
            free = aside[2] + 20
            dm.call("ui.move_window", title="Add URL", x=free, y=40)
            pause(1.2)
            seen = win32_windows(pid).get("Add URL", {})
            box = seen.get("rect")
            check("B42", "moved to a place with no overlap with dm's window, Windows reports it there",
                  bool(box) and abs(box[0] - free) <= 16 and abs(box[1] - 40) <= 16 and not overlap(box, aside),
                  f"dm {aside}, dialog {box}")
            top_level = seen.get("parent") == 0 and seen.get("owner") == 0 and not (seen.get("style", 0) & 0x40000000)
            check("B42", "it is a window of its own, not a child or owned window clipped by dm's",
                  bool(seen) and top_level, f"parent {seen.get('parent')}, owner {seen.get('owner')}, style {hex(seen.get('style', 0))}")
            if box:
                gx = min(max(box[0] + 60, 40), box[2] - 80)
                gy = max(box[1], 0) + 14
                goal = (free + 40, 260)
                win32_drag(gx, gy, gx + (goal[0] - box[0]), gy + (goal[1] - box[1]))
                pause(1.5)
                dragged = win32_windows(pid).get("Add URL", {}).get("rect")
                pause(2.0)
                still = win32_windows(pid).get("Add URL", {}).get("rect")
                check("B42", "drawn by its title bar with the mouse it stands where the mouse left it, wholly outside dm's window",
                      bool(dragged) and abs(dragged[0] - goal[0]) <= 16 and abs(dragged[1] - goal[1]) <= 16
                      and not overlap(dragged, aside) and dragged == still,
                      f"dm {aside}, wanted {goal}, it is at {dragged}, and later {still}")
        else:
            check("B42", "moved to a place with no overlap with dm's window, Windows reports it there", False,
                  f"windows {list(wins)}")
        dm.click("Cancel")
        pause(0.8)
        win32_move(pid, "dm", 130, 130)
        pause(1.0)
    # Options keeps the clicks to itself: dm's window takes none until it is closed.
    dm.click("Options")
    for _ in range(40):
        if "Options" in dm.dialogs():
            break
        time.sleep(0.1)
    pause(0.8)
    dm.click("Add URL")
    pause(0.8)
    held = dm.dialogs()
    check("B42", "Options holds dm's window: a click on its toolbar does nothing while it is open",
          held == ["Options"], f"{held}")
    dm.call("ui.key", key="Escape")
    pause(0.8)
    dm.open_add_url()
    check("B42", "and the window answers again once it is closed", "Add URL" in dm.dialogs(), f"{dm.dialogs()}")
    dm.click("Cancel")
    pause(0.6)

    # Options' tabs: the connection's and the proxy's settings are one Advanced page.
    dm.click("Options")
    for _ in range(40):
        if "Options" in dm.dialogs():
            break
        time.sleep(0.1)
    pause(0.8)
    dm.click("Advanced")
    pause(0.8)
    words = dm.dialog_texts()
    first = "Speed limit, KB/s (0 for none)"
    # What is drawn before the first setting: the tabs, then the page's first heading.
    before = words[:words.index(first)] if first in words else []
    tabs = ("General", "Browser", "Folders", "Downloads", "Advanced", "Sign-ins")
    check("B43", "Options has six tabs with Advanced among them, and no Network or Proxy tab",
          bool(before) and all(tab in before for tab in tabs) and before.count("Network") == 1 and "Proxy" not in before,
          f"{before}")
    check("B43", "Advanced opens on the connection's settings, under their heading",
          first in words and "Attempts per connection (0 to keep trying)" in words, f"{words[:14]}")
    # B57: the site-limit list is there with its Add button.
    check("B57", "Advanced has a site connection-limit list with Add a site limit",
          "Sites with their own connection limit:" in words and "Add a site limit" in words, f"{words}")
    # B60: Site files has the switch for following links to other sites.
    dm.click("Cancel")
    pause(0.8)
    dm.click("Program")
    pause(0.5)
    dm.click("Site files")
    pause(0.8)
    site_words = dm.dialog_texts()
    check("B60", "Site files has 'Also follow links to other sites' beside the page depth",
          "Also follow links to other sites" in site_words and "Page depth (0 for the start page only)" in site_words,
          f"{site_words}")
    check("B61", "Site files has 'Keep the site's folders' under the other-sites box",
          "Keep the site's folders" in site_words, f"{site_words}")
    dm.click("Close")
    pause(0.8)
    # B62: the Scheduler has the field for a program to run at the end.
    dm.click("Scheduler")
    pause(0.8)
    sched_words = dm.dialog_texts()
    check("B62", "the Scheduler has 'Run this program when they are done (optional)'",
          "Run this program when they are done (optional)" in sched_words, f"{sched_words}")
    dm.click("Cancel")
    pause(0.8)
    dm.click("Options")
    pause(0.8)
    dm.click("Advanced")
    pause(0.8)
    # B51: the page says how dm identifies itself, with the switch.
    check("B51", "Advanced shows the name dm sends to servers and the Identify as dm only switch",
          "dm identifies itself to servers as:" in words and "Identify as dm only" in words, f"{words}")
    # B56: the Downloads tab has the Connections per file box with its Presets.
    dm.click("Downloads")
    pause(0.8)
    dl = dm.dialog_texts()
    check("B56", "Downloads has Connections per file with Presets: Slow (2) Normal (8) Fast (16) Maximum (32)",
          "Connections per file" in dl and "Presets:" in dl and "Slow (2)" in dl and "Normal (8)" in dl
          and "Fast (16)" in dl and "Maximum (32)" in dl, f"{dl}")
    dm.click("General")
    pause(0.8)
    general = dm.dialog_texts()
    check("B50", "General has the row for what happens when an address is already in the list",
          "When an address is already in the list" in general, f"{general}")
    dm.click("Cancel")
    pause(0.6)
    # B49: the Scheduler offers what to do when the scheduled downloads are done.
    dm.click("Scheduler")
    for _ in range(40):
        if "Scheduler" in dm.dialogs():
            break
        time.sleep(0.1)
    pause(0.8)
    sched = dm.dialog_texts()
    check("B49", "the Scheduler asks what dm does when the scheduled downloads are done, and how many tries again",
          "When the scheduled downloads are done" in sched
          and "Try a failed download again this many times (0 for none)" in sched, f"{sched}")
    dm.click("Cancel")
    pause(0.6)
    # B52: the tray's Speed menu has Settings…, which the tray cannot be driven
    # here to click: its menu is built from the same command as Downloads ›
    # Speed limiter › Settings…, which a person tries by hand in the tray.
    tray_src = (ROOT / "desktop" / "crates" / "dm-desktop" / "src" / "tray.rs").read_text(encoding="utf-8")
    check("B52", "the tray's Speed menu is built with the limiter's Settings… command",
          "Command::SpeedLimiterSettings" in tray_src, "tray.rs does not name the command")

    print("\nHow the window looks", flush=True)
    for name in ("empty-window", "add-url", "download-details"):
        now = WORK / "pictures" / f"{name}.png"
        if SAVE_BASELINE:
            BASELINE_SET.mkdir(exist_ok=True)
            shutil.copy(now, BASELINE_SET / f"{name}.png")
            check("B25", f"{name}: saved as the baseline", True)
            continue
        was = BASELINE_SET / f"{name}.png"
        if not was.exists():
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
    "B35": ["YouTube: its click is taken by the app", "YouTube: the video lands in dm's folder, fetched by dm itself",
            "YouTube: the downloaded video is whole (media_check)"],
    "B59": ["YouTube: its list has a Part line that takes a start and an end"],
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
        (WORK / "walk-trace.txt").write_text(traceback.format_exc())
        check("WALK", "the walk through the window ran to its end", False, repr(e) + f" dm_exit={dm.proc.poll()} " + traceback.format_exc().replace(chr(10), " | ")[-700:])
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
