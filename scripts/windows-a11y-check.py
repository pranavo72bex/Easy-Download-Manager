#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What a screen reader finds in dm's dialogs on Windows (gaps row 160):

    python scripts/windows-a11y-check.py

Every dialog of dm's is a window of its own (contract row B42), and a screen
reader reads a window only through what the program gives the system's
accessibility interface for that window. This starts the real binary on a
throwaway home with its window shown, opens each dialog through the control
protocol, and asks Windows' UI Automation (what Narrator and other screen
readers read) for the buttons inside that dialog's own window. The buttons
listed for each dialog below have to be drawn by dm and found there by name.

Windows only (on Linux, `scripts/linux-gui-check.sh` asks AT-SPI the same);
standard library and Windows PowerShell only. The binary is `DM_BINARY`, else
the debug build under `CARGO_TARGET_DIR` or `desktop/target`. Exits non-zero,
naming each dialog and button not reached. The window is shown as a person's
is: leave the mouse and keyboard alone while it runs (under a minute).
"""

import json
import os
import pathlib
import shutil
import socket
import subprocess
import sys
import tempfile
import time

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


def binary():
    if os.environ.get("DM_BINARY"):
        return pathlib.Path(os.environ["DM_BINARY"])
    for target in filter(None, [os.environ.get("CARGO_TARGET_DIR"), str(ROOT / "desktop" / "target")]):
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


def main():
    # Names hold "…" and "›": as UTF-8 in a log, not the Windows code page.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    if sys.platform != "win32":
        sys.exit("Windows only: on Linux, scripts/linux-gui-check.sh asks AT-SPI the same")
    exe = binary()
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-a11y-"))
    (home / "downloads").mkdir()
    (home / "settings.json").write_text(json.dumps({"download_dir": str(home / "downloads")}))
    reader = home / "uia.ps1"
    reader.write_text(UIA, encoding="utf-8")
    env = dict(os.environ, DM_HOME=str(home))
    app = subprocess.Popen([str(exe), "--control", "0"], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    failures = []
    reached = 0
    conn = None
    try:
        port_file = home / "control.port"
        port = until("dm's control port", lambda: port_file.exists() and port_file.read_text().strip(), 90)
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

        call("auth", token=(home / "control.token").read_text().strip())
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
    print(f"{reached} of {len(DIALOGS)} dialogs reach a screen reader")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
