#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The release binary's size against the last one recorded for this system (gaps row 188).

    python3 scripts/binary-size.py BINARY            # compare, and say so
    python3 scripts/binary-size.py BINARY --record   # make BINARY's size the recorded one

The sizes are in scripts/binary-size.json, one for each system and processor. CI runs this
after each release build and fails only when the binary grew by more than 10%: a size moves a
little with every dependency, and a jump that large is a profile changed, a crate added by
mistake or debug info left in. A smaller binary passes, and says it is time to record the new
size. A system with nothing recorded passes and prints the size to record.
"""

import json
import pathlib
import platform
import sys

HERE = pathlib.Path(__file__).resolve().parent
RECORDED = HERE / "binary-size.json"
LIMIT = 0.10


def system():
    name = {"darwin": "macos", "win32": "windows"}.get(sys.platform, "linux" if sys.platform.startswith("linux") else sys.platform)
    machine = platform.machine().lower().replace("aarch64", "arm64").replace("amd64", "x86_64")
    return f"{name}-{machine}"


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 1:
        sys.exit(__doc__)
    binary = pathlib.Path(args[0])
    if not binary.is_file():
        sys.exit(f"no binary at {binary}")
    size = binary.stat().st_size
    sizes = json.loads(RECORDED.read_text()) if RECORDED.exists() else {}
    key = system()
    if "--record" in sys.argv:
        sizes[key] = size
        RECORDED.write_text(json.dumps(dict(sorted(sizes.items())), indent=1) + "\n")
        print(f"{key}: {size:,} bytes recorded")
        return
    was = sizes.get(key)
    if was is None:
        print(f"{key}: {size:,} bytes; nothing recorded for this system yet "
              f"(python3 scripts/binary-size.py {binary} --record)")
        return
    change = (size - was) / was
    print(f"{key}: {size:,} bytes, {change:+.1%} from the {was:,} recorded")
    if change > LIMIT:
        sys.exit(f"the binary grew by {change:.1%}, over {LIMIT:.0%}: find out why before it ships "
                 "(cargo bloat --release -p dm-gpui --crates), or record the new size if it is meant")
    if change < -LIMIT:
        print("much smaller than recorded: record the new size")


if __name__ == "__main__":
    main()
