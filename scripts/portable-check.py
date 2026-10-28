#!/usr/bin/env python3
"""A portable dm keeps everything beside itself.

    python3 scripts/portable-check.py path/to/dm-desktop[.exe]

Copies the program into a folder of its own with an empty file named
`portable` beside it (what the Windows portable zip holds), starts it with no
window and no `DM_HOME`, on a computer whose usual places for a program's data
(home, AppData, XDG) are all a scratch folder, and checks that:

  * a `dm-data` folder appears beside the program, with its control port in it;
  * a link added is written to the list in `dm-data`;
  * nothing of dm's was written to the usual places.

Standard library only, so it runs on a bare CI machine. Exits non-zero, saying
what failed.
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

if len(sys.argv) != 2:
    sys.exit(__doc__)
source = pathlib.Path(sys.argv[1]).resolve()
if not source.is_file():
    sys.exit(f"{source} is not a file")

work = pathlib.Path(tempfile.mkdtemp(prefix="dm-portable-"))
program_dir = work / "dm-portable"
decoy = work / "usual-places"
program_dir.mkdir()
decoy.mkdir()
program = program_dir / source.name
shutil.copy2(source, program)
# The portable zip ships the Mesa-free program alone; whatever sits beside the
# one under test (a software OpenGL, on a CI runner) is not needed headless.
(program_dir / "portable").write_bytes(b"")

env = {k: v for k, v in os.environ.items() if k != "DM_HOME"}
for name in ("HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "XDG_DATA_HOME",
             "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"):
    env[name] = str(decoy)

failures = []


def check(what, ok, detail=""):
    print(("  ok    " if ok else "  FAIL  ") + what + (f"  [{detail}]" if detail and not ok else ""))
    if not ok:
        failures.append(what)


app = subprocess.Popen([str(program), "--headless", "--control", "0"], env=env, cwd=work,
                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
data = program_dir / "dm-data"
try:
    port_file = data / "control.port"
    for _ in range(200):
        if port_file.exists() and port_file.read_text().strip():
            break
        if app.poll() is not None:
            break
        time.sleep(0.1)
    check("a dm-data folder appears beside the program", data.is_dir())
    check("its control port is written there", port_file.exists() and port_file.read_text().strip() != "",
          (app.stderr.read().decode(errors="replace")[-300:] if app.poll() is not None else ""))
    if port_file.exists() and (data / "control.token").exists():
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=30)
        replies = conn.makefile("r")

        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())

        call("auth", token=(data / "control.token").read_text().strip())
        # Added for later: nothing is fetched, only the list is written.
        added = call("downloads.add", urls=["http://127.0.0.1:9/portable-check.bin"], later=True)
        check("a link can be added", added.get("ok"), str(added)[:200])
        listed = data / "queue.json"
        for _ in range(100):
            if listed.exists() and "portable-check.bin" in listed.read_text():
                break
            time.sleep(0.1)
        check("the list is written in dm-data", listed.exists() and "portable-check.bin" in listed.read_text())
finally:
    app.terminate()
    try:
        app.wait(15)
    except subprocess.TimeoutExpired:
        app.kill()

# Nothing of dm's in the usual places: its own folder name, or any file it made.
strays = [str(p.relative_to(decoy)) for p in decoy.rglob("*") if p.is_file()]
check("nothing was written to the usual places", not strays, ", ".join(strays[:5]))

shutil.rmtree(work, ignore_errors=True)
if failures:
    sys.exit(f"portable check failed: {', '.join(failures)}")
print("a portable dm keeps what is its own beside it")
