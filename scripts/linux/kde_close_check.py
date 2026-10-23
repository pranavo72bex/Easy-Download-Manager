#!/usr/bin/env python3
"""Closing dm's window under KDE's compositor (KWin, Wayland), run by
scripts/linux-kde-close-check.sh in the Arch container.

What a KDE user reported: closing the window left a phantom window. This
does what that person does, and reads what KWin itself says about it:

  1. dm opens a window KWin manages;
  2. the window is closed (KWin's own closeWindow, the same request the
     close button sends): dm minimizes into the task manager instead of
     going on showing a window it no longer draws, KWin says it is minimized
     (it leaves the screen), and dm runs on, answers, and downloads on;
  3. the task manager restores it (minimized = false): the window is back,
     and it draws and answers.

KWin is driven through its scripting interface; the scripts call back into
a D-Bus service of this check's, which records what KWin reports.
"""
import http.server
import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import threading
import time

from gi.repository import Gio, GLib

BINARY = os.environ.get("DM_BINARY", "/target/debug/dm-desktop")
HERE = pathlib.Path(__file__).parent
failed = 0
reports = []  # what KWin's scripts said, in order


def check(what, ok, detail=""):
    global failed
    print(("ok   " if ok else "FAIL ") + what + ("" if ok else f"  [{detail}]"), flush=True)
    failed += 0 if ok else 1


def wait(probe, seconds):
    end = time.time() + seconds
    while time.time() < end:
        got = probe()
        if got:
            return got
        time.sleep(0.3)
    return None


# --- a D-Bus service KWin's scripts report to -------------------------------
PROBE = """<node><interface name="test.dm.Probe">
<method name="report"><arg type="s" direction="in"/></method></interface></node>"""


def serve_probe():
    def call(connection, sender, path, iface, method, params, invocation):
        reports.append(params.unpack()[0])
        invocation.return_value(None)

    def acquired(connection, name):
        info = Gio.DBusNodeInfo.new_for_xml(PROBE)
        connection.register_object("/", info.interfaces[0], call, None, None)

    Gio.bus_own_name(Gio.BusType.SESSION, "test.dm.Probe", Gio.BusNameOwnerFlags.NONE, acquired, None, None)
    threading.Thread(target=GLib.MainLoop().run, daemon=True).start()


def kwin_script(source, name):
    """Loads and runs a KWin script, as `qdbus … loadScript` and `run` do."""
    path = pathlib.Path(tempfile.mkdtemp()) / f"{name}.js"
    path.write_text(source)
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def call(obj, iface, method, params, reply):
        return bus.call_sync("org.kde.KWin", obj, iface, method, params, reply,
                             Gio.DBusCallFlags.NONE, 10000, None)

    got = call("/Scripting", "org.kde.kwin.Scripting", "loadScript",
               GLib.Variant("(ss)", (str(path), name)), GLib.VariantType("(i)"))
    ident = got.unpack()[0]
    call(f"/Scripting/Script{ident}", "org.kde.kwin.Script", "run", None, None)
    return ident


def say(text):
    return (f'callDBus("test.dm.Probe", "/", "test.dm.Probe", "report", {json.dumps(text)});')


# A script that reports each dm window as it appears, and each change of its
# minimized state after, as KWin sees them.
WATCH = """
function dm(w) { return String(w.resourceClass).toLowerCase().indexOf("dm") >= 0 || String(w.caption) === "dm"; }
function watch(w) {
  if (!dm(w)) return;
  %s
  w.minimizedChanged.connect(function () { %s });
}
workspace.windowAdded.connect(watch);
workspace.windowList().forEach(watch);
""" % (
    say("added").replace('"added"', '"added:" + w.caption + ":" + w.resourceClass + ":" + w.minimized'),
    say("x").replace('"x"', '"minimized:" + w.minimized'),
)
CLOSE = 'workspace.windowList().forEach(function (w) { if (String(w.resourceClass).toLowerCase().indexOf("dm") >= 0 || String(w.caption) === "dm") { w.closeWindow(); } });'
RESTORE = 'workspace.windowList().forEach(function (w) { if (String(w.resourceClass).toLowerCase().indexOf("dm") >= 0 || String(w.caption) === "dm") { w.minimized = false; %s } });' % (
    say("x").replace('"x"', '"restored:" + w.minimized'))
STATE = 'workspace.windowList().forEach(function (w) { if (String(w.resourceClass).toLowerCase().indexOf("dm") >= 0 || String(w.caption) === "dm") { %s } });' % (
    say("x").replace('"x"', '"state:" + w.minimized + ":" + w.hidden'))


class Control:
    def __init__(self, home):
        port = int(wait(lambda: (home / "control.port").exists() and (home / "control.port").read_text().strip(), 30))
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=15)
        self.file = self.sock.makefile("rw")
        self.call("auth", {"token": (home / "control.token").read_text().strip()})

    def call(self, method, params=None):
        self.file.write(json.dumps({"id": 1, "method": method, "params": params or {}}) + "\n")
        self.file.flush()
        reply = json.loads(self.file.readline())
        if not reply.get("ok"):
            raise RuntimeError(reply.get("error"))
        return reply.get("result")


def main():
    serve_probe()
    home = pathlib.Path(tempfile.mkdtemp())
    (home / "dl").mkdir()
    (home / "settings.json").write_text(json.dumps({"download_dir": str(home / "dl")}))
    env = dict(os.environ, DM_HOME=str(home), HOME=str(home), XDG_SESSION_TYPE="wayland",
               XDG_CURRENT_DESKTOP="KDE")
    env.pop("DISPLAY", None)

    # A file to download after the window is closed.
    site = pathlib.Path(tempfile.mkdtemp())
    (site / "after-close.bin").write_bytes(os.urandom(300_000))
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(site), **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/after-close.bin"

    watcher = subprocess.Popen([sys.executable, str(HERE / "tray_watcher.py")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.7)
    host = subprocess.Popen([sys.executable, str(HERE / "tray_host.py")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    kwin_script(WATCH, "dm-watch")
    app = subprocess.Popen([BINARY, "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        check("KWin manages a window for dm", bool(wait(lambda: any(r.startswith("added:") for r in reports), 40)),
              str(reports))
        control = Control(home)
        check("the window starts shown, not minimized", any(r.startswith("added:") and r.endswith(":false") for r in reports),
              str(reports))
        time.sleep(3)  # the tray item registers with the watcher

        kwin_script(CLOSE, "dm-close")
        check("closing the window minimizes it: KWin says it left the screen",
              bool(wait(lambda: "minimized:true" in reports, 15)), str(reports))
        check("dm keeps running", app.poll() is None, app.stderr.read().decode()[-400:] if app.poll() is not None else "")
        state = control.call("ui.inspect")
        check("dm still answers", isinstance(state, dict))
        check("and it is not parked as hidden, drawing nothing", state["hidden"] is False)

        # Downloads go on with the window away.
        control.call("downloads.add", {"urls": [url]})
        done = wait(lambda: any(r["name"] == "after-close.bin" and "omplete" in r["status"]
                                for r in control.call("ui.inspect")["rows"]), 30)
        check("a download started with the window closed completes", bool(done))
        landed = list((home / "dl").rglob("after-close.bin"))
        check("and its file is whole",
              bool(landed) and landed[0].read_bytes() == (site / "after-close.bin").read_bytes())

        # The task manager brings the window back.
        kwin_script(RESTORE, "dm-restore")
        check("restoring it from the task manager shows the window again",
              bool(wait(lambda: "minimized:false" in reports and "restored:false" in reports, 15)), str(reports))
        control.call("downloads.add", {"urls": ["http://127.0.0.1:9/shown-again.bin"], "later": True})
        time.sleep(1)
        check("and it draws and answers: it keeps up with changes",
              any("shown-again.bin" in r["name"] for r in control.call("ui.inspect")["rows"]))

        # Closing it a second time does the same: the cycle repeats.
        reports.clear()
        kwin_script(CLOSE, "dm-close-again")
        check("closing it again minimizes it again", bool(wait(lambda: "minimized:true" in reports, 15)), str(reports))
        check("and dm is still there", app.poll() is None)
    finally:
        for p in (app, host, watcher):
            p.terminate()
        server.shutdown()
        print(f"{failed} failed", flush=True)
    return failed


sys.exit(1 if main() else 0)
