"""Drives the real desktop binary and reports what it did.

    python3 scripts/app-check.py

What the unit tests cannot see: the app as a process. It launches a debug
build with its own DM_HOME, and checks the things that only exist once the
binary is running — the single-instance lock, a second launch handing its
links to the first, a browser's native-messaging hand-off landing in the
running app, and a download that goes all the way to the folder the
settings name, sorted as they say.

macOS and Linux; it opens the real window, so it wants a display.
"""
import json, os, pathlib, shutil, socket, struct, subprocess, sys, tempfile, threading, time
WINDOWS = sys.platform == "win32"
if WINDOWS:
    import msvcrt
else:
    import fcntl
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = pathlib.Path(__file__).resolve().parent.parent
# Where cargo put the client: its own target directory unless one was named.
TARGET = pathlib.Path(os.environ["CARGO_TARGET_DIR"]) if os.environ.get("CARGO_TARGET_DIR") else ROOT / "desktop" / "target"
CLIENT = "dm-desktop"
EXE = CLIENT + (".exe" if WINDOWS else "")
# Or the binary a package installed, so what is shipped is what is driven.
BINARY = pathlib.Path(os.environ["DM_BINARY"]) if os.environ.get("DM_BINARY") else TARGET / "debug" / EXE
PAYLOAD = bytes(range(256)) * 4096  # 1 MiB, distinctive

failures = []


def check(what, actual, expected=True):
    ok = actual == expected
    print(f"  {'ok ' if ok else 'FAIL'} {what}" + ("" if ok else f"\n       expected {expected!r}\n       got      {actual!r}"))
    if not ok:
        failures.append(what)


# The dm being waited on, so a wait can tell "not yet" from "dead": an app
# that opened its window and then died — a tray library it could not load —
# looked like a hand-off that never arrived, with the reason on a stderr the
# timeout never got round to printing.
APP = None


def wait_for(what, condition, seconds=15):
    deadline = time.time() + seconds
    while time.time() < deadline:
        value = condition()
        if value:
            return value
        if APP is not None and APP.poll() is not None:
            err = APP.stderr.read().decode(errors="replace").strip()
            raise RuntimeError(f"dm exited with code {APP.returncode} while waiting for: {what}\n"
                               + ("  it said:\n    " + err.replace("\n", "\n    ") if err else "  it said nothing"))
        time.sleep(0.1)
    raise TimeoutError(what)


HITS = {}


class Serve(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        HITS[self.path] = HITS.get(self.path, 0) + 1
        if self.path.startswith("/v/"):
            return self.stream()
        if self.path == "/watch":
            page = b'<html><body><video src="/clip.mp4" controls></video></body></html>'
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if self.path == "/clip.mp4":
            self.send_response(200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Content-Length", str(len(CLIP)))
            self.end_headers()
            self.wfile.write(CLIP)
            return
        body = PAYLOAD
        rng = self.headers.get("Range")
        status = 200
        if rng and rng.startswith("bytes="):
            a, b = rng[6:].split("-")
            a = int(a or 0)
            b = int(b) if b else len(body) - 1
            b = min(b, len(body) - 1)
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {a}-{b}/{len(body)}")
            body = body[a:b + 1]
        else:
            self.send_response(status)
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Type", "application/zip")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


SEGMENTS = [bytes([i]) * 4096 for i in range(4)]
CLIP = bytes(i * 13 % 251 for i in range(640 * 1024))


def stream_of(handler):
    """A three-file HLS stream: master, one rendition, four segments."""
    path = handler.path
    if path.endswith("/master.m3u8"):
        body = b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=400000,RESOLUTION=640x360\nlow/index.m3u8\n"
        kind = "application/vnd.apple.mpegurl"
    elif path.endswith("/index.m3u8"):
        body = ("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n"
                + "".join(f"#EXTINF:4.0,\nseg{i}.ts\n" for i in range(len(SEGMENTS)))
                + "#EXT-X-ENDLIST\n").encode()
        kind = "application/vnd.apple.mpegurl"
    else:
        index = int(path.rsplit("seg", 1)[1].split(".")[0])
        body, kind = SEGMENTS[index], "video/mp2t"
    handler.send_response(200)
    handler.send_header("Content-Type", kind)
    handler.send_header("Content-Length", str(len(body)))
    handler.end_headers()
    handler.wfile.write(body)


Serve.stream = stream_of


def frame(message):
    body = json.dumps(message).encode()
    return struct.pack("<I", len(body)) + body


def read_frames(data):
    out, at = [], 0
    while at + 4 <= len(data):
        n = struct.unpack("<I", data[at:at + 4])[0]
        out.append(json.loads(data[at + 4:at + 4 + n]))
        at += 4 + n
    return out


def lock_is_held(home):
    with open(home / "instance.lock", "a+") as f:
        if WINDOWS:
            # dm holds the whole file with LockFileEx; a byte of it is
            # enough to see that, and is given back at once.
            f.seek(0)
            try:
                msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
            except OSError:
                return True
            f.seek(0)
            msvcrt.locking(f.fileno(), msvcrt.LK_UNLCK, 1)
            return False
        try:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            return True
        fcntl.flock(f, fcntl.LOCK_UN)
        return False


def rows(home):
    if not (home / "queue.json").exists():
        return []
    return json.loads((home / "queue.json").read_text() or "[]")


def hand_off(env, url, ident="one"):
    """One browser hand-off through the binary in host mode; returns the replies."""
    host = subprocess.run([BINARY, "--native-host"], env=env, timeout=20,
                          input=frame({"protocolVersion": 1, "type": "engine.hello", "id": "h"})
                          + frame({"protocolVersion": 1, "type": "download.start", "id": ident,
                                   "url": url, "referrer": "https://page.example/"}),
                          capture_output=True)
    return read_frames(host.stdout), host.stderr


def unchecked(what, why):
    """Said as not checked, never as passed: a check the desktop would not
    answer used to print "ok" exactly like one that held."""
    print(f"  --   {what} (not checked: {why})")


def windows_of(name):
    """How many windows the process shows, or None where the desktop will not say."""
    if sys.platform == "darwin":
        script = f'tell application "System Events" to count windows of process "{name}"'
        got = subprocess.run(["osascript", "-e", script], capture_output=True, text=True, timeout=10)
        return int(got.stdout.strip()) if got.returncode == 0 and got.stdout.strip().isdigit() else None
    # X11 (Xvfb in the container): the mapped top-level windows titled dm.
    if shutil.which("xwininfo") and os.environ.get("DISPLAY"):
        got = subprocess.run(["xwininfo", "-root", "-tree"], capture_output=True, text=True, timeout=10)
        if got.returncode != 0:
            return None
        mapped = 0
        for line in got.stdout.splitlines():
            if '"dm"' not in line:
                continue
            ident = line.split()[0]
            state = subprocess.run(["xwininfo", "-id", ident], capture_output=True, text=True, timeout=10)
            if "IsViewable" in state.stdout:
                mapped += 1
        return mapped
    return None


def launch(env, *args):
    global APP
    APP = subprocess.Popen([BINARY, *args], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    return APP


def stop(app, home):
    global APP
    APP = None
    app.terminate()
    try:
        app.wait(10)
    except subprocess.TimeoutExpired:
        app.kill()
    err = app.stderr.read().decode(errors="replace")
    if err.strip():
        print("  dm said on stderr:\n    " + err.strip().replace("\n", "\n    "))
    wait_for("the lock was not released on exit", lambda: not lock_is_held(home), 10)


def stop_any(home):
    """Ends a dm the host started, which is nobody's child here."""
    if WINDOWS:
        subprocess.run(["taskkill", "/F", "/IM", BINARY.name], capture_output=True)
    else:
        got = subprocess.run(["pgrep", "-f", str(BINARY)], capture_output=True, text=True)
        for pid in got.stdout.split():
            try:
                os.kill(int(pid), 15)
            except ProcessLookupError:
                pass
    wait_for("the dm the host started did not exit", lambda: not lock_is_held(home), 15)


def main():
    print("building the client")
    cargo = shutil.which("cargo") or str(pathlib.Path.home() / ".cargo" / "bin" / "cargo")
    if not os.environ.get("DM_BINARY"):
        subprocess.run([cargo, "build", "-p", CLIENT], cwd=ROOT / "desktop", check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-home-"))
    downloads = home / "Downloads"
    env = {**os.environ, "DM_HOME": str(home)}
    # Three attempts, not the ten a person gets: a check that a failure is a
    # failure would otherwise wait out minutes of half-minute retries.
    settings = {"download_dir": str(downloads), "categorize": True, "connections": 4, "attempts": 3}
    (home / "settings.json").write_text(json.dumps(settings))
    # The PATH a Dock or login launch has: no Homebrew, no ~/.local/bin. The
    # tools dm runs are found by their usual homes, or they are not found.
    # On Windows, the System32 a Start-menu or browser launch is sure of.
    if WINDOWS:
        root = os.environ.get("SystemRoot", r"C:\Windows")
        env["PATH"] = f"{root}\\System32;{root}"
    else:
        env["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"

    server = ThreadingHTTPServer(("127.0.0.1", 0), Serve)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    port = server.server_address[1]
    url = lambda name: f"http://127.0.0.1:{port}/{name}"

    try:
        print("one dm, links handed to it, a browser hand-off, downloads sorted where the settings say")
        app = launch(env)
        wait_for("the app never took its lock", lambda: (home / "instance.lock").exists() and lock_is_held(home))
        check("a running dm holds the instance lock", lock_is_held(home))
        shown = wait_for("the window never showed", lambda: windows_of(BINARY.name) or None, 10) \
            if windows_of(BINARY.name) is not None else None
        if shown is None:
            unchecked("a normal launch shows its window", "the desktop will not count windows here")
        else:
            check("a normal launch shows its window", shown >= 1)

        second = subprocess.run([BINARY, url("second.zip")], env=env, timeout=20, capture_output=True)
        check("a second launch exits rather than opening beside the first", second.returncode, 0)
        queued = wait_for("the link from the second launch never reached the queue",
                          lambda: [r for r in rows(home) if r["url"].endswith("/second.zip")])
        check("the running dm queued the second launch's link", len(queued), 1)
        # Taken after the link, on the same pass or the next: waited for,
        # not looked at once the link is seen.
        marker = home / "inbox" / "show"
        try:
            consumed = wait_for("the show marker was never consumed", lambda: not marker.exists(), 5)
        except TimeoutError:
            consumed = False
        check("the show marker was consumed", consumed)
        check("only one dm is running", lock_is_held(home))

        replies, err = hand_off(env, url("handed.zip"))
        check("the host answers hello", [r["type"] for r in replies][:1], ["engine.hello"])
        check("the host accepts the download", [r["type"] for r in replies][1:2], ["download.started"])
        check("the host wrote nothing but frames", err, b"")
        check("the host did not start a second dm", lock_is_held(home))
        handed = wait_for("the hand-off never reached the queue",
                          lambda: [r for r in rows(home) if r["url"].endswith("/handed.zip")])
        check("the hand-off carries the page it came from", handed[0]["referrer"], "https://page.example/")
        check("the inbox is empty once consumed", [p.name for p in (home / "inbox").glob("*.json")], [])

        done = wait_for("the downloads never completed",
                        lambda: (lambda d: d if len(d) == 2 else None)(
                            [r for r in rows(home) if r["state"]["kind"] == "completed"]), 30)
        for row in done:
            path = pathlib.Path(row["path"])
            check(f"{path.name} landed in the Archives folder of the download dir",
                  path.parent, downloads / "Archives")
            check(f"{path.name} holds the bytes served", path.read_bytes() == PAYLOAD)
        check("no row failed", [r["state"] for r in rows(home) if r["state"]["kind"] != "completed"], [])


        print("a stream and a player page, through the running app")
        hand_off(env, url("v/master.m3u8"), "hls")
        hand_off(env, url("watch"), "page")
        def both():
            done = {r["url"].rsplit("/", 1)[1]: r for r in rows(home) if r["state"]["kind"] in ("completed", "failed")}
            return done if {"master.m3u8", "watch"} <= set(done) else None
        done = wait_for("the stream or the page never settled", both, 90)
        hls = done["master.m3u8"]
        check("an HLS stream completes as one file", hls["state"]["kind"], "completed", )
        if hls["state"]["kind"] == "completed":
            check("holding every segment in order", pathlib.Path(hls["path"]).read_bytes() == b"".join(SEGMENTS))
        page = done["watch"]
        # Where dm itself looks for the tool, on either platform.
        if WINDOWS:
            homes = [pathlib.Path(os.environ.get("LOCALAPPDATA", "")) / "Microsoft" / "WinGet" / "Links",
                     pathlib.Path.home() / "scoop" / "shims",
                     pathlib.Path(os.environ.get("ProgramData", "")) / "chocolatey" / "bin"]
            homes += sorted((pathlib.Path(os.environ.get("APPDATA", "")) / "Python").glob("Python3*/Scripts"))
            ytdlp = shutil.which("yt-dlp", path=os.pathsep.join(map(str, homes)))
        else:
            ytdlp = shutil.which("yt-dlp", path="/opt/homebrew/bin:/usr/local/bin:/usr/bin:" + str(pathlib.Path.home() / ".local/bin"))
        if ytdlp:
            check("a player page is handed to yt-dlp, found without the shell's PATH", page["state"], {"kind": "completed"})
            if page["state"]["kind"] == "completed":
                check("and the file it produced is the clip", pathlib.Path(page["path"]).read_bytes() == CLIP)
        else:
            # None installed: dm fetches yt-dlp itself into its own folder.
            exe = "yt-dlp.exe" if WINDOWS else "yt-dlp"
            check("with no yt-dlp installed, dm fetches its own and the page downloads",
                  (page["state"], (home / "tools" / exe).exists()), ({"kind": "completed"}, True))
            if page["state"]["kind"] == "completed":
                check("and the file it produced is the clip", pathlib.Path(page["path"]).read_bytes() == CLIP)
        stop(app, home)
        check("the lock is released when dm exits", not lock_is_held(home))

        print("a hand-off with no dm running starts one")
        replies, _ = hand_off(env, url("cold.zip"), "cold")
        check("the host accepts the download", [r["type"] for r in replies][1:2], ["download.started"])
        wait_for("the host did not start a dm", lambda: lock_is_held(home), 20)
        check("a dm was started to take it", lock_is_held(home))
        cold = wait_for("the cold hand-off never completed",
                        lambda: [r for r in rows(home)
                                 if r["url"].endswith("/cold.zip") and r["state"]["kind"] == "completed"], 30)
        check("and it downloaded the file", pathlib.Path(cold[0]["path"]).read_bytes() == PAYLOAD)
        stop_any(home)

        print("a proxy that cannot be reached is not gone around")
        with_proxy = {**settings, "proxy": {"enabled": True, "type": "http", "host": "127.0.0.1", "port": 9}}
        (home / "settings.json").write_text(json.dumps(with_proxy))
        (home / "queue.json").unlink(missing_ok=True)
        app = launch(env)
        wait_for("the app never took its lock", lambda: lock_is_held(home))
        hand_off(env, url("proxied.zip"), "proxied")
        failed = wait_for("the proxied download neither failed nor finished",
                          lambda: [r for r in rows(home)
                                   if r["url"].endswith("/proxied.zip") and r["state"]["kind"] in ("failed", "completed")], 60)
        check("a download behind a dead proxy fails", failed[0]["state"]["kind"], "failed")
        check("and the server was never contacted directly", HITS.get("/proxied.zip", 0), 0)
        stop(app, home)
        (home / "settings.json").write_text(json.dumps(settings))

        print("a speed limit and a schedule, from the settings file to the transfer")
        (home / "settings.json").write_text(json.dumps({**settings, "speed_limit": 256}))
        (home / "queue.json").unlink(missing_ok=True)
        app = launch(env)
        wait_for("the app never took its lock", lambda: lock_is_held(home))
        started = time.time()
        hand_off(env, url("limited.zip"), "limited")
        wait_for("the limited download never completed",
                 lambda: [r for r in rows(home)
                          if r["url"].endswith("/limited.zip") and r["state"]["kind"] == "completed"], 60)
        took = time.time() - started
        # A mebibyte at 256 KB/s is four seconds; unlimited on loopback it
        # is a few milliseconds.
        check("a 256 KB/s limit holds a 1 MiB download to seconds, not milliseconds", took >= 3)
        stop(app, home)

        now = time.localtime()
        later = lambda h: f"{(now.tm_hour + h) % 24:02d}:{now.tm_min:02d}"
        closed = {"enabled": True, "from": later(2), "to": later(3), "days": [0, 1, 2, 3, 4, 5, 6]}
        (home / "settings.json").write_text(json.dumps({**settings, "schedule": closed}))
        (home / "queue.json").unlink(missing_ok=True)
        app = launch(env)
        wait_for("the app never took its lock", lambda: lock_is_held(home))
        hand_off(env, url("scheduled.zip"), "scheduled")
        wait_for("the scheduled hand-off never reached the queue",
                 lambda: [r for r in rows(home) if r["url"].endswith("/scheduled.zip")])
        time.sleep(3)
        held = [r for r in rows(home) if r["url"].endswith("/scheduled.zip")][0]
        check("outside the schedule's window a download waits", held["state"]["kind"] in ("queued", "paused"))
        check("and the server is not asked for it", HITS.get("/scheduled.zip", 0), 0)
        stop(app, home)
        (home / "settings.json").write_text(json.dumps(settings))

        print("the login item's launch stays in the tray")
        (home / "control.port").unlink(missing_ok=True)
        app = launch(env, "--hidden", "--control", "0")
        wait_for("the app never took its lock", lambda: lock_is_held(home))
        port_file = home / "control.port"
        wait_for("the control port was never written", lambda: port_file.exists() and port_file.read_text().strip())
        time.sleep(2)
        hidden = windows_of(BINARY.name)
        if hidden is None:
            unchecked("a --hidden launch shows no window", "the desktop will not count windows here")
        else:
            check("a --hidden launch shows no window", hidden, 0)
        # It makes no window until one is asked for, and still downloads.
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")
        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())
        check("in the tray, the control protocol answers", call("auth", token=(home / "control.token").read_text().strip())["ok"], True)
        call("downloads.add", urls=[url("tray.zip")])
        wait_for("a download added in the tray never finished",
                 lambda: [r for r in rows(home) if r["url"].endswith("/tray.zip") and r["state"]["kind"] == "completed"])
        check("in the tray, a download finishes", True)
        second = subprocess.run([BINARY], env=env, timeout=20, capture_output=True)
        check("opening dm again hands over and ends", second.returncode, 0)
        wait_for("the window never came out of the tray", lambda: windows_of(BINARY.name) in (None, 1))
        shown = windows_of(BINARY.name)
        if shown is None:
            unchecked("opening dm again shows the tray's window", "the desktop will not count windows here")
        else:
            check("opening dm again shows the tray's window", shown, 1)
        shot = call("ui.screenshot", name="from-the-tray")
        check("and it is drawn", shot.get("error"), None)
        conn.close()
        stop(app, home)

        print("the command line, handed to a running dm")
        app = launch(env)
        wait_for("the app never took its lock", lambda: lock_is_held(home))
        chosen = home / "chosen"
        subprocess.run([BINARY, "--save-to", str(chosen), "--name", "named.zip", "--later", url("cli.zip")],
                       env=env, timeout=20, capture_output=True)
        waiting = wait_for("the command-line link never reached the queue",
                           lambda: [r for r in rows(home) if r["url"].endswith("/cli.zip")])
        check("--later puts it in the main queue, not started",
              (waiting[0]["state"]["kind"], waiting[0]["queue"]), ("paused", "Main queue"))
        subprocess.run([BINARY, "--start-queue"], env=env, timeout=20, capture_output=True)
        done = wait_for("--start-queue never started the queue",
                        lambda: [r for r in rows(home) if r["url"].endswith("/cli.zip") and r["state"]["kind"] == "completed"], 30)
        check("--start-queue starts the queue, and --save-to and --name say where and as what",
              pathlib.Path(done[0]["path"]), chosen / "named.zip")
        check("the file holds the bytes served", (chosen / "named.zip").read_bytes() == PAYLOAD)
        stop(app, home)

        print("the control protocol drives the running window")
        # A dm ended hard (as Windows' terminate is) leaves its port file;
        # the new one's is waited for, not the old one read.
        (home / "control.port").unlink(missing_ok=True)
        app = launch(env, "--control", "0")
        port_file = home / "control.port"
        wait_for("the control port was never written", lambda: port_file.exists() and port_file.read_text().strip())
        token = (home / "control.token").read_text().strip()
        if WINDOWS:
            unchecked("the token is private", "Windows keeps it private by its folder's permissions, not mode bits")
        else:
            check("the token is private", oct((home / "control.token").stat().st_mode & 0o777), "0o600")
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")
        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())
        check("a request before the token is refused", call("commands.list")["ok"], False)
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")
        check("the token is accepted", call("auth", token=token)["ok"], True)
        ids = [c["id"] for c in call("commands.list")["result"]]
        check("every registry command is listed", len(ids) >= 40 and "program.add_url" in ids)
        # Downloads finishing must not hold the window up: each one's
        # notification once froze it for two seconds on macOS (ui-shot.py).
        for n in range(4):
            call("downloads.add", urls=[url(f"fast-{n}.zip")])
        time.sleep(0.5)
        slowest = 0.0
        for n in range(4):
            started = time.time()
            call("downloads.add", urls=[url(f"later-{n}.zip")])
            slowest = max(slowest, time.time() - started)
        check("the window answers in under a second while downloads finish",
              f"{slowest:.2f}s" if slowest >= 1.0 else "ok", "ok")
        check("a command runs in the window", call("command.run", id="program.add_url")["ok"], True)
        dialogs = call("ui.inspect")["result"]["dialogs"]
        check("and the window shows what it opened", "Add URL" in dialogs)
        shot = call("ui.screenshot", name="control-check")
        if shot.get("ok"):
            png = pathlib.Path(shot["result"]["path"])
            check("a screenshot is a PNG of the window", png.read_bytes()[:4], b"\x89PNG")
        else:
            check("a screenshot of the running window", shot.get("error"), None)
        conn.close()
        stop(app, home)

        print("headless, driven by the control protocol and by an MCP client")
        (home / "control.port").unlink(missing_ok=True)
        app = launch(env, "--headless", "--control", "0")
        port_file = home / "control.port"
        wait_for("the headless dm never wrote its port", lambda: port_file.exists() and port_file.read_text().strip())
        token = (home / "control.token").read_text().strip()
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")
        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())
        call("auth", token=token)
        added = call("downloads.add", urls=[url("headless.zip")])
        check("a download added headless", added.get("result"), {"added": 1})
        done = wait_for("the headless download never finished",
                        lambda: [r for r in rows(home) if r["url"].endswith("/headless.zip") and r["state"]["kind"] == "completed"], 30)
        check("and it holds the bytes served", pathlib.Path(done[0]["path"]).read_bytes() == PAYLOAD)
        shot = call("ui.screenshot", name="none")
        check("a screenshot is refused with no window", (shot["ok"], "headless" in shot.get("error", "")), (False, True))
        conn.close()
        mcp_in = "\n".join([
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                        "params": {"name": "dm_add_downloads", "arguments": {"urls": [url("agent.zip")]}}}),
        ]) + "\n"
        mcp = subprocess.run([BINARY, "mcp"], env=env, input=mcp_in.encode(), capture_output=True, timeout=30)
        answers = [json.loads(l) for l in mcp.stdout.decode().splitlines() if l.strip()]
        check("the MCP server introduces itself", answers[0]["result"]["serverInfo"]["name"] if answers else None, "dm")
        check("and an agent's tool call adds a download", len(answers) == 2 and not answers[1]["result"].get("isError"), True)
        wait_for("the agent's download never finished",
                 lambda: [r for r in rows(home) if r["url"].endswith("/agent.zip") and r["state"]["kind"] == "completed"], 30)
        check("which completes", True)
        stop(app, home)

    finally:
        server.shutdown()
        if WINDOWS:
            subprocess.run(["taskkill", "/F", "/IM", BINARY.name], capture_output=True)
        else:
            subprocess.run(["pgrep", "-f", str(BINARY)], capture_output=True)
        shutil.rmtree(home, ignore_errors=True)

    if failures:
        print(f"\n{len(failures)} check(s) failed")
        sys.exit(1)
    print("\nthe app does what it says")


if __name__ == "__main__":
    main()
