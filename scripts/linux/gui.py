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



def dm_popen(argv, **kw):
    """dm started in a process group of its own, so `end` stops all of it:
    an AppImage runs dm as its wrapper's child, and a signal to the wrapper
    alone left that dm running, which later launches then handed over to."""
    return subprocess.Popen(argv, start_new_session=True, **kw)


def end(app):
    """Stops a dm and everything in its group, and waits for it."""
    if app.poll() is not None:
        return
    import signal
    try:
        os.killpg(app.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        app.wait(10)
    except subprocess.TimeoutExpired:
        os.killpg(app.pid, signal.SIGKILL)
        app.wait(5)
    time.sleep(1)

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
    """Where dm's window really is on screen: its client area's absolute
    corner and size, from the X server. `xdotool getwindowgeometry` gives the
    frame's corner under some window managers, one title bar too high, which
    sent every click computed from a row's rectangle one row low."""
    w = dm_windows()
    if not w:
        return None
    out = x("xwininfo", "-id", w[0]).stdout
    g = {}
    for label, field in (("Absolute upper-left X:", "X"), ("Absolute upper-left Y:", "Y"),
                         ("Width:", "WIDTH"), ("Height:", "HEIGHT")):
        m = re.search(re.escape(label) + r"\s+(-?\d+)", out)
        if m:
            g[field] = int(m.group(1))
    return g if {"X", "Y", "WIDTH", "HEIGHT"} <= g.keys() else None


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


def tray_tooltip():
    """dm's tray item's ToolTip property, as a panel reads it: (icon name,
    pixmaps, title, description)."""
    item = tray_item()
    if not item:
        return ""
    name, _, _, path = item
    return gdbus("--dest", name, "--object-path", path, "--method",
                 "org.freedesktop.DBus.Properties.Get", "org.kde.StatusNotifierItem", "ToolTip")


def tray_tooltip_said():
    """Its title is dm and its words say the downloads go on: a panel that
    shows only the title shows dm, and one that shows more says why."""
    said = tray_tooltip()
    return "'dm'" in said and "downloads keep running here" in said


# dm's tray menu, by label, as a panel shows it.
SHOW, SPEED_LIMITER, SCHEDULER, QUIT = "Show dm", "Speed limiter", "Scheduler…", "Exit dm"


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
                            # "slow" ones a quarter as fast: the Stop and
                            # Delete scene takes longer than 48 MiB at the
                            # usual rate, and found it already finished.
                            # Half the old pace for every large file: Stop
                            # and the progress window's scenes now and then
                            # found theirs already finished.
                            time.sleep(0.256 if "slow" in self.path else 0.128)
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


def type_text(text, delay=15):
    x("xdotool", "type", "--delay", str(delay), text)


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


def finished_files_by_hand(home, base, site, good, evil):
    """What else a person does with finished downloads: the file's SHA-256 and
    a digest checked in Properties, several files named by a pattern, and a
    folder dm watches for links."""
    import hashlib

    print("\nthe digest, a pattern, and a watched folder")
    path = pathlib.Path((row_of(home, good) or {}).get("path", "/nonexistent"))
    expected = hashlib.sha256(path.read_bytes()).hexdigest()
    good_id = (row_of(home, good) or {}).get("id")
    check("the zip's row is selected by clicking it", click_row("good.zip")
          and bool(wait_for(lambda: (ORACLE("ui.inspect") or {}).get("selected") == [good_id], 5)),
          str((ORACLE("ui.inspect") or {}).get("selected")))
    check("right-clicking it offers Properties", click_row("good.zip", button="3") and bool(wait_for(lambda: where("*", "Properties"), 5)))
    click("*", "Properties", seconds=5)
    check("File properties shows its SHA-256, worked out beside the window",
          bool(wait_for(lambda: where("*", expected), 15)), expected[:16])
    check("and a box to check it against a digest", click_field("Check against"))
    type_text(expected.upper(), delay=5)
    check("a digest it has, in capitals, matches", bool(wait_for(lambda: where("*", "It matches."), 8)))
    key("ctrl+a")
    type_text("0" * 64, delay=5)
    check("a digest it does not have is said not to match", bool(wait_for(lambda: where("*", "It does not match."), 8)))
    key("Escape")
    time.sleep(0.5)

    # Two rows selected, named by a pattern.
    check("the first zip is selected", click_row("good.zip"))
    at = wait_for(lambda: row_point("evil.zip"), 5)
    x("xdotool", "mousemove", "--sync", str(at[0]), str(at[1]))
    x("xdotool", "keydown", "ctrl")
    time.sleep(0.2)
    x("xdotool", "click", "1")
    time.sleep(0.2)
    x("xdotool", "keyup", "ctrl")
    time.sleep(0.5)
    check("and a second added with Ctrl-click", len((ORACLE("ui.inspect") or {}).get("selected", [])) == 2,
          str((ORACLE("ui.inspect") or {}).get("selected")))
    click_row("evil.zip", button="3")
    check("Move/Rename is in the row menu", click("*", "Move/Rename", seconds=5))
    check("for several files it asks for a name pattern", click_field("Name pattern"))
    type_text("Archive {n} ({name})", delay=40)
    click("push button", "OK")
    check("OK names them in order and keeps their endings",
          wait_for(lambda: ((row_of(home, good) or {}).get("filename"), (row_of(home, evil) or {}).get("filename"))
                   == ("Archive 1 (good).zip", "Archive 2 (evil).zip"), 15),
          str(((row_of(home, good) or {}).get("filename"), (row_of(home, evil) or {}).get("filename"))))
    renamed = pathlib.Path((row_of(home, good) or {}).get("path", "/nonexistent"))
    check("and the files on disk are renamed with them", renamed.name == "Archive 1 (good).zip" and renamed.exists(), str(renamed))

    # A folder to watch, set in Options and used at once.
    watched = home / "watched-by-hand"
    watched.mkdir(exist_ok=True)
    label = 'Watch a folder for links: a .url, .webloc or text file of links, or a .torrent saved there is added and moved to its "done" folder (empty is off)'
    key("ctrl+comma")
    wait_for(lambda: where("*", "General"), 10)
    check("Options › Downloads has the watched folder's field",
          open_options_tab("Downloads", label) and (scroll_dialog() or True) and click_field(label))
    type_text(str(watched), delay=20)
    key("Tab")
    click("push button", "OK")
    check("OK saves the folder", wait_for(lambda: settings_of(home).get("watch_folder") == str(watched), 10),
          str(settings_of(home).get("watch_folder")))
    link = f"{base}/from-folder.zip"
    saved = watched / "saved-link.url"
    saved.write_text(f"[InternetShortcut]\nURL={link}\n")
    old = time.time() - 60
    os.utime(saved, (old, old))
    check("a link saved there appears in dm's list and downloads", wait_for(lambda: state_of(home, link) == "completed", 60),
          str(state_of(home, link)))
    check("and its file is put aside in done", (watched / "done" / "saved-link.url").exists() and not saved.exists())
    settle()


def extract_and_command_line(home, env):
    """Extract here, clicked on a real zip's row with the real pointer, and
    the command line against this same dm: what a tester would do after
    downloading an archive."""
    import http.server
    import threading
    import zipfile

    print("\nextracting a downloaded zip, and the command line")
    site = pathlib.Path(tempfile.mkdtemp(prefix="dm-zips-"))
    with zipfile.ZipFile(site / "good.zip", "w") as z:
        z.writestr("readme.txt", "hello from the archive")
        z.writestr("docs/deep/note.txt", "nested")
    (site / "from-folder.zip").write_bytes(b"saved in a watched folder " * 400)
    with zipfile.ZipFile(site / "evil.zip", "w") as z:
        z.writestr("fine.txt", "fine")
        z.writestr("../escaped.txt", "must never land outside")
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(site), **k)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{server.server_address[1]}"
    try:
        good = f"{base}/good.zip"
        check("a zip is downloaded through the window", add_url_by_hand(good) and finished(home, good, 60))
        path = pathlib.Path((row_of(home, good) or {}).get("path", "/nonexistent"))
        check("and is on disk", path.exists(), str(path))

        check("right-clicking its row opens the row menu", click_row("good.zip", button="3", seconds=10))
        check("the menu offers Extract here", bool(wait_for(lambda: where("*", "Extract here"), 5)))
        check("and Extract to…", bool(where("*", "Extract to…")))
        check("Extract here, clicked, is picked", click("*", "Extract here", seconds=5))
        folder = path.with_suffix("")
        check("the files land in a folder beside the archive",
              wait_for(lambda: (folder / "docs" / "deep" / "note.txt").exists(), 15), str(folder))
        check("with their contents", (folder / "readme.txt").read_text() == "hello from the archive"
              if (folder / "readme.txt").exists() else False)
        check("the archive is still there", path.exists())
        said = (ORACLE("ui.inspect") or {}).get("said", "")
        check("the status line says what was done", said.startswith("Extracted 2 files"), said)

        # Again: a second folder beside the first, nothing replaced.
        click_row("good.zip", button="3")
        click("*", "Extract here", seconds=5)
        check("extracting again makes a second folder and replaces nothing",
              wait_for(lambda: (path.parent / (folder.name + " (1)") / "readme.txt").exists(), 15))

        # Share on this network, on the same finished row: clicked as a
        # person does, then fetched from outside the app with curl the way
        # a neighbour's download manager would, and stopped.
        click_row("good.zip", button="3")
        check("the row menu offers Share on this network", bool(wait_for(lambda: where("*", "Share on this network"), 5)))
        click("*", "Share on this network", seconds=5)
        said = wait_for(lambda: (ORACLE("ui.inspect") or {}).get("said", "").startswith(("Network link copied", "Not shared"))
                        and (ORACLE("ui.inspect") or {}).get("said"), 15)
        check("the status line gives a network link", bool(said) and said.startswith("Network link copied.") and "http://" in said, str(said))
        link = str(said).rsplit(" ", 1)[-1].strip()
        pasted = subprocess.run(["xclip", "-o", "-selection", "clipboard"], capture_output=True, text=True).stdout.strip()
        check("and the same link is on the clipboard", pasted == link, pasted)
        check("the link is not the loopback, which only this computer reaches", "://127." not in link and "://localhost" not in link, link)
        got = subprocess.run(["curl", "-sS", "--max-time", "20", "-o", "/tmp/shared-copy.zip", "-w", "%{http_code}", link], capture_output=True, text=True).stdout
        check("another program fetches the file from the link", got == "200" and open("/tmp/shared-copy.zip", "rb").read() == path.read_bytes(), got)
        part = subprocess.run(["curl", "-sS", "--max-time", "20", "-r", "0-9", "-w", "|%{http_code}", link], capture_output=True).stdout
        check("and a range of it, which is what lets a download manager use several connections",
              part == path.read_bytes()[:10] + b"|206", repr(part))
        wrong = subprocess.run(["curl", "-sS", "--max-time", "20", "-o", "/dev/null", "-w", "%{http_code}", link.rsplit("/", 2)[0] + "/00000000000000000000000000000000/good.zip"], capture_output=True, text=True).stdout
        check("a link with another token gets nothing", wrong == "404", wrong)
        click_row("good.zip", button="3")
        check("while sharing, the row menu offers Stop sharing", bool(wait_for(lambda: where("*", "Stop sharing"), 5)))
        click("*", "Stop sharing", seconds=5)
        gone = wait_for(lambda: subprocess.run(["curl", "-sS", "--max-time", "5", "-o", "/dev/null", "-w", "%{http_code}", link], capture_output=True, text=True).stdout == "000", 10)
        check("after Stop sharing the link is dead", bool(gone))
        click_row("good.zip", button="3")
        check("and the row menu no longer offers Stop sharing", not where("*", "Stop sharing"))
        x("xdotool", "key", "Escape")

        # A hostile archive: refused whole, nothing written anywhere.
        evil = f"{base}/evil.zip"
        add_url_by_hand(evil)
        finished(home, evil, 60)
        epath = pathlib.Path((row_of(home, evil) or {}).get("path", "/nonexistent"))
        click_row("evil.zip", button="3")
        click("*", "Extract here", seconds=5)
        said = wait_for(lambda: (ORACLE("ui.inspect") or {}).get("said", "").startswith("Not extracted") and
                        (ORACLE("ui.inspect") or {}).get("said"), 15)
        check("a zip with an entry that leaves its folder is refused, and says so",
              bool(said) and "leave its folder" in said, str(said))
        check("no folder is made for it and nothing escaped",
              not epath.with_suffix("").exists() and not list(home.rglob("escaped.txt"))
              and not list(pathlib.Path(tempfile.gettempdir()).glob("escaped.txt")))

        # The command line, against this dm (started with --control 0).
        def cli(*args):
            return subprocess.run([str(BINARY), *args], env=env, capture_output=True, text=True, timeout=30)
        listed = cli("list")
        check("`dm-desktop list` shows the downloads, one a line",
              listed.returncode == 0 and "good.zip" in listed.stdout and "completed" in listed.stdout,
              listed.stdout + listed.stderr)
        table = json.loads(cli("list", "--json").stdout or "[]")
        row = next((r for r in table if r["name"] == "good.zip"), {})
        check("`list --json` has the id, state and size", row.get("state") == "completed" and row.get("total"), str(row))
        check("and nothing private: no cookies, referrer or note",
              not {"cookies", "referrer", "description"} & set(row))
        check("`status` says the same for one", json.loads(cli("status", str(row.get("id")), "--json").stdout or "[{}]")[0].get("name") == "good.zip")
        refused = cli("pause", str(row.get("id")))
        check("pausing a finished download exits 1 and says why",
              refused.returncode == 1 and "not running" in refused.stderr, refused.stderr)
        check("a number that is not there exits 1", cli("status", "999999").returncode == 1)
        check("a wrong command exits 2", cli("pause").returncode == 2)
        finished_files_by_hand(home, base, site, good, evil)
    finally:
        server.shutdown()


def open_options_tab(label, expect):
    """A tab of the open Options dialog, clicked by its name. The same word may
    also be a menu behind the dialog, which takes no clicks, so what the tab
    might be is tried until `expect`, a control of that tab, shows."""
    for role in ("page tab", "radio button", "toggle button", "push button", "*"):
        if click(role, label, seconds=2) and wait_for(lambda: where("*", expect), 3):
            return True
    return False


def scroll_dialog():
    """The wheel over the dialog, as a person scrolls a long tab."""
    geo = win_geo()
    if geo:
        x("xdotool", "mousemove", "--sync", str(geo["X"] + geo["WIDTH"] // 2), str(geo["Y"] + geo["HEIGHT"] // 2))
        x("xdotool", "click", "--repeat", "12", "--delay", "40", "5")
        time.sleep(0.5)


def options_by_hand(home):
    """Options, as a person goes through it: every tab opened by clicking its
    name, a control of that tab found, then one value set, saved with OK and
    read back from the settings file."""
    print("\nOptions, tab by tab, with the real pointer")
    recording = "Stop recording after (minutes, 0 for no limit)"
    tabs = [("General", "Language"), ("Browser", "File types"), ("Folders", "Download folder"),
            ("Downloads", recording), ("Network", "Speed limit, KB/s (0 for none)"),
            ("Proxy", "Proxy type"), ("Sign-ins", "Add a login")]
    key("ctrl+comma")
    check("Ctrl+, opens Options", bool(wait_for(lambda: where("*", "General"), 10)))

    for label, expect in tabs:
        check(f"the {label} tab, clicked, shows its settings", open_options_tab(label, expect), expect)

    check("on Downloads, the recording limit's box is scrolled to and clicked", open_options_tab("Downloads", recording)
          and (scroll_dialog() or True) and click_field(recording))
    type_text("45", delay=120)
    key("Tab")
    click("push button", "OK")
    check("OK saves it: the settings file holds 45 minutes",
          wait_for(lambda: settings_of(home).get("record_minutes") == 45, 10), str(settings_of(home).get("record_minutes")))

    key("ctrl+comma")
    wait_for(lambda: where("*", "General"), 10)
    check("opened again, the value is still there",
          open_options_tab("Downloads", recording) and (scroll_dialog() or True) and bool(field_at(recording)))
    click_field(recording)
    type_text("0", delay=120)
    key("Return")
    click("push button", "OK")
    check("and set back to none, saved",
          wait_for(lambda: settings_of(home).get("record_minutes") == 0, 10), str(settings_of(home).get("record_minutes")))


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

        extract_and_command_line(home, env)
        settle()
        options_by_hand(home)
        settle()

        print("\nthe progress window and the completion dialog (turned on in Options)")
        key("ctrl+comma")
        # Both switches are on the General tab, which Options opens on.
        check("Options has a switch for the progress window",
              click("*", "Show progress when a download starts"))
        check("and one for the completion window",
              click("*", "Show a message when a download finishes"))
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
        click("*", "Show progress when a download starts")
        click("*", "Show a message when a download finishes")
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
        click("push button", "Remove")
        time.sleep(1)
        check("Delete on a running download is refused until it is stopped",
              not where("push button", "Yes, remove") and row_of(home, slow) is not None)
        # Stop it, then Delete asks and removes it with its parts.
        parts = home / "parts"
        check("Stop, clicked, stops it", click("push button", "Stop")
              and wait_for(lambda: state_of(home, slow) == "paused", 15), str(state_of(home, slow)))
        click_row(slow, seconds=5)
        check("Delete on the stopped download asks first",
              click("push button", "Remove") and wait_for(lambda: where("push button", "Yes, remove"), 5))
        check("Yes, delete removes the row and its parts",
              click("push button", "Yes, remove") and wait_for(lambda: row_of(home, slow) is None, 10)
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
              click("*", "Speed limiter settings", contains=True) and click_field("Speed limit (KB/s, 0 for none)"))
        type_text("256")
        key("Return")
        click("push button", "OK")
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
        app = dm_popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
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
        app = dm_popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        wait_for(dm_windows, 30)
        ORACLE = oracle(home)
        return app
    finally:
        files.server.shutdown()


def panel():
    """A panel's tray part starting: the StatusNotifierWatcher and a host
    registered with it (scripts/linux/tray_host.py), as a panel brings both
    up together. Returns both, to be ended together."""
    watcher = subprocess.Popen(["status-notifier-watcher"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    host = subprocess.Popen([sys.executable, str(pathlib.Path(__file__).with_name("tray_host.py"))],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return [watcher, host]


def end_panel(parts):
    for part in parts:
        part.terminate()
        part.wait(10)


def tray_host_timing(env):
    """A tray host that is not there, or comes after dm: at login dm can
    start before the panel, and GNOME without its extension has none."""
    print("\na tray host that is missing, or late")
    # Anchored: the shell that started the watcher has its name in its own
    # command line, and ending that ends the session.
    x("pkill", "-f", "^status-notifier-watcher")
    # Anchored at a python as the watcher is: the session's shell has the
    # host's name in its own command line too, and ending it ends the run.
    x("pkill", "-f", "^[^ ]*python3?[^ ]* [^ ]*tray_host\\.py")
    wait_for(lambda: not x("pgrep", "-f", "^status-notifier-watcher").stdout.strip(), 10)

    app = dm_popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("with no tray host dm still opens", wait_for(dm_windows, 30))
    x("wmctrl", "-i", "-c", dm_windows()[0])
    check("and closing the window quits, having no tray to hide into",
          wait_for(lambda: app.poll() is not None, 15))

    app = dm_popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wait_for(dm_windows, 30)
    time.sleep(2)
    watcher = panel()
    check("a tray host that comes later gets dm's item", wait_for(tray_icon, 20), "nothing registered")
    check("and its tooltip names dm and says the downloads go on", tray_tooltip_said(),
          tray_tooltip())
    x("wmctrl", "-i", "-c", dm_windows()[0])
    check("and closing then hides it into that tray",
          wait_for(lambda: not dm_windows(), 10) and app.poll() is None)
    # The panel restarts (waybar is often reloaded): its new watcher can
    # drop the registration that arrives as it starts.
    end_panel(watcher)
    watcher = panel()
    check("a tray host that restarts gets dm's item back", wait_for(tray_icon, 20), "nothing registered")
    tray_menu(QUIT)
    wait_for(lambda: app.poll() is not None, 15)

    end_panel(watcher)
    hidden = dm_popen([str(BINARY), "--hidden"], env=env, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    time.sleep(3)
    check("a login launch with no tray host waits hidden at first", not dm_windows())
    check("then opens its window rather than run unseen", wait_for(dm_windows, 20))
    # Left running with a host again, for whatever runs next.
    panel()
    return hidden


def start_on_login(home, env):
    """Start on login, as a desktop session does it: dm writes its XDG
    autostart entry, and `dex` starts what is in the autostart folder the
    way GNOME, KDE, Xfce and the rest do at login. Returns the dm it
    started, for the caller to end."""
    print("\nstart on login")
    config = home / "config"
    # Without DM_HOME: a dm on a home of its own (a test's, a script's) leaves
    # the login item alone by design, so that every test's fresh home does not
    # register one on the computer running it. A real login launch has no
    # DM_HOME, and keeps its home under XDG_DATA_HOME.
    saved = json.loads((home / "settings.json").read_text())
    env = {k: v for k, v in env.items() if k != "DM_HOME"}
    env = dict(env, XDG_CONFIG_HOME=str(config), XDG_CONFIG_DIRS=str(home / "no-system-autostart"),
               XDG_DATA_HOME=str(home / "xdg-data"))
    own = home / "xdg-data" / "dm-engine"
    own.mkdir(parents=True, exist_ok=True)
    settings = own / "settings.json"
    settings.write_text(json.dumps(dict(saved, start_on_login=True)))
    entry = config / "autostart" / "dm.desktop"

    # Turned on: the next launch writes the entry, aimed at this dm.
    first = dm_popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    written = wait_for(entry.exists, 15)
    text = entry.read_text() if written else ""
    check("turning it on writes an autostart entry", written)
    check("which starts this dm into the tray", f"Exec={BINARY} --hidden\n" in text, text)
    end(first)

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
    last = dm_popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("turning it off removes the entry", wait_for(lambda: not entry.exists(), 15))
    return last


def main():
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-gui-"))
    (home / "downloads").mkdir()
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(home / "downloads"), "speed_limit": 0, "watch_clipboard": True,
        "show_progress": False, "show_complete": False,
        # Three attempts, not a person's ten: the failing download's scene
        # would otherwise sit through minutes of half-minute retries.
        "attempts": 3,
    }))
    env = dict(os.environ, DM_HOME=str(home))
    screen_reader_on()
    app = dm_popen([str(BINARY), "--control", "0"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
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
        # another client's grab.
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
        end(app)
        app = tray_host_timing(env)
    finally:
        end(app)
    print()
    failed = RESULTS.count(False)
    print(f"{len(RESULTS) - failed} of {len(RESULTS)} real-input checks passed")
    sys.exit(1 if failed else 0)


main()
