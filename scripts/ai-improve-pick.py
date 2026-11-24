#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The next row of docs/gaps.md a bot can take: `python3 scripts/ai-improve-pick.py [SKIP…]`.

docs/gaps.md is the maintainer's ranked work list: its rows are what dm should do next, in the
order the maintainer set (its sections, top first). A bot takes a row that is OPEN or PARTIAL
and that needs nothing only a person can give:

- not one waiting for the maintainer (a "decide", their word on the wording, their approval
  still to come, the "Waiting for the maintainer" section);
- not one that needs a machine or a thing a runner does not have (a real phone, the Mac or
  the Windows laptop, a USB disk, the bench, a store's account, a certificate, signing);
- not one in SKIP (tried in the last fortnight, or with its issue still open).

Prints `ID<TAB>the row's problem<TAB>its state<TAB>what is done and left`, the smallest of the
first section that has one, or nothing.
"""

import re
import sys

WAITS = re.compile(r"decide|maintainer'?s? (word|approval|call|choice)|needs? the maintainer|before work starts|ask(ed)? the maintainer", re.I)
NEEDS = re.compile(
    r"real phone|on a phone \(|the mac\b|on the mac|windows laptop|usb (hard )?disk|on the bench|bench first|"
    r"store account|developer id|certificate|signing|signed by|notari[sz]|buy|paid|hardware|a person|by hand|"
    r"real torrent|real site|in sight|visible window",
    re.I,
)
SIZE = {"S": 0, "S–M": 1, "S-M": 1, "M": 2, "M–L": 3, "M-L": 3, "L": 4}


def main(skip):
    try:
        text = open("docs/gaps.md", encoding="utf-8").read()
    except OSError:
        return
    section, waiting, rank = "", False, 0
    best = None
    for line in text.splitlines():
        if line.startswith("#"):
            section = line.lstrip("#").strip()
            waiting = "waiting for the maintainer" in section.lower() or section.lower().startswith("watch")
            rank += 1
            continue
        m = re.match(r"^\|\s*([A-Z]+[0-9]+[a-z]?)\s*\|(.*)\|\s*$", line)
        if not m or waiting:
            continue
        row, cells = m.group(1), [c.strip() for c in m.group(2).split("|")]
        if len(cells) < 2 or row in skip:
            continue
        problem, state = cells[0], cells[1]
        detail = cells[2] if len(cells) > 2 else ""
        if not re.match(r"(OPEN|PARTIAL)\b", state):
            continue
        if WAITS.search(state) or WAITS.search(problem):
            continue
        # What is left is what the bot would do: it must need nothing a runner lacks.
        left = detail.split("Left:", 1)[1] if "Left:" in detail else detail
        if NEEDS.search(left) or NEEDS.search(problem):
            continue
        size_m = re.search(r"·\s*(S–M|S-M|M–L|M-L|S|M|L)\b", state)
        size = SIZE.get(size_m.group(1), 2) if size_m else 2
        key = (rank, size)
        if best is None or key < best[0]:
            best = (key, row, problem, state, detail)
    if best:
        _, row, problem, state, detail = best
        print(f"{row}\t{problem}\t{state}\t{detail}")


if __name__ == "__main__":
    main(set(sys.argv[1:]))
