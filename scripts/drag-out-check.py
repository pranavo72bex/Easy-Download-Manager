#!/usr/bin/env python3
"""A finished file dragged out of dm's window lands in Finder (macOS only).

    python3 scripts/drag-out-check.py

Runs the built dm on a throwaway home, downloads a file from a loopback
server, opens an empty folder in Finder, then drags the row with the real
mouse (CGEvent, through a small Swift helper) from dm's window into that
Finder window, and checks the file was copied there byte for byte. Needs
the terminal to be allowed to control the computer (System Settings ›
Privacy & Security › Accessibility); without it the events go nowhere and
the check says so. Not in CI: it moves the real pointer.
"""
import http.server
import json
import os
import pathlib
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")
PAYLOAD = os.urandom(300_000)

# Window bounds by owner and title, and a real left-button drag along a path.
SWIFT = r"""
import CoreGraphics
import Foundation
let args = CommandLine.arguments
if args[1] == "bounds" {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
    for w in list {
        let owner = w[kCGWindowOwnerName as String] as? String ?? ""
        let name = w[kCGWindowName as String] as? String ?? ""
        let layer = w[kCGWindowLayer as String] as? Int ?? 1
        // Titles need Screen Recording; without them, the frontmost normal
        // window of that application.
        if owner.contains(args[2]) && layer == 0 && (args.count < 4 || name.isEmpty || name.contains(args[3])) {
            let b = w[kCGWindowBounds as String] as! [String: Any]
            print("\(b["X"]!) \(b["Y"]!) \(b["Width"]!) \(b["Height"]!)")
            exit(0)
        }
    }
    exit(1)
}
func at(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: x, y: y) }
let (x0, y0, x1, y1) = (Double(args[2])!, Double(args[3])!, Double(args[4])!, Double(args[5])!)
let src = CGEventSource(stateID: .hidSystemState)
CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: at(x0, y0), mouseButton: .left)!.post(tap: .cghidEventTap)
usleep(300_000)
CGEvent(mouseEventSource: src, mouseType: .leftMouseDown, mouseCursorPosition: at(x0, y0), mouseButton: .left)!.post(tap: .cghidEventTap)
usleep(200_000)
for i in 1...40 {
    let t = Double(i) / 40
    CGEvent(mouseEventSource: src, mouseType: .leftMouseDragged, mouseCursorPosition: at(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t), mouseButton: .left)!.post(tap: .cghidEventTap)
    usleep(25_000)
}
usleep(400_000)
CGEvent(mouseEventSource: src, mouseType: .leftMouseUp, mouseCursorPosition: at(x1, y1), mouseButton: .left)!.post(tap: .cghidEventTap)
"""


class Serve(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/zip")
        self.send_header("Content-Length", str(len(PAYLOAD)))
        self.end_headers()
        self.wfile.write(PAYLOAD)


def main():
    if sys.platform != "darwin":
        print("macOS only")
        return 0
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-drag-"))
    helper = work / "drag"
    (work / "drag.swift").write_text(SWIFT)
    subprocess.run(["swiftc", "-O", "-o", str(helper), str(work / "drag.swift")], check=True)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Serve)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    home, target = work / "home", work / "dropped-here"
    target.mkdir()
    home.mkdir()
    # Its own download folder: never the person's.
    (home / "settings.json").write_text(json.dumps({"download_dir": str(work / "out")}))
    env = {**os.environ, "DM_HOME": str(home)}
    app = subprocess.Popen([str(BINARY), "--control", "0"], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    ok = False
    try:
        port_file = home / "control.port"
        for _ in range(200):
            if port_file.exists() and port_file.read_text().strip():
                break
            time.sleep(0.1)
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")

        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())

        call("auth", token=(home / "control.token").read_text().strip())
        call("downloads.add", urls=[f"http://127.0.0.1:{server.server_port}/dragged.zip"])
        for _ in range(100):
            rows = call("queue.list")["result"]
            if rows and rows[0]["state"]["kind"] == "completed":
                break
            time.sleep(0.1)
        row = call("ui.inspect")["result"]["rows"][0]
        print("  row:", row["name"], row["status"], row["rect"])

        subprocess.run(["open", str(target)], check=True)
        time.sleep(1.5)
        # Beside dm's window, on the same display.
        bounds = lambda *who: subprocess.run([str(helper), "bounds", *who], capture_output=True, text=True).stdout.split()
        dm = bounds("dm-desktop") or bounds("dm")
        finder = bounds("Finder", target.name)
        print("  dm window:", dm, " Finder window:", finder)
        if not dm or not finder:
            print("FAIL  could not find both windows")
            return 1
        dx, dy, dw, dh = map(float, dm)
        fx0, fy0 = int(dx + dw + 20), int(dy)
        moved = subprocess.run(["osascript", "-e",
                                f'tell application "Finder" to set bounds of front window to {{{fx0}, {fy0}, {fx0 + 500}, {fy0 + 400}}}'],
                               capture_output=True, text=True)
        if moved.returncode:
            print("  could not move the Finder window:", moved.stderr.strip()[:120])
        time.sleep(0.8)
        finder = bounds("Finder", target.name) or finder
        # The content starts below macOS's title bar.
        left, top, right, bottom = row["rect"]
        x0, y0 = dx + left + 40, dy + 28 + (top + bottom) / 2
        fx, fy, fw, fh = map(float, finder)
        x1, y1 = fx + fw / 2, fy + fh / 2 + 20
        subprocess.run([str(helper), "drag", str(x0), str(y0), str(x1), str(y1)], check=True)
        landed = target / "dragged.zip"
        for _ in range(50):
            if landed.exists() and landed.stat().st_size == len(PAYLOAD):
                break
            time.sleep(0.1)
        ok = landed.exists() and landed.read_bytes() == PAYLOAD
        said = call("ui.inspect")["result"]["said"]
        print(("  ok  " if ok else "FAIL  ") + "the dragged file landed in the Finder folder, byte for byte"
              + ("" if ok else f" (folder holds {[p.name for p in target.iterdir()]}; dm said {said!r})"))
        if not ok:
            print("      if nothing moved: allow this terminal under Privacy & Security › Accessibility")
        conn.close()
    finally:
        app.terminate()
        try:
            app.wait(5)
        except subprocess.TimeoutExpired:
            app.kill()
        subprocess.run(["osascript", "-e", f'tell application "Finder" to close (every window whose name is "{target.name}")'],
                       capture_output=True)
        shutil.rmtree(work, ignore_errors=True)
    return 0 if ok else 1


sys.exit(main())
