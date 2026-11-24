#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Reads the table `cargo llvm-cov --summary-only` prints (for .github/workflows/ai-tests.yml).

    python3 scripts/coverage-table.py total TABLE              # the workspace's % of lines
    python3 scripts/coverage-table.py missed TABLE FILE        # FILE's lines no test runs
    python3 scripts/coverage-table.py pick TABLE [SKIP...]     # the file to write tests for

`pick` names the file with the most lines no test runs, leaving out what a Linux runner
cannot run (each system's own code: platform/, the tray, X11, Wayland, macOS, Windows,
Android, Safari), files too small to be worth a night, and each SKIP (files tried lately).
It prints `FILE LINES MISSED`, or nothing when no file is left.
"""

import re
import sys

ELSEWHERE = re.compile(
    r"/platform/|tray|x11|wayland|macos|windows|android|syswindow|mica|safari|"
    r"crates/dm-android/|crates/dm-safari/|/main\.rs$|/build\.rs$"
)


def rows(path):
    """(file, lines, missed) for each file row, and the TOTAL row as file "TOTAL"."""
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            cells = line.split()
            # Filename, then regions, missed, cover, functions, missed, executed, lines,
            # missed lines, cover, branches, missed, cover.
            if len(cells) < 13 or not (cells[0].endswith(".rs") or cells[0] == "TOTAL"):
                continue
            try:
                out.append((cells[0], int(cells[7]), int(cells[8])))
            except ValueError:
                continue
    return out


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    what, table = argv[1], argv[2]
    found = rows(table)
    if what == "total":
        for name, lines, missed in found:
            if name == "TOTAL" and lines:
                print(f"{100.0 * (lines - missed) / lines:.2f}")
                return
        sys.exit(f"no TOTAL row in {table}")
    if what == "missed":
        if len(argv) < 4:
            sys.exit(__doc__)
        for name, _, missed in found:
            if name == argv[3] or name.endswith("/" + argv[3]):
                print(missed)
                return
        sys.exit(f"{argv[3]} is not in {table}")
    if what == "pick":
        skip = set(argv[3:])
        left = [
            (missed, name, lines)
            for name, lines, missed in found
            if name != "TOTAL"
            and name not in skip
            and not ELSEWHERE.search(name)
            and lines >= 60
            and missed >= 20
        ]
        if left:
            missed, name, lines = max(left)
            print(f"{name} {lines} {missed}")
        return
    sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
