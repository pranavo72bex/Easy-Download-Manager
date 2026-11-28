#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The twenty everyday tasks of docs/tasks.md, done in EDM's window and counted.

    python -X utf8 scripts/task-steps.py            # every driven task
    python -X utf8 scripts/task-steps.py T05 T09    # some of them

Each task marked "driven" in docs/tasks.md is done in EDM's real window the way a
person does it, through the control port (`ui.click` on the words, `ui.type`
into the field that has the keyboard, `ui.key`), against a server on 127.0.0.1.
Every click, key and typed field is counted, and every window that opens (from
`ui.inspect`'s dialogs); then the task's outcome is checked (the file whole, the
row gone, the setting kept). The tasks marked "by hand" need the browser or a
real site, or would run a file or turn the computer off: their counts are read
from docs/tasks.md. The table printed puts reference A's counts, as a person
filled them in docs/tasks.md, beside EDM's, and lists each task where EDM takes
more: each such task is a row to propose in docs/gaps.md.

Each task runs in an EDM of its own, started with `--control 0` and EDM_HOME in a
work folder, as a new install has it (only the download folder set), so a
download's progress window opens as it starts and is counted, as a person sees
it. What a task needs beforehand (a finished download, a failed one, links on
the clipboard) is set up first and not counted, and the windows that leaves are
closed before counting begins. The work folder is EDM_TASKS_WORK, or
edm-task-steps under the system's temporary folder; the record is
task-steps.json there.

Needs a debug build (`cargo build -p edm-gpui`); EDM_BINARY names
another. EDM_TASKS_PACE (1.0 at first) sets how long each step stays on screen.
Standard library only. On Windows, start it from PowerShell or Windows Terminal,
not from Git Bash (from there EDM's window is not shown as a person sees it),
with `python -X utf8`; it also sets UTF-8 for its own output and files. It never
imports scripts/human-qa.py (importing that runs it): what it needs of it is
copied here.
"""
import collections, hashlib, json, os, pathlib, re, shutil, signal, socket, subprocess, sys, tempfile, threading, time, traceback

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

ROOT = pathlib.Path(__file__).resolve().parent.parent
TASKS_DOC = ROOT / "docs" / "tasks.md"
SHIPPED = "edm-gpui"
BINARY = pathlib.Path(os.environ.get("EDM_BINARY") or ROOT / "target/debug"
                      / (SHIPPED + (".exe" if sys.platform == "win32" else "")))
PACE = float(os.environ.get("EDM_TASKS_PACE", "1.0"))
WORK = pathlib.Path(os.environ.get("EDM_TASKS_WORK") or pathlib.Path(tempfile.gettempdir()) / "edm-task-steps").resolve()
CMD = "cmd" if sys.platform == "darwin" else "ctrl"
MiB = 1024 * 1024


def pause(seconds):
    time.sleep(seconds * PACE)


# ---------------------------------------------------------------- the server

def body(name, size):
    seed = int(hashlib.sha256(name.encode()).hexdigest()[:8], 16)
    block = bytes((seed + i * 131) & 255 for i in range(65536))
    return (block * (size // 65536 + 1))[:size]


# name: (size, bytes a second per connection or 0)
FILES = {
    "copied.bin": (4 * MiB, 0),                    # T05
    "pause.bin": (64 * MiB, 256 * 1024),           # T06: about half a minute on eight connections
    "batch-a.bin": (1 * MiB, 0),                   # T07
    "batch-b.bin": (1 * MiB, 0),
    "batch-c.bin": (1 * MiB, 0),
    "later.bin": (2 * MiB, 0),                     # T08
    "found-alpha.bin": (1 * MiB, 0),               # T11
    "other-beta.bin": (1 * MiB, 0),
    "third-gamma.bin": (1 * MiB, 0),
    "fresh.bin": (2 * MiB, 0),                     # T13: the fresh link
    "flaky.bin": (2 * MiB, 0),                     # T14: not found until it is back
    "moved.bin": (1 * MiB, 0),                     # T15
    "kept.bin": (1 * MiB, 0),                      # T16
    "done-a.bin": (1 * MiB, 0),                    # T17
    "done-b.bin": (1 * MiB, 0),
    "photo1.bin": (1 * MiB, 0),                    # T18
    "photo2.bin": (1 * MiB, 0),
    "photo3.bin": (1 * MiB, 0),
}
DATA = {n: body(n, s) for n, (s, _) in FILES.items()}
# Answered 404 while named here (T14: a server that comes back).
HELD = set()
LOCK = threading.Lock()


def serve(c):
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
            with LOCK:
                held = name in HELD
            if name not in FILES or held:
                c.sendall(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                continue
            size, rate = FILES[name]
            rng = hdr.get("range")
            first, last = 0, size - 1
            if rng and rng.startswith("bytes="):
                a, b2 = rng[6:].split("-", 1)
                first = int(a or 0)
                last = min(int(b2), size - 1) if b2 else size - 1
            h = (f"HTTP/1.1 {'206 Partial Content' if rng else '200 OK'}\r\n"
                 f"Content-Type: application/octet-stream\r\nContent-Length: {last - first + 1}\r\n"
                 "Accept-Ranges: bytes\r\n")
            if rng:
                h += f"Content-Range: bytes {first}-{last}/{size}\r\n"
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
    except (OSError, ValueError):
        pass
    finally:
        try:
            c.close()
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
            threading.Thread(target=serve, args=(c,), daemon=True).start()

    threading.Thread(target=accept, daemon=True).start()
    return f"http://127.0.0.1:{srv.getsockname()[1]}"


def whole(row, name):
    """Whether the row's file holds exactly what the server has for `name`."""
    path = pathlib.Path(row.get("path") or "") if row else None
    if not path or not path.is_file():
        return False, f"no file for {name}"
    same = hashlib.sha256(path.read_bytes()).hexdigest() == hashlib.sha256(DATA[name]).hexdigest()
    return same, f"{path.parent.name}/{path.name}, {'byte-exact' if same else 'DIFFERENT BYTES'}"


def set_clipboard(text):
    """Puts `text` on the system's clipboard, as copying it elsewhere does."""
    put = {"darwin": ["pbcopy"],
           "win32": ["powershell", "-NoProfile", "-Command", "$input | Set-Clipboard"]}.get(
        sys.platform, ["xclip", "-selection", "clipboard"])
    try:
        # xclip stays on to serve the clipboard: kept off this script's output.
        return subprocess.run(put, input=text.encode("utf-8"), timeout=10,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


# ---------------------------------------------------------------- EDM, driven

class Dm:
    """EDM started on a home of its own and spoken to over its control port, as
    scripts/human-qa.py's Dm (copied, not imported: importing that runs it)."""

    def __init__(self, home):
        self.home, self.proc, self.conn, self.replies = home, None, None, None

    def start(self):
        (self.home / "control.port").unlink(missing_ok=True)
        self.proc = subprocess.Popen([str(BINARY), "--control", "0"], env=dict(os.environ, EDM_HOME=str(self.home)),
                                     stdout=subprocess.DEVNULL,
                                     stderr=open(self.home.parent / "edm-stderr.txt", "a", encoding="utf-8"))
        port = self.home / "control.port"
        for _ in range(200):
            if port.exists() and port.read_text(encoding="utf-8").strip():
                break
            time.sleep(0.1)
        self.conn = socket.create_connection(("127.0.0.1", int(port.read_text(encoding="utf-8"))), timeout=60)
        self.replies = self.conn.makefile("r", encoding="utf-8")
        self.call("auth", token=(self.home / "control.token").read_text(encoding="utf-8").strip())
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

    def row_id(self, id_):
        return next((r for r in self.rows() if r.get("id") == id_), None)

    def wait(self, part, states, seconds):
        end = time.time() + seconds
        while time.time() < end:
            r = self.row(part)
            if r and r["state"]["kind"] in states:
                return r
            time.sleep(0.25)
        return self.row(part)

    def inspect(self):
        return self.call("ui.inspect").get("result", {}) or {}

    def dialogs(self):
        return self.inspect().get("dialogs") or []

    def texts(self):
        return [t["text"] for t in self.call("ui.texts").get("result", {}).get("texts", [])]

    def dialog_texts(self):
        texts = self.call("ui.texts").get("result", {}).get("texts", [])
        return [t["text"] for t in reversed(texts) if t.get("window") == "dialog"]

    def settings(self):
        try:
            return json.loads((self.home / "settings.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return {}

    def press(self, text, **more):
        """ui.click on `text`, asked again for a moment while it is not drawn yet
        (a dialog's window is made by the system, its words a moment later)."""
        reply = self.call("ui.click", text=text, **more)
        for _ in range(16):
            if reply.get("ok"):
                break
            time.sleep(0.25)
            reply = self.call("ui.click", text=text, **more)
        return bool(reply.get("ok"))


def close_dialogs(edm):
    """Closes every window EDM has open besides its own, not counted: what setting a task
    up left open (a progress window, "This link no longer works"). Escape closes a dialog
    as its close button does; Close or OK where it does not. Never Cancel: in a progress
    window that would be Cancel download's neighbour."""
    for _ in range(12):
        open_now = edm.dialogs()
        if not open_now:
            return True
        if "This link no longer works" in open_now:
            edm.press("Close")
        else:
            edm.call("ui.key", key="Escape")
        pause(0.5)
        if collections.Counter(edm.dialogs()) == collections.Counter(open_now):
            if not edm.press("Close"):
                edm.press("OK")
            pause(0.5)
    return not edm.dialogs()


class Steps:
    """What a person does for a task, counted as it is done."""

    def __init__(self, edm):
        self.edm = edm
        self.clicks = self.keys = self.typed = self.windows = 0
        self.opened, self.done, self.notes = [], [], []
        self.open = collections.Counter(edm.dialogs())

    def look(self):
        """Counts each window that opened since the last look."""
        now = collections.Counter(self.edm.dialogs())
        for name, n in (now - self.open).items():
            self.windows += n
            self.opened.extend([name] * n)
            self.done.append(f"window opened: {name}" + (f" (x{n})" if n > 1 else ""))
        self.open = now

    def click(self, text, right=False, double=False):
        """Clicks the words `text`; counted only if they were there to click."""
        more = {}
        if right:
            more["button"] = "right"
        if double:
            more["double"] = True
        ok = self.edm.press(text, **more)
        if ok:
            self.clicks += 1
            self.done.append(("right-click " if right else "double-click " if double else "click ") + repr(text))
        else:
            self.notes.append(f"nothing to click for {text!r}")
            self.done.append(f"(not on screen: {text!r})")
        pause(0.6)
        self.look()
        return ok

    def row(self, name, right=False):
        """Clicks a download's row by its name; one click either way. Where its name is
        not where a click lands, the row is chosen as a click would (`ui.select`)."""
        if right:
            return self.click(name, right=True)
        r = self.edm.row(name)
        ok = self.edm.press(name)
        chosen = self.edm.inspect().get("selected") or []
        if not (ok and r and r["id"] in chosen) and r:
            self.edm.call("ui.select", ids=[r["id"]])
            self.notes.append(f"the row {name} chosen with ui.select, as a click on it")
        self.clicks += 1
        self.done.append(f"click the row {name!r}")
        pause(0.6)
        self.look()
        return bool(r)

    def key(self, key, modifiers=()):
        reply = self.edm.call("ui.key", key=key, modifiers=list(modifiers))
        self.keys += 1
        self.done.append("key " + "+".join([*modifiers, key]))
        if not reply.get("ok"):
            self.notes.append(f"key {key} refused: {reply.get('error')}")
        pause(0.6)
        self.look()

    def type(self, text):
        reply = self.edm.call("ui.type", text=text)
        self.typed += 1
        self.done.append(f"type {text!r}")
        if not reply.get("ok"):
            self.notes.append(f"typing refused: {reply.get('error')}")
        pause(0.6)
        self.look()

    def counted(self, clicks=0, keys=0, typed=0, windows=0, why=""):
        """Steps of the path counted but not done by the script, and why."""
        self.clicks += clicks
        self.keys += keys
        self.typed += typed
        self.windows += windows
        self.done.append(f"counted, not done: {why}")
        self.notes.append(f"counted, not done: {why}")

    def until(self, test, seconds):
        """Waits for `test()`, looking for windows as they open; its last answer."""
        end = time.time() + seconds
        got = test()
        while not got and time.time() < end:
            time.sleep(0.25)
            self.look()
            got = test()
        self.look()
        return got

    def dialog(self, title, seconds=10):
        return self.until(lambda: title in self.edm.dialogs(), seconds)

    def finished(self, names, seconds=90):
        """Waits for every download named to end; the rows."""
        def ended():
            rows = [self.edm.row(n) for n in names]
            return rows if all(r and r["state"]["kind"] in ("completed", "failed") for r in rows) else None
        rows = self.until(ended, seconds) or [self.edm.row(n) for n in names]
        # The progress window turns into Download complete, or one opens: seen as it does.
        pause(1.0)
        self.look()
        return rows


def shown_with(edm, text, within=2.0):
    """Whether `text` is drawn anywhere within `within` seconds."""
    end = time.time() + within
    while time.time() < end:
        if any(text in t for t in edm.texts()):
            return True
        time.sleep(0.3)
    return False


def all_whole(rows, names):
    checked = [whole(r, n) for r, n in zip(rows, names)]
    return all(ok for ok, _ in checked), "; ".join(d for _, d in checked)


# ---------------------------------------------------------------- the tasks
# Each: setup(EDM, files, work) not counted; run(EDM, s, files, work) -> (ok, detail).

def add_and_finish(edm, files, names, within=60):
    edm.call("downloads.add", urls=[f"{files}/{n}" for n in names])
    for n in names:
        edm.wait(n, {"completed", "failed"}, within)


def open_add_url(edm, s, url):
    """Add URL, with the copied link put in; typed where the clipboard did not fill it."""
    edm.call("ui.window", to="shown")
    pause(0.6)
    s.click("Add URL")
    s.dialog("Add URL", 5)
    if not shown_with(edm, url, 2.0):
        s.notes.append("Add URL did not put in the copied link: typed")
        s.type(url)
    s.click("OK")
    return s.dialog("Download details", 15)


def t05_setup(edm, files, work):
    if not set_clipboard(f"{files}/copied.bin"):
        print("  (could not put the link on the clipboard: it will be typed)", flush=True)


def t05_run(edm, s, files, work):
    if not open_add_url(edm, s, f"{files}/copied.bin"):
        return False, f"no Download details: dialogs {edm.dialogs()}"
    s.click("Start now")
    (r,) = s.finished(["copied.bin"])
    return whole(r, "copied.bin")


def t06_setup(edm, files, work):
    edm.call("downloads.add", urls=[f"{files}/pause.bin"])
    edm.wait("pause.bin", {"running"}, 15)
    pause(2)


def t06_run(edm, s, files, work):
    s.row("pause.bin")
    s.click("Stop")
    stopped = s.until(lambda: (edm.row("pause.bin") or {}).get("state", {}).get("kind") == "paused", 10)
    had = ((edm.row("pause.bin") or {}).get("progress") or {}).get("downloaded", 0)
    pause(1.5)
    s.click("Resume")
    (r,) = s.finished(["pause.bin"], 180)
    ok, detail = whole(r, "pause.bin")
    return bool(stopped and had > 0 and ok), f"stopped with {had} bytes; {detail}"


BATCH = ["batch-a.bin", "batch-b.bin", "batch-c.bin"]


def t07_setup(edm, files, work):
    if not set_clipboard("\n".join(f"{files}/{n}" for n in BATCH)):
        print("  (could not put the links on the clipboard)", flush=True)


def t07_run(edm, s, files, work):
    s.click("Tasks")
    s.click("Add links from clipboard")
    if not s.dialog("Links on the clipboard"):
        return False, f"no Links on the clipboard: dialogs {edm.dialogs()}"
    s.click("Start now")
    rows = s.finished(BATCH)
    return all_whole(rows, BATCH)


def t08_setup(edm, files, work):
    if not set_clipboard(f"{files}/later.bin"):
        print("  (could not put the link on the clipboard: it will be typed)", flush=True)


def t08_run(edm, s, files, work):
    if not open_add_url(edm, s, f"{files}/later.bin"):
        return False, f"no Download details: dialogs {edm.dialogs()}"
    s.click("Add for later")
    pause(2)
    waiting = edm.row("later.bin")
    held = bool(waiting) and waiting["state"]["kind"] not in ("running", "completed")
    s.click("Start queue")
    (r,) = s.finished(["later.bin"])
    ok, detail = whole(r, "later.bin")
    return held and ok, f"waited first: {held}; {detail}"


def t09_run(edm, s, files, work):
    s.click("Scheduler")
    if not s.dialog("Scheduler"):
        return False, f"no Scheduler: dialogs {edm.dialogs()}"
    pause(0.8)
    s.click("Only download between set hours")
    words = edm.dialog_texts()
    hour12 = "1 AM" in words or "7 AM" in words
    start, start_to, stop, stop_to = ("1 AM", "2 AM", "7 AM", "6 AM") if hour12 else ("01", "02", "07", "06")
    s.click(start)
    s.click(start_to)
    s.click(stop)
    s.click(stop_to)
    s.click("OK")

    def kept():
        sch = edm.settings().get("schedule") or {}
        return sch if sch.get("enabled") and sch.get("from") == "02:00" and sch.get("to") == "06:00" else None
    sch = s.until(kept, 6)
    got = edm.settings().get("schedule") or {}
    return bool(sch), f"schedule kept: enabled {got.get('enabled')}, {got.get('from')} to {got.get('to')}, days {got.get('days')}"


def t10_run(edm, s, files, work):
    s.click("Downloads")
    s.click("Speed limiter")
    s.click("Low speed (256 KB/s)")
    kept = s.until(lambda: edm.settings().get("speed_limit") == 256, 6)
    said = edm.inspect().get("said", "")
    return bool(kept), f"speed_limit {edm.settings().get('speed_limit')}; the status line: {said!r}"


def t11_setup(edm, files, work):
    add_and_finish(edm, files, ["found-alpha.bin", "other-beta.bin", "third-gamma.bin"])


def t11_run(edm, s, files, work):
    target = edm.row("found-alpha.bin")
    s.key("F", [CMD])
    s.until(lambda: edm.dialogs(), 4)
    s.type("alpha")
    s.click("Find next")
    s.click("Close")
    chosen = edm.inspect().get("selected") or []
    s.counted(clicks=2, windows=1, why="File › Open folder: it opens the system's file window, left to a person")
    return bool(target) and chosen == [target["id"]], f"chosen {chosen}, the file's row {target and target['id']}"


def t13_setup(edm, files, work):
    edm.call("downloads.add", urls=[f"{files}/gone.bin"])
    edm.wait("gone.bin", {"failed"}, 40)
    pause(2)


def t13_run(edm, s, files, work):
    r = edm.row("gone.bin")
    if not r or r["state"]["kind"] != "failed":
        return False, f"the download did not fail first: {r and r['state']}"
    s.row("gone.bin", right=True)
    s.click("Update download link")
    if not s.dialog("Update download link"):
        return False, f"no Update download link: dialogs {edm.dialogs()}"
    pause(0.5)
    s.type(f"{files}/fresh.bin")
    s.click("OK")
    done = s.until(lambda: (edm.row_id(r["id"]) or {}).get("state", {}).get("kind") in ("completed",), 60)
    pause(1)
    s.look()
    got = edm.row_id(r["id"])
    ok, detail = whole(got, "fresh.bin")
    return bool(done) and ok, f"{got and got['state']['kind']}; {detail}"


def t14_setup(edm, files, work):
    with LOCK:
        HELD.add("flaky.bin")
    edm.call("downloads.add", urls=[f"{files}/flaky.bin"])
    edm.wait("flaky.bin", {"failed"}, 40)
    pause(2)


def t14_ready(edm, files, work):
    # The server is back once the task begins.
    with LOCK:
        HELD.discard("flaky.bin")


def t14_run(edm, s, files, work):
    r = edm.row("flaky.bin")
    if not r or r["state"]["kind"] != "failed":
        return False, f"the download did not fail first: {r and r['state']}"
    s.row("flaky.bin")
    if not s.click("Resume"):
        s.click("Retry")
    (got,) = s.finished(["flaky.bin"])
    ok, detail = whole(got, "flaky.bin")
    return got["state"]["kind"] == "completed" and ok, detail


def t15_setup(edm, files, work):
    add_and_finish(edm, files, ["moved.bin"])
    (work / "moved here").mkdir(parents=True, exist_ok=True)


def t15_run(edm, s, files, work):
    r = edm.row("moved.bin")
    if not r or r["state"]["kind"] != "completed":
        return False, f"nothing finished to move: {r and r['state']}"
    old = pathlib.Path(r["path"])
    target = work / "moved here"
    s.row("moved.bin", right=True)
    s.click("Move/Rename")
    if not s.dialog("Move/Rename"):
        return False, f"no Move/Rename: dialogs {edm.dialogs()}"
    pause(0.6)
    words = edm.dialog_texts()
    folder = str(old.parent)
    if folder in words:
        s.click(folder)
    else:
        s.notes.append("the Folder field's words were not drawn whole: Tab from the name field")
        s.key("Tab")
    s.key("A", [CMD])
    s.type(str(target))
    s.click("OK")
    moved = s.until(lambda: (target / old.name).is_file(), 20)
    pause(1)
    got = edm.row_id(r["id"])
    ok, detail = whole(got, "moved.bin")
    return bool(moved) and not old.exists() and ok, f"in {target}: {bool(moved)}; old place empty: {not old.exists()}; {detail}"


def t16_setup(edm, files, work):
    add_and_finish(edm, files, ["kept.bin"])


def t16_run(edm, s, files, work):
    r = edm.row("kept.bin")
    if not r or r["state"]["kind"] != "completed":
        return False, f"nothing finished: {r and r['state']}"
    s.row("kept.bin")
    s.click("Delete")
    gone = s.until(lambda: edm.row("kept.bin") is None, 6)
    stays = pathlib.Path(r["path"]).is_file()
    return bool(gone) and stays, f"off the list: {bool(gone)}; file kept: {stays}; asked: {edm.dialogs()}"


def t17_setup(edm, files, work):
    add_and_finish(edm, files, ["done-a.bin", "done-b.bin"])


def t17_run(edm, s, files, work):
    rows = [edm.row(n) for n in ("done-a.bin", "done-b.bin")]
    if not all(r and r["state"]["kind"] == "completed" for r in rows):
        return False, "nothing finished to clear"
    s.click("Delete completed")
    gone = s.until(lambda: not any(x["state"]["kind"] == "completed" for x in edm.rows()), 6)
    stays = all(pathlib.Path(r["path"]).is_file() for r in rows)
    return bool(gone) and stays, f"off the list: {bool(gone)}; files kept: {stays}"


SERIES = ["photo1.bin", "photo2.bin", "photo3.bin"]


def t18_run(edm, s, files, work):
    s.click("Tasks")
    s.click("Add numbered series")
    if not s.dialog("Add numbered series"):
        return False, f"no Add numbered series: dialogs {edm.dialogs()}"
    pause(0.6)
    s.type(f"{files}/photo*.bin")
    s.key("Tab")
    s.type("1")
    s.key("Tab")
    s.type("3")
    s.click("Start now")
    rows = s.finished(SERIES)
    return all_whole(rows, SERIES)


def t19_setup(edm, files, work):
    (work / "new downloads").mkdir(parents=True, exist_ok=True)


def t19_run(edm, s, files, work):
    new = work / "new downloads"
    now = str(pathlib.Path(edm.settings().get("download_dir") or ""))
    s.click("Options")
    if not s.dialog("Options"):
        return False, f"no Options: dialogs {edm.dialogs()}"
    pause(0.8)
    s.click("Folders")
    pause(0.6)
    words = edm.dialog_texts()
    # The field's words, whole, or their end where the field shows only that.
    field = now if now in words else next(
        (w for w in words if len(w.lstrip("…. ")) > 8 and now.endswith(w.lstrip("…. "))), None)
    if field:
        s.click(field)
    else:
        s.notes.append("the Download folder field's words were not found: its label clicked, then Tab")
        s.click("Download folder")
        s.key("Tab")
    s.key("A", [CMD])
    s.type(str(new))
    s.click("OK")
    kept = s.until(lambda: pathlib.Path(edm.settings().get("download_dir") or "") == new, 6)
    return bool(kept), f"download_dir {edm.settings().get('download_dir')}; still open: {edm.dialogs()}"


DRIVEN = {
    "T05": (t05_setup, None, t05_run),
    "T06": (t06_setup, None, t06_run),
    "T07": (t07_setup, None, t07_run),
    "T08": (t08_setup, None, t08_run),
    "T09": (None, None, t09_run),
    "T10": (None, None, t10_run),
    "T11": (t11_setup, None, t11_run),
    "T13": (t13_setup, None, t13_run),
    "T14": (t14_setup, t14_ready, t14_run),
    "T15": (t15_setup, None, t15_run),
    "T16": (t16_setup, None, t16_run),
    "T17": (t17_setup, None, t17_run),
    "T18": (None, None, t18_run),
    "T19": (t19_setup, None, t19_run),
}


# ---------------------------------------------------------------- the list

def number(cell):
    """The whole number a cell starts with ("2 + 1 a video" is 2); None where empty."""
    m = re.match(r"\s*(\d+)", cell or "")
    return int(m.group(1)) if m else None


def read_tasks():
    """docs/tasks.md's table: each task's words, how it is done, its path and counts,
    and reference A's as a person filled them in."""
    tasks = []
    for line in TASKS_DOC.read_text(encoding="utf-8").splitlines():
        if not re.match(r"^\| T\d\d \|", line):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        cells += [""] * (11 - len(cells))
        tasks.append({
            "id": cells[0], "task": cells[1], "how": cells[2], "path": cells[3],
            "expected": {"clicks": number(cells[4]), "keys": number(cells[5]),
                         "typed": number(cells[6]), "windows": number(cells[7]), "windows_text": cells[7]},
            "reference_a": {"steps": number(cells[8]), "windows": number(cells[9]), "path": cells[10]},
        })
    return tasks


def run_task(task, files):
    setup, ready, run = DRIVEN[task["id"]]
    work = WORK / task["id"]
    home = work / "home"
    (home / "downloads").mkdir(parents=True)
    # A new install: only where downloads go is chosen (B63's new home).
    (home / "settings.json").write_text(json.dumps({"download_dir": str(home / "downloads")}), encoding="utf-8")
    edm = Dm(home)
    record = {"clicks": None, "keys": None, "typed": None, "windows": None, "ok": False, "detail": "",
              "opened": [], "done": [], "notes": []}
    try:
        edm.start()
        if setup:
            setup(edm, files, work)
        if not close_dialogs(edm):
            record["notes"].append(f"left open before the task, not counted: {edm.dialogs()}")
        if ready:
            ready(edm, files, work)
        edm.call("ui.window", to="shown")
        pause(0.8)
        s = Steps(edm)
        try:
            ok, detail = run(edm, s, files, work)
        except Exception:   # a step that broke fails its task; the rest still run
            ok, detail = False, traceback.format_exc().replace("\n", " | ")[-600:]
        record.update(clicks=s.clicks, keys=s.keys, typed=s.typed, windows=s.windows, ok=bool(ok),
                      detail=detail, opened=s.opened, done=s.done, notes=record["notes"] + s.notes)
    except Exception:
        record["detail"] = "EDM could not be driven: " + traceback.format_exc().replace("\n", " | ")[-600:]
    finally:
        try:
            edm.quit()
        except Exception:
            if edm.proc:
                edm.proc.kill()
    return record


def main():
    tasks = read_tasks()
    if len(tasks) != 20:
        sys.exit(f"docs/tasks.md lists {len(tasks)} tasks, not twenty")
    asked = [a.upper() for a in sys.argv[1:] if re.match(r"^[Tt]\d\d$", a)]
    unknown = [a for a in asked if a not in {t["id"] for t in tasks}]
    if unknown:
        sys.exit(f"no such task: {', '.join(unknown)}")
    driven_doc = {t["id"] for t in tasks if t["how"] == "driven"}
    if driven_doc != set(DRIVEN):
        sys.exit(f"docs/tasks.md and this script disagree on what is driven: "
                 f"{sorted(driven_doc ^ set(DRIVEN))}")
    if not BINARY.exists():
        sys.exit(f"no EDM at {BINARY}: cargo build -p {SHIPPED}")
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)
    origin = start_server()
    files = f"{origin}/files"
    print(f"EDM: {BINARY}\nserver: {origin}\nwork: {WORK}", flush=True)

    for task in tasks:
        if task["how"] != "driven":
            e = task["expected"]
            task["EDM"] = {"clicks": e["clicks"], "keys": e["keys"], "typed": e["typed"], "windows": e["windows"],
                          "windows_text": e["windows_text"], "ok": None, "detail": "by hand: counted from its path",
                          "source": "docs/tasks.md"}
            continue
        if asked and task["id"] not in asked:
            task["EDM"] = None
            continue
        print(f"\n{task['id']}  {task['task']}", flush=True)
        record = run_task(task, files)
        record["source"] = "driven"
        task["EDM"] = record
        for step in record["done"]:
            print(f"    {step}", flush=True)
        for note in record["notes"]:
            print(f"    note: {note}", flush=True)
        print(f"  {'DONE' if record['ok'] else 'FAIL'}  {record['detail']}", flush=True)
        e = task["expected"]
        if record["clicks"] is not None and any(
                record[k] != e[k] for k in ("clicks", "keys", "typed") if e[k] is not None):
            print(f"  (docs/tasks.md expects {e['clicks']} clicks, {e['keys']} keys, {e['typed']} typed)", flush=True)
    set_clipboard("")

    print("\nTask                                              How      Clicks Keys Typed Steps Windows  Done  Ref A steps/windows", flush=True)
    more, failed = [], []
    for task in tasks:
        d = task["EDM"]
        if d is None:
            continue
        steps = None if d["clicks"] is None else d["clicks"] + (d["keys"] or 0) + (d["typed"] or 0)
        d["steps"] = steps
        ref = task["reference_a"]
        windows = d.get("windows_text") or d["windows"]
        done = "-" if d["ok"] is None else ("yes" if d["ok"] else "NO")
        if d["ok"] is False:
            failed.append(task["id"])
        refs = "" if ref["steps"] is None else f"{ref['steps']}/{'' if ref['windows'] is None else ref['windows']}"
        takes_more = (ref["steps"] is not None and steps is not None
                      and (steps > ref["steps"] or (steps == ref["steps"] and ref["windows"] is not None
                                                    and d["windows"] is not None and d["windows"] > ref["windows"])))
        task["edm_takes_more"] = takes_more
        if takes_more:
            more.append(task)
        name = f"{task['id']} {task['task']}"[:49]
        print(f"{name:<50}{task['how']:<9}{_n(d['clicks']):>6}{_n(d['keys']):>5}{_n(d['typed']):>6}"
              f"{_n(steps):>6} {str(windows if windows is not None else '-'):>7}  {done:<5} {refs}"
              + ("   EDM takes more" if takes_more else ""), flush=True)

    unfilled = [t["id"] for t in tasks if t["reference_a"]["steps"] is None]
    if unfilled:
        print(f"\nReference A not counted yet for {len(unfilled)} task(s): {', '.join(unfilled)} "
              f"(a person fills its columns in docs/tasks.md)", flush=True)
    if more:
        print("\nWhere EDM takes more than reference A, each a row to propose in docs/gaps.md:", flush=True)
        for t in more:
            d, ref = t["EDM"], t["reference_a"]
            print(f"  {t['id']} {t['task']}: EDM {d['steps']} steps and {d['windows']} windows, "
                  f"reference A {ref['steps']} and {ref['windows']} ({ref['path'] or 'its path not written'})", flush=True)
    out = WORK / "task-steps.json"
    out.write_text(json.dumps({"binary": str(BINARY), "tasks": tasks}, indent=1, ensure_ascii=False), encoding="utf-8")
    print(f"\nthe record: {out}", flush=True)
    if failed:
        print(f"not done: {', '.join(failed)}", flush=True)
    sys.exit(1 if failed else 0)


def _n(v):
    return "-" if v is None else str(v)


if __name__ == "__main__":
    main()
