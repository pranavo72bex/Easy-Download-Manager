#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What a screen reader finds in dm's dialogs and its list on Windows (gaps rows 160, U23):

    python scripts/windows-a11y-check.py

Every dialog of dm's is a window of its own (contract row B42), and a screen
reader reads a window only through what the program gives the system's
accessibility interface for that window. This starts the real binary on a
throwaway home with its window shown, opens each dialog through the control
protocol, and asks Windows' UI Automation (what Narrator and other screen
readers read) for the buttons inside that dialog's own window. The buttons
listed for each dialog below have to be drawn by dm and found there by name.

Then the main window's list (gaps row U23), with downloads from a server of
its own: a table named Downloads, a row for each download under the headings'
row and each cell named; a greyed button told off (Delete with nothing
chosen); a running download's bar valued from 0 to 100; "X finished." and "X
failed: reason" said in the status region, politely live; a heading's sort,
where this computer's UI Automation client reads ARIA's properties; and the
progress window's Connections, open or shut and turned by a click.

Windows only (on Linux, `scripts/linux-gui-check.sh` asks AT-SPI the same);
standard library and Windows PowerShell only. The binary is `DM_BINARY`, else
the debug build under `CARGO_TARGET_DIR` or `target`. Exits non-zero,
naming each dialog, button and list check not reached. The window is shown as
a person's is: leave the mouse and keyboard alone while it runs (two minutes
or so).
"""

import http.server
import json
import os
import pathlib
import shutil
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Each dialog opened: its command, the title of its window, and the buttons (or a
# page's tabs) it has.
DIALOGS = [
    ("program.add_url", "Add URL", ["OK", "Cancel"]),
    ("downloads.options", "Options", ["General", "Advanced", "OK", "Cancel"]),
    ("downloads.scheduler", "Scheduler", ["Start now", "Stop", "OK", "Cancel"]),
    ("downloads.speed_limiter.settings", "Speed limiter", ["OK", "Cancel"]),
    ("help.about", "About dm", ["Check for updates"]),
]

# Asks UI Automation what the process's window titled $Title holds.
UIA = r"""
param([int]$ProcessId, [string]$Title)
# Names as they are (an ellipsis, a letter outside the console's code page).
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$C = [System.Windows.Automation.ControlType]
$byProcess = New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $ProcessId)
$windows = @($A::RootElement.FindAll($T::Children, $byProcess))
$dialog = $windows | Where-Object { $_.Current.Name -eq $Title } | Select-Object -First 1
$buttons = @()
$tabs = @()
$fields = @()
if ($dialog) {
  $all = $dialog.FindAll($T::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  $buttons = @($all | Where-Object { $_.Current.ControlType -eq $C::Button } | ForEach-Object { $_.Current.Name })
  $tabs = @($all | Where-Object { $_.Current.ControlType -eq $C::TabItem } | ForEach-Object { $_.Current.Name })
  $fields = @($all | Where-Object { $_.Current.ControlType -eq $C::Edit } | ForEach-Object { $_.Current.Name })
}
@{ windows = @($windows | ForEach-Object { $_.Current.Name }); found = [bool]$dialog;
   buttons = $buttons; tabs = $tabs; fields = $fields } | ConvertTo-Json -Compress
"""

# Asks UI Automation what a window of the process tells of its table, buttons, bars and status
# region (gaps row U23): the main window, known by its title ("dm", or how far and how fast the
# downloads are before " — dm", gaps row U16), or with -Like the first window whose title is
# like that pattern. A row is a data item with data items (its cells) inside; a heading is a
# cell of the first row. The dash is written as its number: Windows PowerShell reads a script
# without a byte-order mark in the computer's own code page.
TOLD = r"""
param([int]$ProcessId, [string]$Like = '')
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$C = [System.Windows.Automation.ControlType]
$byProcess = New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $ProcessId)
$windows = @($A::RootElement.FindAll($T::Children, $byProcess))
$ending = ' ' + [char]0x2014 + ' dm'
if ($Like) {
  $window = $windows | Where-Object { $_.Current.Name -like $Like } | Select-Object -First 1
} else {
  $window = $windows | Where-Object { $_.Current.Name -eq 'dm' -or $_.Current.Name.EndsWith($ending) } | Select-Object -First 1
}
# Read where this computer's UI Automation client knows them: a live region's politeness
# (.NET 4.7.1 and later) and ARIA's properties, where a heading's sort is written.
$live = $null
try { $live = [System.Windows.Automation.AutomationElementIdentifiers]::LiveSettingProperty } catch {}
$aria = $null
try { $aria = [System.Windows.Automation.AutomationProperty]::LookupById(30102) } catch {}
$items = New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, $C::DataItem)
$out = @{ windows = @($windows | ForEach-Object { $_.Current.Name }); title = $null; tables = @(); rows = @();
          buttons = @(); bars = @(); status = @(); live_readable = [bool]$live; sort_readable = [bool]$aria }
if ($window) {
  $out.title = $window.Current.Name
  $all = $window.FindAll($T::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  $pattern = $null
  foreach ($e in $all) {
    $type = $e.Current.ControlType
    if ($type -eq $C::Table) {
      $out.tables += $e.Current.Name
      foreach ($row in @($e.FindAll($T::Descendants, $items))) {
        $cells = @($row.FindAll($T::Descendants, $items))
        if ($cells.Count -eq 0) { continue }
        $sorts = @()
        if ($aria) { $sorts = @($cells | ForEach-Object { [string]$_.GetCurrentPropertyValue($aria) }) }
        $out.rows += ,@{ name = $row.Current.Name; cells = @($cells | ForEach-Object { $_.Current.Name }); sorts = $sorts }
      }
    } elseif ($type -eq $C::Button) {
      $state = $null
      if ($e.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
        $state = [string]$pattern.Current.ExpandCollapseState
      }
      $out.buttons += ,@{ name = $e.Current.Name; enabled = $e.Current.IsEnabled; state = $state }
    } elseif ($type -eq $C::ProgressBar) {
      $value = $null
      if ($e.TryGetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern, [ref]$pattern)) {
        $value = $pattern.Current.Value
      }
      $out.bars += ,@{ name = $e.Current.Name; value = $value }
    } elseif ($type -eq $C::StatusBar) {
      $polite = $null
      if ($live) { try { $polite = [int]$e.GetCurrentPropertyValue($live) } catch {} }
      $out.status += ,@{ name = $e.Current.Name; live = $polite }
    }
  }
}
$out | ConvertTo-Json -Compress -Depth 6
"""

# What the list's downloads are fetched from: a slow one that runs while it is read, one that
# finishes at once, and anything else answered 404 (a failure, said with its reason).
SLOW, DONE, GONE = "slow-told.bin", "done-told.bin", "missing-told.bin"
SLOW_SIZE, DONE_SIZE = 96 * 1024 * 1024, 64 * 1024
BLOCK = bytes((i * 31 + 7) & 0xFF for i in range(65536))


class Files(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        self.do_GET(head=True)

    def do_GET(self, head=False):
        name = self.path.rsplit("/", 1)[-1]
        size = {SLOW: SLOW_SIZE, DONE: DONE_SIZE}.get(name)
        if size is None:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        start, end = 0, size - 1
        asked = self.headers.get("Range")
        if asked and asked.startswith("bytes="):
            a, _, b = asked[6:].partition("-")
            start, end = int(a or 0), min(int(b), size - 1) if b else size - 1
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Type", "application/octet-stream")
        self.end_headers()
        if head:
            return
        try:
            at = start
            while at <= end:
                n, i = min(65536, end - at + 1), at % 65536
                self.wfile.write((BLOCK[i:] + BLOCK[:i])[:n])
                at += n
                if name == SLOW:
                    # About 128 KiB a second a connection: a minute or more on dm's connections.
                    time.sleep(0.5)
        except OSError:
            pass


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def binary():
    if os.environ.get("DM_BINARY"):
        return pathlib.Path(os.environ["DM_BINARY"])
    for target in filter(None, [os.environ.get("CARGO_TARGET_DIR"), str(ROOT / "target")]):
        # The window dm ships on Windows (docs/gaps.md G9).
        candidate = pathlib.Path(target) / "debug" / "dm-gpui.exe"
        if candidate.exists():
            return candidate
    sys.exit("no dm-gpui.exe: build it (cargo build -p dm-gpui) or set DM_BINARY")


def as_list(value):
    """PowerShell's JSON gives a list of one as the item itself."""
    if value is None:
        return []
    return value if isinstance(value, list) else [value]


def until(what, check, seconds=15.0):
    end = time.time() + seconds
    while time.time() < end:
        got = check()
        if got:
            return got
        time.sleep(0.25)
    raise TimeoutError(what)


def soon(check, seconds=15.0):
    """`check`'s answer once it has one within `seconds`, else its last."""
    try:
        return until("", check, seconds)
    except TimeoutError:
        return check()


def the_list(call, dialogs, read, failures):
    """Gaps row U23 in the main window and a progress window, as UI Automation reads them;
    each check not held is added to `failures`. Returns how many were made."""
    made = [0]

    def check(what, ok, detail):
        made[0] += 1
        print(f"  {'ok  ' if ok else 'FAIL'}  {what}" + ("" if ok else f" — {detail}"), flush=True)
        if not ok:
            failures.append(f"the list: {what} ({detail})")

    server = Server(("127.0.0.1", 0), Files)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = lambda name: f"http://127.0.0.1:{server.server_address[1]}/{name}"
    rows = lambda: call("queue.list").get("result") or []
    state = lambda name: next((r["state"]["kind"] for r in rows() if r.get("url", "").endswith("/" + name)), None)
    named = lambda told, name: [b for b in as_list(told.get("buttons")) if b.get("name") == name]
    print("\nthe main window's list (gaps row U23)")
    try:
        call("downloads.add", urls=[url(SLOW)])
        soon(lambda: state(SLOW) == "running", 20)
        # The tree is built once a reader asks, and the cells are told once one reads.
        soon(lambda: len(as_list(read().get("rows"))) >= 2, 30)

        # Said for twelve seconds from a point of the status region of its own: read at once.
        call("downloads.add", urls=[url(DONE)])
        soon(lambda: state(DONE) in ("completed", "failed"), 30)
        said = soon(lambda: [s for s in as_list(read().get("status")) if f"{DONE} finished." in (s.get("name") or "")], 10)
        check('"X finished." is said in the status region', bool(said), str(as_list(read().get("status"))))
        if said and said[0].get("live") is not None:
            check("politely, as a live region", said[0].get("live") == 1, f"live setting {said[0].get('live')}")
        call("downloads.add", urls=[url(GONE)])
        soon(lambda: state(GONE) == "failed", 30)
        said = soon(lambda: [s for s in as_list(read().get("status")) if f"{GONE} failed: " in (s.get("name") or "")], 10)
        check('and "X failed: reason"', bool(said), str(as_list(read().get("status"))))

        call("ui.select", ids=[])
        time.sleep(1.0)
        told = read()
        listed = as_list(told.get("rows"))
        cells = [c for row in listed for c in as_list(row.get("cells"))]
        headings = as_list(listed[0].get("cells")) if listed else []
        check("the list is a table named Downloads", "Downloads" in as_list(told.get("tables")),
              f"tables {as_list(told.get('tables'))} in {told.get('title')!r}")
        check("a row for each download under the headings' row", len(listed) == len(rows()) + 1,
              f"{len(listed)} rows for {len(rows())} downloads")
        check("the headings named as the columns are", all(h in headings for h in ("File name", "Size", "Status")),
              str(headings))
        check("each download's name a cell of its row", all(n in cells for n in (SLOW, DONE, GONE)), str(cells[:24]))
        check("a greyed button is told off: Delete with nothing chosen, Add URL not",
              any(not b.get("enabled") for b in named(told, "Delete"))
              and any(b.get("enabled") for b in named(told, "Add URL")),
              str([(b.get("name"), b.get("enabled")) for b in as_list(told.get("buttons"))][:16]))
        bars = [b.get("value") for b in as_list(told.get("bars"))]
        check("the running download's bar has its value, 0 to 100",
              any(isinstance(v, (int, float)) and 0 <= v <= 100 for v in bars) and state(SLOW) == "running",
              f"bars {bars}, {state(SLOW)}")

        if told.get("sort_readable"):
            call("ui.click", text="Size")
            sorted_by = lambda t: [s for row in as_list(t.get("rows"))[:1]
                                   for c, s in zip(as_list(row.get("cells")), as_list(row.get("sorts")))
                                   if c == "Size" and "sort=" in (s or "")]
            check("a heading clicked says the list is sorted by it", bool(soon(lambda: sorted_by(read()), 10)),
                  str(as_list(read().get("rows"))[:1]))
        else:
            print("  --    a heading's sort (not checked: this computer's UI Automation client does not read "
                  "ARIA's properties)", flush=True)

        done = next((r for r in rows() if r.get("url", "").endswith("/" + DONE)), None)
        call("ui.select", ids=[done["id"]] if done else [])
        check("and Delete is no longer told off once a finished download is chosen",
              bool(done) and bool(soon(lambda: any(b.get("enabled") for b in named(read(), "Delete")), 10)),
              str(named(read(), "Delete")))

        # The progress window's Connections: open or shut, as its arrow shows, and turned by a click.
        slow = next((r for r in rows() if r.get("url", "").endswith("/" + SLOW)), None)
        call("ui.select", ids=[slow["id"]] if slow else [])
        call("command.run", id="file.show_progress")
        shown = soon(lambda: [d for d in dialogs() if "Download progress" in (d or "")], 15)
        connections = lambda: named(read("*Download progress*"), "Connections")
        before = soon(lambda: [b for b in connections() if b.get("state") in ("Collapsed", "Expanded")], 10)
        check("its Connections says whether it is open or shut", bool(shown) and bool(before),
              f"windows {dialogs()}, Connections {connections()}")
        if before:
            call("ui.click", text="Connections")
            was = before[0].get("state")
            after = soon(lambda: [b for b in connections() if b.get("state") not in (None, was)], 10)
            check("and a click turns it", bool(after), f"was {was}, now {connections()}")
        call("ui.key", key="Escape")
    except (OSError, ValueError, KeyError, TypeError) as broke:
        check("the list's reading ran to its end", False, repr(broke))
    finally:
        call("command.run", id="downloads.stop_all")
        server.shutdown()
    return made[0]


def main():
    if sys.platform != "win32":
        sys.exit("Windows only: on Linux, scripts/linux-gui-check.sh asks AT-SPI the same")
    exe = binary()
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-a11y-"))
    (home / "downloads").mkdir()
    # Without the progress and finished windows a new install opens on every download (contract
    # row B63): the list is what is read, and the progress window is opened when asked for.
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(home / "downloads"), "show_progress": False, "show_complete": False,
        "popups_version": 2}), encoding="utf-8")
    reader = home / "uia.ps1"
    reader.write_text(UIA, encoding="utf-8")
    told = home / "told.ps1"
    told.write_text(TOLD, encoding="utf-8")
    env = dict(os.environ, DM_HOME=str(home))
    app = subprocess.Popen([str(exe), "--control", "0"], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    failures = []
    reached = 0
    listed = 0
    conn = None
    try:
        port_file = home / "control.port"
        port = until("dm's control port", lambda: port_file.exists() and port_file.read_text(encoding="utf-8").strip(), 90)
        conn = socket.create_connection(("127.0.0.1", int(port)), timeout=60)
        replies = conn.makefile("r", encoding="utf-8")

        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())

        def dialogs():
            return [d.get("title") for d in (call("ui.windows").get("result") or {}).get("dialogs", [])]

        def drawn():
            texts = (call("ui.texts").get("result") or {}).get("texts", [])
            return {t["text"] for t in texts if t.get("window") == "dialog"}

        def read(like=""):
            """What UI Automation reads of the main window, or with `like` of a dialog (`TOLD`)."""
            asked = subprocess.run(
                ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(told),
                 "-ProcessId", str(app.pid)] + (["-Like", like] if like else []),
                capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=90)
            try:
                return json.loads(asked.stdout.strip() or "{}")
            except ValueError:
                return {}

        call("auth", token=(home / "control.token").read_text(encoding="utf-8").strip())
        until("dm's window", lambda: call("ui.inspect").get("ok"), 30)
        time.sleep(1.5)
        for command, title, wanted in DIALOGS:
            reply = call("command.run", id=command)
            if not reply.get("ok"):
                failures.append(f"{title}: {command} was refused: {reply.get('error')}")
                continue
            try:
                until(f"{title}'s window", lambda: title in dialogs())
                until(f"{title}'s buttons drawn", lambda: set(wanted) <= drawn())
            except TimeoutError as waited:
                failures.append(f"{title}: not drawn as expected ({waited}; drawn: {sorted(drawn())})")
                call("ui.key", key="Escape")
                continue
            # The system's tree is built once the window has been shown a moment.
            time.sleep(1.0)
            asked = subprocess.run(
                ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(reader),
                 "-ProcessId", str(app.pid), "-Title", title],
                capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
            try:
                seen = json.loads(asked.stdout.strip() or "{}")
            except ValueError:
                seen = {}
            buttons, fields = as_list(seen.get("buttons")), as_list(seen.get("fields"))
            # A page's tab is read as a tab (the GPUI window's Options), a button as a button.
            tabs = as_list(seen.get("tabs"))
            missing = [w for w in wanted if w not in buttons and w not in tabs]
            print(f"{title}: dm drew {', '.join(wanted)}; UI Automation reads buttons "
                  f"[{' | '.join(buttons)}], tabs [{' | '.join(tabs)}], text fields [{' | '.join(fields)}]")
            if not seen.get("found"):
                windows = ", ".join(repr(w) for w in as_list(seen.get("windows")))
                failures.append(f"{title}: no window of that name in UI Automation (the process's: {windows}) "
                                f"{asked.stderr.strip()[:300]}")
            elif missing:
                failures.append(f"{title}: a screen reader does not reach {', '.join(missing)}")
            else:
                reached += 1
            call("ui.key", key="Escape")
            until(f"{title} to close", lambda: title not in dialogs(), 10)
        listed = the_list(call, dialogs, read, failures)
    finally:
        if conn:
            try:
                call("command.run", id="program.exit")
                app.wait(20)
            except (OSError, ValueError, subprocess.TimeoutExpired):
                pass
            conn.close()
        if app.poll() is None:
            app.kill()
            app.wait(15)
        shutil.rmtree(home, ignore_errors=True)
    for failure in failures:
        print("FAIL", failure)
    print(f"{reached} of {len(DIALOGS)} dialogs reach a screen reader; "
          f"{listed - len([f for f in failures if f.startswith('the list: ')])} of {listed} list checks hold")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
