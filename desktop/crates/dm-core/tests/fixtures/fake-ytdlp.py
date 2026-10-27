#!/usr/bin/env python3
"""A stand-in for yt-dlp in tests/ytdlp_fetch.rs.

Asked for the page's reading (`-J`) or what it would download (`-O`), it prints the reading the test wrote in its case
folder, with `__STAGE__` made the folder dm told it to save in. Asked to
download (from the page, or from a reading with `--load-info-json`), it
saves the test's `fallback.bin` there and names it as yt-dlp does. Every
call is written to the case folder's `calls.log`.
"""

import json
import os
import shutil
import sys

args = sys.argv[1:]


def after(flag):
    return args[args.index(flag) + 1] if flag in args else None


stage = os.path.dirname(after("-o") or "")
if "--load-info-json" in args:
    with open(after("--load-info-json")) as f:
        case = json.load(f)["fake_case"]
else:
    page = args[args.index("--") + 1]
    case = page.split("case=", 1)[1]

with open(os.path.join(case, "calls.log"), "a") as log:
    log.write(" ".join(args) + "\n")

if "-J" in args:
    # A slow page: a case folder's `delay` file says for how many seconds.
    if os.path.exists(os.path.join(case, "delay")):
        import time
        time.sleep(float(open(os.path.join(case, "delay")).read()))
    # The panel's list: the reading as yt-dlp prints it for a page.
    with open(os.path.join(case, "info.json")) as f:
        print(f.read().replace("\n", " "))
    sys.exit(0)

if "-O" in args:
    with open(os.path.join(case, "info.json")) as f:
        print(f.read().replace("__STAGE__", stage).replace("\n", " "))
    sys.exit(0)

os.makedirs(stage, exist_ok=True)
out = os.path.join(stage, "fallback [x].mp4")
shutil.copy(os.path.join(case, "fallback.bin"), out)
with open(args[args.index("--print-to-file") + 2], "w") as f:
    f.write(out + "\n")
