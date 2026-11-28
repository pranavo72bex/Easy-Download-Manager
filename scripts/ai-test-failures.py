#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Which tests failed, and whether they are only those main already fails (for the bots).

    python3 scripts/ai-test-failures.py junit JUNIT.xml          # each failed test: binary<TAB>test
    python3 scripts/ai-test-failures.py annotations FILE          # the same, from a run's annotations
    python3 scripts/ai-test-failures.py within FAILED KNOWN [--fewer]

A bot's change is not refused for what main already gets wrong: `within` exits 0 when every
test in FAILED is in KNOWN (main's own failures, read from its last run), and with --fewer
only when FAILED also has fewer (a fix for main's red run must make it less red). It exits 1
when a test fails that main does not fail, and says which. An empty FAILED is within
anything. scripts/ci-failed-tests.py writes the annotations as "binary test: first words".
"""

import sys
import xml.etree.ElementTree as ET


def from_junit(path):
    out = set()
    try:
        root = ET.parse(path).getroot()
    except (OSError, ET.ParseError):
        return None
    for case in root.iter("testcase"):
        if case.find("failure") is not None or case.find("error") is not None:
            out.add(f"{case.get('classname', '').strip()}\t{case.get('name', '').strip()}")
    return out


def from_annotations(path):
    out = set()
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            head = line.split(": ", 1)[0].strip()
            parts = head.split(" ")
            if len(parts) == 2 and parts[0] and parts[1]:
                out.add(f"{parts[0]}\t{parts[1]}")
    return out


def read_set(path):
    out = set()
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.rstrip("\n")
                if "\t" in line:
                    out.add("\t".join(line.split("\t")[:2]))
    except OSError:
        pass
    return out


def main(argv):
    if len(argv) >= 3 and argv[1] == "junit":
        found = from_junit(argv[2])
        if found is None:
            print("no test report: the tests did not run (a build failure)", file=sys.stderr)
            return 2
        for t in sorted(found):
            print(t)
        return 0
    if len(argv) >= 3 and argv[1] == "annotations":
        for t in sorted(from_annotations(argv[2])):
            print(t)
        return 0
    if len(argv) >= 4 and argv[1] == "within":
        failed, known = read_set(argv[2]), read_set(argv[3])
        new = sorted(failed - known)
        for t in new:
            print(f"fails here, not on main: {t.replace(chr(9), ' ')}")
        if new:
            return 1
        if "--fewer" in argv and known and not len(failed) < len(known):
            print(f"main's {len(known)} failing test(s) all still fail: nothing fixed")
            return 1
        print(f"{len(failed)} failing, all among main's own {len(known)}")
        return 0
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
