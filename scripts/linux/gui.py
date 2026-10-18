#!/usr/bin/env python3
"""Real mouse and keyboard on dm's window and tray, under X11.

Runs inside the Linux container (scripts/linux-gui-check.sh), where X11
input can be sent for real: a window manager (openbox), xdotool, and a
StatusNotifierWatcher, the tray service GNOME, KDE and waybar speak. The
tray is driven as their panels drive it, over D-Bus (dbusmenu "clicked"
events), since dm's tray is a StatusNotifierItem and no window to click.
What macOS will not let this shell do:

- close the window into the tray, then bring it back from the tray menu;
- turn the speed limiter on from the tray menu;
- quit from the tray menu;
- drop a file of links onto the window, dragged from a GTK drag source;
- download a file by hand: Add URL, the address typed, OK, Start now,
  Stop and Resume clicked where they are on screen, and the file checked;
- read the window the way a Linux screen reader (Orca) does, over AT-SPI.

Each is a real X11 event to the real binary; nothing is called directly.
"""
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import time

BINARY = pathlib.Path(os.environ.get("DM_BINARY", "/target/debug/dm-desktop"))
RESULTS = []


ORACLE = None


def check(name, ok, detail=""):
    RESULTS.append(bool(ok))
    print(f"  {'ok  ' if ok else 'FAIL'}  {name}" + (f" — {detail}" if detail and not ok else ""), flush=True)
    if not ok and ORACLE:
        try:
            seen = ORACLE("ui.inspect")
            rows = [(r.get("name"), r.get("status")) for r in seen.get("rows", [])]
            print(f"        dm says: dialogs {seen.get('dialogs')}, selected {seen.get('selected')}, rows {rows}", flush=True)
        except Exception as e:  # diagnostics only
            print(f"        (no word from dm: {e})", flush=True)


def oracle(home):
    """dm's own account of its window (the control protocol), read on a
    failure to say what the window held: diagnostics, never the check."""
    import socket
    port_file = home / "control.port"
    if not wait_for(lambda: port_file.exists() and port_file.read_text().strip(), 20):
        return None
    # The file outlives the dm that wrote it: after an exit, or a dm that
    # never opened, its port answers nobody. Diagnostics, so no oracle then.
    try:
        sock = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=10)
    except (OSError, ValueError):
        return None
    lines = sock.makefile("r")

    def call(method, **params):
        sock.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(lines.readline()).get("result") or {}

    call("auth", token=(home / "control.token").read_text().strip())
    return call


def x(*args):
    return subprocess.run(list(args), capture_output=True, text=True, timeout=20)


def wait_for(probe, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(0.2)
    return None


def dm_windows():
    """dm's top-level windows that are viewable."""
    found = x("xdotool", "search", "--onlyvisible", "--name", "^dm$").stdout.split()
    return found


def screen_reader_on():
    """Says on the accessibility bus that a screen reader is running, as Orca
    does; toolkits publish their windows over AT-SPI only then."""
    for prop in ("IsEnabled", "ScreenReaderEnabled"):
        x("gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus",
          "--method", "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", prop, "<true>")


# Run in a process of its own, which leaves the accessibility bus when it is
# done: a client that stays on the bus without answering it holds up GTK's
# accessibility bridge, and with it the tray's menu.
SPOKEN = r"""
import json, pyatspi
desktop = pyatspi.Registry.getDesktop(0)
found = None
for i in range(desktop.childCount):
    app = desktop[i]
    if not app or app.name != "dm-desktop":
        continue
    frames, buttons = [], []
    def walk(node, depth=0):
        if node is None or depth > 40:
            return
        role = node.getRoleName()
        if role == "frame":
            frames.append(node.name)
        elif role in ("push button", "toggle button", "check box"):
            buttons.append(node.name)
        for j in range(node.childCount):
            walk(node[j], depth + 1)
    walk(app)
    if buttons:
        found = [frames, buttons]
print(json.dumps(found))
"""


WHERE = r"""
import json, sys, pyatspi
role, name, contains = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
desktop = pyatspi.Registry.getDesktop(0)
found, seen = [], set()
for i in range(desktop.childCount):
    app = desktop[i]
    if not app or app.name != "dm-desktop":
        continue
    def walk(node, depth=0):
        if node is None or depth > 60:
            return
        r = node.getRoleName()
        seen.add(r)
        # A menu item's name carries its key ("Exit Ctrl+Q") or its arrow.
        if (role == "*" or r == role) and (node.name == name or (contains and (node.name or "").startswith(name))):
            try:
                e = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                if e.width > 0 and e.height > 0:
                    found.append([e.x + e.width // 2, e.y + e.height // 2])
            except Exception:
                pass
        for j in range(node.childCount):
            walk(node[j], depth + 1)
    walk(app)
print(json.dumps({"at": found[0] if found else None, "roles": sorted(seen)}))
"""


def where(role, name, contains=False):
    """Where on screen dm's control of this role and name is, read over
    AT-SPI as a screen reader reads it: its centre, or None. With
    `contains`, a name that starts with `name` counts (menu items)."""
    out = x(sys.executable, "-c", WHERE, role, name, "1" if contains else "0").stdout.strip().splitlines()
    return json.loads(out[-1]).get("at") if out else None


def click(role, name, seconds=10, contains=False):
    """Moves the real pointer onto the control and clicks it, as a person
    does. False if no such control shows within `seconds`."""
    at = wait_for(lambda: where(role, name, contains), seconds)
    if not at:
        return False
    x("xdotool", "mousemove", "--sync", str(at[0]), str(at[1]))
    time.sleep(0.2)
    x("xdotool", "click", "1")
    time.sleep(0.5)
    return True


def win_geo():
    w = dm_windows()
    if not w:
        return None
    g = {}
    for line in x("xdotool", "getwindowgeometry", "--shell", w[0]).stdout.split():
        if "=" in line:
            k, _, v = line.partition("=")
            if k in ("X", "Y", "WIDTH", "HEIGHT"):
                g[k] = int(v)
    return g if {"X", "Y"} <= g.keys() else None


def row_point(match):
    """The centre of a row on screen, from dm's own row rectangle (the
    control protocol) plus the window's origin. egui's accessibility tree
    reports table-row cells at the wrong place, so a row is never clicked
    through `where`; the toolbar, menus and dialogs are. `match` is checked
    against each row's name and its shown address, so a URL or a filename
    finds it."""
    geo = win_geo()
    if not geo or not ORACLE:
        return None
    tail = match.rstrip("/").rsplit("/", 1)[-1]
    for r in (ORACLE("ui.inspect").get("rows") or []):
        if not r.get("rect"):
            continue
        if match in (r.get("name"), r.get("url")) or tail and (r.get("name") == tail or tail in (r.get("url") or "")):
            a = r["rect"]
            # On its name, near the row's left end, where a person clicks: the
            # row's middle is off the window when the columns run past it.
            x_at = a[0] + min(80, (a[2] - a[0]) / 2)
            return (geo["X"] + int(x_at), geo["Y"] + int((a[1] + a[3]) / 2))
    return None


def click_row(match, button="1", seconds=10):
    """Clicks a row by name or URL with the real pointer. `button` 3 right-clicks."""
    at = wait_for(lambda: row_point(match), seconds)
    if not at:
        return False
    x("xdotool", "mousemove", "--sync", str(at[0]), str(at[1]))
    time.sleep(0.2)
    x("xdotool", "click", button)
    time.sleep(0.5)
    return True


def spoken():
    """What a screen reader finds of dm over AT-SPI: the window's name, and
    the names of its buttons."""
    out = x(sys.executable, "-c", SPOKEN).stdout.strip().splitlines()
    return json.loads(out[-1]) if out else None


def tray_item():
    """dm's tray item as a panel finds it: registered with the
    StatusNotifierWatcher, its menu read over dbusmenu. (bus name, menu
    path, {label: id}, item path), or None while there is none."""
    out = gdbus("--dest", "org.kde.StatusNotifierWatcher", "--object-path", "/StatusNotifierWatcher",
                "--method", "org.freedesktop.DBus.Properties.Get",
                "org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems")
    registered = re.findall(r"'([^']+)'", out)
    if not registered:
        return None
    name, _, path = registered[0].partition("/")
    if path:
        path = "/" + path
    else:
        # Registered by bus name alone, as this watcher keeps it: the item
        # is whichever of that name's objects is one, as a host finds it.
        tree = x("gdbus", "introspect", "--session", "--dest", name, "--object-path", "/",
                 "--recurse").stdout
        node, path = None, "/StatusNotifierItem"
        for line in tree.splitlines():
            line = line.strip()
            if line.startswith("node /"):
                node = line.split()[1]
            elif line.startswith("interface org.kde.StatusNotifierItem") and node:
                path = node
                break
    menu = re.search(r"objectpath '([^']+)'", gdbus(
        "--dest", name, "--object-path", path, "--method",
        "org.freedesktop.DBus.Properties.Get", "org.kde.StatusNotifierItem", "Menu"))
    if not menu:
        return None
    layout = gdbus("--dest", name, "--object-path", menu.group(1), "--method",
                   "com.canonical.dbusmenu.GetLayout", "0", " -1", "[]")  # " -1": not an option
    entries = {label.replace("_", ""): int(i)
               for i, label in re.findall(r"\((\d+), \{[^}]*'label': <'([^']*)'>", layout)}
    return (name, menu.group(1), entries, path) if entries else None


def tray_icon():
    """Whether dm's tray item is there, menu and all."""
    return tray_item() is not None


# dm's tray menu, by label, as a panel shows it.
SHOW, SPEED_LIMITER, SCHEDULER, QUIT = "Show dm", "Speed limiter on/off", "Scheduler…", "Quit dm"


def tray_menu(pick):
    """Clicks the tray menu item `pick` the way a panel does: a dbusmenu
    "clicked" event, which is what GNOME, KDE and waybar send."""
    item = wait_for(tray_item, 15)
    if not item or pick not in item[2]:
        return False
    name, menu, entries, _ = item
    gdbus("--dest", name, "--object-path", menu, "--method", "com.canonical.dbusmenu.Event",
          str(entries[pick]), "clicked", "<0>", "0")
    time.sleep(1.0)
    return True


def gdbus(*args):
    done = x("gdbus", "call", "--session", *args)
    return done.stdout or done.stderr


class SlowFiles:
    """A local server a person's session downloads from: ranges at about
    1 MB/s a connection (time enough to press Stop), a path that fails
    until told to work (for Retry), and any name."""

    def __init__(self):
        import http.server
        import socketserver
        import threading

        # Long enough, at the slow rate below and dm's default eight
        # connections, to still be running when the Stop and Delete scenes
        # reach the row: 24 MiB finished in about three seconds.
        self.body = bytes((i * 31 + 7) & 0xFF for i in range(48 * 1024 * 1024))
        self.small = bytes((i * 13 + 5) & 0xFF for i in range(512 * 1024))
        self.broken = True
        server = self

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                pass

            def do_HEAD(self):
                self.do_GET(head=True)

            def do_GET(self, head=False):
                if self.path.startswith("/broken") and server.broken:
                    self.send_response(500)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                body = server.small if "small" in self.path or "broken" in self.path else server.body
                start, end = 0, len(body) - 1
                asked = self.headers.get("Range")
                if asked:
                    a, _, b = asked[6:].partition("-")
                    start, end = int(a), min(int(b), end) if b else end
                    self.send_response(206)
                    self.send_header("Content-Range", f"bytes {start}-{end}/{len(body)}")
                else:
                    self.send_response(200)
                self.send_header("Content-Length", str(end - start + 1))
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("ETag", '"hand"')
                self.end_headers()
                if head:
                    return
                try:
                    for at in range(start, end + 1, 64 * 1024):
                        self.wfile.write(body[at:min(at + 64 * 1024, end + 1)])
                        if body is server.body:
                            time.sleep(0.064)
                except OSError:
                    pass

        class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
            daemon_threads = True

        self.server = Server(("127.0.0.1", 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}"

    def url(self, name):
        return f"{self.base}/{name}"


def key(*keys):
    for k in keys:
        x("xdotool", "key", "--clearmodifiers", k)
        time.sleep(0.3)


def type_text(text):
    x("xdotool", "type", "--delay", "15", text)


def field_at(name):
    """A field by its label: a text entry, or a number a person drags or
    types into (a spin button)."""
    for role in ("text", "entry", "spin button", "slider"):
        at = where(role, name)
        if at:
            return at
    return None


def click_field(name):
    """Clicks into the field labelled `name` and selects what it holds."""
    at = wait_for(lambda: field_at(name), 10)
    if not at:
        return False
    x("xdotool", "mousemove", "--sync", str(at[0]), str(at[1]))
    x("xdotool", "click", "1")
    time.sleep(0.2)
    key("ctrl+a")
    return True


def add_url_by_hand(url, then="Start now"):
    """Add URL, the address typed, OK, then `then` in Download details."""
    settle()
    if not click("push button", "Add URL"):
        return False
    if not click_field("Address"):
        return False
    type_text(url)
    if not click("push button", "OK"):
        return False
    return click("push button", then)


def settle():
    """Closes a Download complete dialog or progress window if one is up,
    as a person clears it before doing the next thing. With the pop-ups off
    for most of the session there is usually nothing to close."""
    for _ in range(4):
        if not (where("*", "Download complete") or where("*", "Download progress", True)):
            return
        key("Escape")
        time.sleep(0.4)


def finished(home, url, seconds):
    """Waits for the row to complete, then puts its dialog away."""
    done = wait_for(lambda: state_of(home, url) == "completed", seconds)
    settle()
    return done


def queue_rows(home):
    try:
        return json.loads((home / "queue.json").read_text())
    except (OSError, ValueError):
        return []


def row_of(home, url):
    return next((r for r in queue_rows(home) if r.get("url") == url), None)


def state_of(home, url):
    return ((row_of(home, url) or {}).get("state") or {}).get("kind")


def settings_of(home):
    try:
        return json.loads((home / "settings.json").read_text())
    except (OSError, ValueError):
        return {}


def human_session(home, env, app):
    global ORACLE
    """A tester's session, with the real pointer and keyboard: every control
    found by its name and clicked where it is on screen, every result read
    from what dm wrote to disk. Returns the dm left running at the end."""
    files = SlowFiles()
    try:
        x("xdotool", "windowactivate", "--sync", dm_windows()[0])

        print("\na download by hand: Add URL, type, OK, Start now, Stop, Resume")
        url = files.url("by-hand.bin")
        check("the Add URL button, clicked, opens New download",
              click("push button", "Add URL") and click_field("Address"))
        type_text(url)
        check("its OK, clicked after typing the address, opens Download details",
              click("push button", "OK") and wait_for(lambda: where("push button", "Start now"), 10))
        check("Start now, clicked, starts the download",
              click("push button", "Start now") and wait_for(lambda: state_of(home, url) == "running", 20),
              str(state_of(home, url)))
        time.sleep(3)
        name = (row_of(home, url) or {}).get("filename") or "by-hand.bin"
        check("the row is selected by clicking it", click_row(url, seconds=5)
              and wait_for(lambda: ORACLE and ORACLE("ui.inspect").get("selected"), 5))
        check("Stop, clicked on the selected row, stops it with its parts kept",
              click("push button", "Stop") and wait_for(lambda: state_of(home, url) == "paused", 15),
              str(state_of(home, url)))
        check("Resume, clicked, carries on", click("push button", "Resume")
              and wait_for(lambda: state_of(home, url) in ("running", "completed"), 15), str(state_of(home, url)))
        done = finished(home, url, 120)
        path = pathlib.Path((row_of(home, url) or {}).get("path", "/nonexistent"))
        check("and the file lands whole and byte-exact", done and path.exists() and path.read_bytes() == files.body,
              f"{state_of(home, url)} at {path}")

        print("\nthe progress window and the completion dialog (turned on in Options)")
        key("ctrl+comma")
        # Both switches are on the General tab, which Options opens on.
        check("Options has a switch for the progress window",
              click("*", "Show a progress window when a download starts"))
        check("and one for the completion window",
              click("*", "Show a window when a download finishes"))
        click("push button", "OK")
        time.sleep(0.5)
        prog = files.url("progress-watch.bin")
        add_url_by_hand(prog)
        check("a progress window opens for the download, naming it",
              wait_for(lambda: where("*", "Download progress", True), 10))
        check("it shows the download's address and connections",
              wait_for(lambda: where("*", "progress-watch.bin", True), 5))
        # Not finished() here: that would close the completion dialog before
        # the check sees it.
        done = wait_for(lambda: state_of(home, prog) == "completed", 60)
        saw_complete = done and wait_for(lambda: where("*", "Download complete"), 8)
        check("a completion dialog appears when it finishes", saw_complete)
        settle()
        check("and it closes", not where("*", "Download complete"))
        # Back off, so the rest of the session is not covered by pop-ups.
        key("ctrl+comma")
        click("*", "Show a progress window when a download starts")
        click("*", "Show a window when a download finishes")
        click("push button", "OK")

        print("\nmistakes and the keyboard")
        key("ctrl+n")
        check("Ctrl+N opens New download", wait_for(lambda: where("push button", "OK"), 5))
        click_field("Address")
        type_text("this is not a link")
        before = len(queue_rows(home))
        click("push button", "OK")
        time.sleep(1.5)
        check("a non-link, OK'd, is refused: the dialog stays and nothing is queued",
              where("push button", "OK") and len(queue_rows(home)) == before)
        # Once: the Escape that takes the keyboard out of the address
        # field closes the dialog, as it does in any dialog.
        key("Escape")
        time.sleep(1)
        check("Escape closes the dialog", not where("push button", "OK"))
        if where("push button", "OK"):
            click("push button", "Cancel")

        print("\nadd for later, then start the queue")
        later = files.url("later-small.bin")
        add_url_by_hand(later, then="Add for later")
        # Added for later, the row waits as a stopped one until Start queue.
        check("Add for later adds it without starting",
              wait_for(lambda: state_of(home, later) == "paused", 10), str(state_of(home, later)))
        time.sleep(2)
        check("and it waits", state_of(home, later) == "paused", str(state_of(home, later)))
        check("Start queue, clicked, downloads it",
              click("push button", "Start queue") and finished(home, later, 60), str(state_of(home, later)))

        print("\na download that fails, and Retry")
        broken = files.url("broken.bin")
        add_url_by_hand(broken)
        failed = wait_for(lambda: state_of(home, broken) == "failed", 90)
        reason = ((row_of(home, broken) or {}).get("state") or {}).get("reason", "")
        check("a server answering 500 fails the row, with a reason", failed and bool(reason), f"{state_of(home, broken)}: {reason}")
        files.broken = False
        click_row(broken, seconds=5)
        # A failed row's Resume says Retry, as a person reads it.
        check("Retry on the failed row retries, and it completes",
              click("push button", "Retry") and finished(home, broken, 60), str(state_of(home, broken)))

        print("\nright-click Properties, then stop and delete with its question")
        slow = files.url("slow-delete.bin")
        add_url_by_hand(slow)
        wait_for(lambda: state_of(home, slow) == "running", 20)
        time.sleep(2)
        row_name = (row_of(home, slow) or {}).get("filename") or "slow-delete.bin"
        opened = click_row(slow, button="3", seconds=10)
        check("a right-click on a row opens its menu, with Properties",
              opened and wait_for(lambda: where("*", "Properties"), 5))
        check("Properties, clicked, shows the file's address",
              click("*", "Properties") and wait_for(lambda: where("text", "Address") or where("entry", "Address"), 10))
        key("Escape")
        time.sleep(0.5)
        # A running download cannot be deleted until it is stopped: Delete,
        # clicked on it, does nothing (dm keeps an active download safe).
        click_row(slow, seconds=5)
        click("push button", "Delete")
        time.sleep(1)
        check("Delete on a running download is refused until it is stopped",
              not where("push button", "Yes, delete") and row_of(home, slow) is not None)
        # Stop it, then Delete asks and removes it with its parts.
        parts = home / "parts"
        check("Stop, clicked, stops it", click("push button", "Stop")
              and wait_for(lambda: state_of(home, slow) == "paused", 15), str(state_of(home, slow)))
        click_row(slow, seconds=5)
        check("Delete on the stopped download asks first",
              click("push button", "Delete") and wait_for(lambda: where("push button", "Yes, delete"), 5))
        check("Yes, delete removes the row and its parts",
              click("push button", "Yes, delete") and wait_for(lambda: row_of(home, slow) is None, 10)
              and not (parts.exists() and any(p.name.startswith("part-") for p in parts.rglob("*") if p.is_file())))

        print("\nthe Downloads menu, and the speed limiter")
        check("the Downloads menu opens", click("*", "Downloads") and wait_for(lambda: where("*", "Speed limiter", True), 5))
        at = where("*", "Speed limiter", True)
        if at:
            x("xdotool", "mousemove", "--sync", str(at[0]), str(at[1]))
            time.sleep(0.8)
        check("its Speed limiter submenu opens on hover",
              wait_for(lambda: where("*", "Speed limiter settings", True), 5))
        check("Speed limiter settings opens the dialog",
              click("*", "Speed limiter settings", contains=True) and click_field("Maximum, KB/s across every download (0 for none)"))
        type_text("256")
        key("Return")
        click("push button", "Apply")
        check("a limit typed and applied is written down",
              wait_for(lambda: settings_of(home).get("speed_limit") == 256, 10), str(settings_of(home).get("speed_limit")))
        key("Escape")

        print("\nfinding, the places on the rail, clearing")
        key("ctrl+f")
        check("Ctrl+F opens Find", click_field("Find what"))
        type_text("by-hand")
        click("push button", "Find next")
        time.sleep(0.5)
        key("Escape")
        # The rail's places carry their counts: "Done (3)".
        check("Done, clicked on the rail, shows the finished rows only",
              click("*", "Done", contains=True) and wait_for(lambda: where("*", "by-hand.bin"), 5)
              and not where("*", "one.zip"))
        check("Everything shows them all again", click("*", "Everything") and wait_for(lambda: where("*", "by-hand.bin"), 5))
        finished_before = len([r for r in queue_rows(home) if (r.get("state") or {}).get("kind") == "completed"])
        check("Clear finished, clicked, removes the finished rows",
              finished_before > 0 and click("push button", "Clear finished")
              and wait_for(lambda: not [r for r in queue_rows(home) if (r.get("state") or {}).get("kind") == "completed"], 10),
              f"{finished_before} finished before")
        check("and leaves the files on disk", path.exists())

        print("\nOptions: the download folder, typed, is where the next file lands")
        elsewhere = home / "elsewhere"
        key("ctrl+comma")
        check("Ctrl+, opens Options", wait_for(lambda: where("*", "Folders"), 5))
        click("*", "Folders")
        check("its Folders tab has the download folder", click_field("Download folder"))
        type_text(str(elsewhere))
        key("Tab")
        click("push button", "OK")
        check("OK writes it down", wait_for(lambda: settings_of(home).get("download_dir") == str(elsewhere), 10),
              str(settings_of(home).get("download_dir")))
        moved = files.url("moved-small.bin")
        add_url_by_hand(moved)
        finished(home, moved, 60)
        landed = pathlib.Path((row_of(home, moved) or {}).get("path", "/nonexistent"))
        check("and the next download lands there", landed.exists() and str(landed).startswith(str(elsewhere)), str(landed))

        print("\nthe clipboard, watched")
        link = files.url("clip-small.bin")
        # xclip stays to serve the selection; its pipes are let go of.
        settle()
        x("sh", "-c", f"printf '%s' '{link}' | xclip -selection clipboard >/dev/null 2>&1 &")
        check("a link copied to the clipboard is offered in the window",
              wait_for(lambda: where("push button", "Dismiss"), 15))
        check("its Download button opens Download details for it",
              click("push button", "Download") and wait_for(lambda: where("push button", "Start now"), 10))
        key("Escape")

        print("\nthe window, and leaving")
        window = dm_windows()[0]
        x("xdotool", "windowsize", "--sync", window, "1000", "700")
        time.sleep(1.5)
        check("Program › Exit, clicked, quits dm",
              click("*", "Program") and click("*", "Exit", contains=True) and wait_for(lambda: app.poll() is not None, 15))
        app = subprocess.Popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        window = wait_for(dm_windows, 30)
        ORACLE = oracle(home)
        size = x("xdotool", "getwindowgeometry", "--shell", window[0]).stdout if window else ""
        w = h = 0
        for line in size.split():
            if line.startswith("WIDTH="):
                w = int(line[6:])
            if line.startswith("HEIGHT="):
                h = int(line[7:])
        check("opened again, the window is the size it was left", abs(w - 1000) <= 4 and abs(h - 700) <= 4, f"{w}x{h}")
        x("xdotool", "windowactivate", "--sync", window[0]) if window else None
        key("ctrl+q")
        check("Ctrl+Q quits", wait_for(lambda: app.poll() is not None, 15))
        app = subprocess.Popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        wait_for(dm_windows, 30)
        ORACLE = oracle(home)
        return app
    finally:
        files.server.shutdown()


def tray_host_timing(env):
    """A tray host that is not there, or comes after dm: at login dm can
    start before the panel, and GNOME without its extension has none."""
    print("\na tray host that is missing, or late")
    # Anchored: the shell that started the watcher has its name in its own
    # command line, and ending that ends the session.
    x("pkill", "-f", "^status-notifier-watcher")
    wait_for(lambda: not x("pgrep", "-f", "^status-notifier-watcher").stdout.strip(), 10)

    app = subprocess.Popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("with no tray host dm still opens", wait_for(dm_windows, 30))
    x("wmctrl", "-i", "-c", dm_windows()[0])
    check("and closing the window quits, having no tray to hide into",
          wait_for(lambda: app.poll() is not None, 15))

    app = subprocess.Popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wait_for(dm_windows, 30)
    time.sleep(2)
    watcher = subprocess.Popen(["status-notifier-watcher"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("a tray host that comes later gets dm's item", wait_for(tray_icon, 20), "nothing registered")
    x("wmctrl", "-i", "-c", dm_windows()[0])
    check("and closing then hides it into that tray",
          wait_for(lambda: not dm_windows(), 10) and app.poll() is None)
    # The panel restarts (waybar is often reloaded): its new watcher can
    # drop the registration that arrives as it starts.
    watcher.terminate()
    watcher.wait(10)
    watcher = subprocess.Popen(["status-notifier-watcher"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("a tray host that restarts gets dm's item back", wait_for(tray_icon, 20), "nothing registered")
    tray_menu(QUIT)
    wait_for(lambda: app.poll() is not None, 15)

    watcher.terminate()
    watcher.wait(10)
    hidden = subprocess.Popen([str(BINARY), "--hidden"], env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    time.sleep(3)
    check("a login launch with no tray host waits hidden at first", not dm_windows())
    check("then opens its window rather than run unseen", wait_for(dm_windows, 20))
    # Left running with a host again, for whatever runs next.
    subprocess.Popen(["status-notifier-watcher"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return hidden


def start_on_login(home, env):
    """Start on login, as a desktop session does it: dm writes its XDG
    autostart entry, and `dex` starts what is in the autostart folder the
    way GNOME, KDE, Xfce and the rest do at login. Returns the dm it
    started, for the caller to end."""
    print("\nstart on login")
    config = home / "config"
    env = dict(env, XDG_CONFIG_HOME=str(config), XDG_CONFIG_DIRS=str(home / "no-system-autostart"))
    settings = home / "settings.json"
    saved = json.loads(settings.read_text())
    settings.write_text(json.dumps(dict(saved, start_on_login=True)))
    entry = config / "autostart" / "dm.desktop"

    # Turned on: the next launch writes the entry, aimed at this dm.
    first = subprocess.Popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    written = wait_for(entry.exists, 15)
    text = entry.read_text() if written else ""
    check("turning it on writes an autostart entry", written)
    check("which starts this dm into the tray", f"Exec={BINARY} --hidden\n" in text, text)
    first.terminate()
    first.wait(10)

    # The login: the session starts the entry.
    # Not captured: dm inherits whatever dex is given, and a pipe it holds
    # open would never reach its end.
    started = subprocess.run(["dex", "--autostart", "--environment", "Openbox"], env=env,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
    pid = lambda: x("pgrep", "-f", f"^{BINARY} --hidden").stdout.split()
    running = wait_for(pid, 15)
    check("at login the session starts dm", running and started.returncode == 0,
          f"dex exited {started.returncode}")
    time.sleep(3)
    check("and it opens no window", not dm_windows())
    check("but waits in the tray", wait_for(tray_icon, 20), "nothing registered")
    tray_menu(SHOW)
    check("whose Show dm opens the window", wait_for(dm_windows, 10))
    tray_menu(QUIT)
    wait_for(lambda: not pid(), 15)

    # Turned off: the next launch removes it.
    settings.write_text(json.dumps(dict(saved, start_on_login=False)))
    last = subprocess.Popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("turning it off removes the entry", wait_for(lambda: not entry.exists(), 15))
    return last


def main():
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-gui-"))
    (home / "downloads").mkdir()
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(home / "downloads"), "speed_limit": 0, "watch_clipboard": True,
        "show_progress": False, "show_complete": False,
    }))
    env = dict(os.environ, DM_HOME=str(home))
    screen_reader_on()
    app = subprocess.Popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        print("the window and the tray, with real input")
        check("dm opens its window", wait_for(dm_windows, 30))
        global ORACLE
        ORACLE = oracle(home)

        orca_log = os.environ.get("DM_ORCA_LOG")
        if not orca_log:
            # Read the way a screen reader reads; with Orca running, Orca
            # itself is the reader, below.
            heard = wait_for(spoken, 20)
            frames, buttons = heard or ([], [])
            check("a screen reader finds the window, named dm", "dm" in frames, str(frames))
            check("and every button in it has a name", buttons and all(b.strip() for b in buttons),
                  f"{len([b for b in buttons if not b.strip()])} of {len(buttons)} unnamed")
            check("among them Add URL", "Add URL" in buttons, str(buttons[:20]))
        else:
            # Orca, the screen reader itself, is running: Tab through the
            # window and hear what it says.
            window = dm_windows()[0]
            x("xdotool", "windowactivate", "--sync", window)
            for _ in range(2):
                time.sleep(1)
                x("xdotool", "key", "Tab")
            def said():
                log = pathlib.Path(orca_log)
                text = log.read_text(errors="replace") if log.exists() else ""
                return [line.split("SPEECH OUTPUT: ", 1)[1].split("{")[0].strip()
                        for line in text.splitlines() if "SPEECH OUTPUT: " in line]
            heard = wait_for(lambda: [s for s in said() if "push button" in s], 15) and said()
            check("Orca names the window: dm frame", any("dm frame" in s for s in heard or []), str(heard))
            check("and says each button as Tab reaches it",
                  any(s.endswith("push button.'") for s in heard or []), str(heard))
        check("and puts its item in the tray, with its menu", wait_for(tray_icon, 20), "nothing registered")

        # Closed through the window manager, as a person closes it.
        window = dm_windows()[0]
        # Asked of openbox the way its close button asks (_NET_CLOSE_WINDOW),
        # not with a synthetic Alt+F4: after that chord openbox now and then
        # kept the keyboard grabbed, and a tray menu cannot open under
        # another client's grab (gap 21).
        closed = x("wmctrl", "-i", "-c", window)
        gone = closed.returncode == 0 and wait_for(lambda: not dm_windows(), 10)
        check("closing the window hides it into the tray, and dm keeps running",
              gone and app.poll() is None, closed.stderr.strip())

        tray_menu(SHOW)
        check("the tray menu's Show dm brings the window back", wait_for(dm_windows, 10))

        # A left click on the icon: the panel calls Activate on the item.
        x("wmctrl", "-i", "-c", dm_windows()[0])
        wait_for(lambda: not dm_windows(), 10)
        item = tray_item()
        if item:
            gdbus("--dest", item[0], "--object-path", item[3], "--method",
                  "org.kde.StatusNotifierItem.Activate", "0", "0")
        check("a left click on the tray icon brings the window back", wait_for(dm_windows, 10))

        tray_menu(SPEED_LIMITER)
        settings = lambda: json.loads((home / "settings.json").read_text())
        limited = wait_for(lambda: settings().get("speed_limit", 0) > 0, 10)
        check("the tray menu's Speed limiter turns the limiter on", limited, str(settings().get("speed_limit")))
        # And off again: left on, it held every later download of the run
        # to the limit and the completion scene timed out.
        tray_menu(SPEED_LIMITER)
        check("and again turns it off",
              wait_for(lambda: settings().get("speed_limit", 0) == 0, 10), str(settings().get("speed_limit")))

        # A real drag and drop: a file of links dragged from a GTK window.
        links = home / "links.txt"
        links.write_text("http://127.0.0.1:9/one.zip\nhttp://127.0.0.1:9/two.zip\n")
        source = subprocess.Popen([sys.executable, str(pathlib.Path(__file__).with_name("drag_source.py")),
                                   str(links)])
        src = wait_for(lambda: x("xdotool", "search", "--name", "^drag me$").stdout.split(), 15)
        target = dm_windows()
        if src and target:
            geo = lambda w: {k: int(v) for k, v in (
                line.split("=") for line in x("xdotool", "getwindowgeometry", "--shell", w).stdout.split()
                if "=" in line and line.split("=")[0] in ("X", "Y", "WIDTH", "HEIGHT"))}
            s, t = geo(src[0]), geo(target[0])
            sx, sy = s["X"] + s["WIDTH"] // 2, s["Y"] + s["HEIGHT"] // 2
            tx, ty = t["X"] + t["WIDTH"] // 2, t["Y"] + t["HEIGHT"] // 2
            x("xdotool", "mousemove", str(sx), str(sy))
            x("xdotool", "mousedown", "1")
            for step in range(1, 21):
                x("xdotool", "mousemove", str(sx + (tx - sx) * step // 20), str(sy + (ty - sy) * step // 20))
                time.sleep(0.05)
            time.sleep(0.5)
            x("xdotool", "mouseup", "1")
        rows = lambda: json.loads((home / "queue.json").read_text()) if (home / "queue.json").exists() else []
        added = wait_for(lambda: len([r for r in rows() if r["url"].endswith((".zip"))]) >= 2, 15)
        check("a file of links dropped on the window is read and its links queued",
              added, f"{[r['url'] for r in rows()]}")
        source.terminate()

        app = human_session(home, env, app)

        tray_menu(QUIT)
        quit_ = wait_for(lambda: app.poll() is not None, 15)
        check("the tray menu's Quit dm ends dm", quit_ is not None and quit_ is not False)

        app = start_on_login(home, env)
        if app.poll() is None:
            app.terminate()
            app.wait(10)
        app = tray_host_timing(env)
    finally:
        if app.poll() is None:
            app.terminate()
            try:
                app.wait(10)
            except subprocess.TimeoutExpired:
                app.kill()
    print()
    failed = RESULTS.count(False)
    print(f"{len(RESULTS) - failed} of {len(RESULTS)} real-input checks passed")
    sys.exit(1 if failed else 0)


main()
