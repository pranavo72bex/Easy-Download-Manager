#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The next row of docs/gaps.md a bot can take: `python3 scripts/ai-improve-pick.py [SKIP…]`.

docs/gaps.md is the maintainer's ranked work list: its rows are what EDM should do next, in the
order the maintainer set (its sections, top first). A bot takes a row that is OPEN or PARTIAL
and that needs nothing only a person can give:

- not one waiting for the maintainer (a "decide", their word on the wording, their approval
  still to come, the "Waiting for the maintainer" section);
- not one that needs a machine or a thing a runner does not have (a real phone, the Mac or
  the Windows laptop, a USB disk, the bench, a store's account, a certificate, signing);
- not one that needs a measurement on the bench, or a change to CI, a workflow or the release
  job (the bots may not edit those);
- not one marked `· person` in its state; one marked `· bot` is taken before the rest;
- not one marked `· split`: its steps, the rows under it (K5a, K5b, …), are taken instead;
- not one marked `· after X` while row X is still open (a done row is deleted from the list);
- not one in a section whose introduction says its rows wait for the maintainer's word;
- not one in SKIP (tried in the last fortnight, or with its issue still open).

Prints `ID<TAB>the row's problem<TAB>its state<TAB>what is done and left`, the smallest of the
first section that has one, or nothing. `--all` prints every row a bot can take now, in the
order they would be taken, one `ID  the problem` a line. `--still ID` says whether row ID is
still one for a bot (there, OPEN or PARTIAL, not split and not a person's): exit 0, or exit 1
with why not, so the worker closes an improvement whose row was done, removed or split since.
"""

import re
import sys

WAITS = re.compile(r"decide|maintainer'?s? (word|approval|call|choice)|needs? the maintainer|before work starts|ask(ed)? the maintainer", re.I)
NEEDS = re.compile(
    r"real phone|on a phone \(|the mac\b|on the mac|windows laptop|usb (hard )?disk|on the bench|bench first|"
    r"store account|developer id|certificate|signing|signed by|notari[sz]|buy|paid|hardware|a person|by hand|"
    r"real torrent|real site|in sight|visible window|benchmark|bench/|cpu use|built by ci|release job|workflow|"
    r"the readme table|comparison table|picture of|screenshot|not compiled|written on the mac|"
    r"a run first|a look first",
    re.I,
)
SECTION_WAITS = re.compile(r"(each|every row here)[^.]*waits? for the maintainer'?s? word", re.I)
SIZE = {"S": 0, "S–M": 1, "S-M": 1, "M": 2, "M–L": 3, "M-L": 3, "L": 4}


def main(skip):
    try:
        text = open("docs/gaps.md", encoding="utf-8").read()
    except OSError:
        return
    section, waiting, rank, intro, level, parent_waits = "", False, 0, True, 2, False
    best = None
    # The rows still open, for "· after X".
    still_open = set(re.findall(r"^\|\s*([A-Z]+[0-9]+[a-z]?)\s*\|[^|]*\|\s*(?:OPEN|PARTIAL)\b", text, re.M))
    for line in text.splitlines():
        if line.startswith("#"):
            level = len(line) - len(line.lstrip("#"))
            section = line.lstrip("#").strip()
            own = "waiting for the maintainer" in section.lower() or section.lower().startswith("watch")
            # A sub-heading keeps its section's rule; a new section starts afresh.
            if level <= 2:
                parent_waits = own
            waiting = own or parent_waits
            rank += 1
            intro = True
            continue
        # A section's introduction, before its first table, may say all its rows wait.
        if intro and not line.startswith("|"):
            if SECTION_WAITS.search(line):
                waiting = True
                parent_waits = parent_waits or level <= 2
            continue
        intro = False
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
        if WAITS.search(state) or WAITS.search(problem) or re.search(r"·\s*(person|split)\b", state):
            continue
        # What is left is what the bot would do: it must need nothing a runner lacks.
        waits_for = re.findall(r"[A-Z]+[0-9]+[a-z]?", " ".join(re.findall(r"·\s*after ([^·|]+)", state)))
        if any(w in still_open for w in waits_for):
            continue
        left = detail.split("Left:", 1)[1] if "Left:" in detail else detail
        if NEEDS.search(left) or NEEDS.search(problem) or NEEDS.search(state):
            continue
        size_m = re.search(r"·\s*(S–M|S-M|M–L|M-L|S|M|L)\b", state)
        size = SIZE.get(size_m.group(1), 2) if size_m else 2
        # The maintainer's "· bot" first, whatever its section.
        key = (0 if re.search(r"·\s*bot\b", state) else 1, rank, size)
        if best is None or key < best[0]:
            best = (key, row, problem, state, detail)
    if best:
        _, row, problem, state, detail = best
        print(f"{row}\t{problem}\t{state}\t{detail}")


def every(skip):
    """Every row a bot can take now, in order: the picker asked again with each one skipped."""
    import contextlib
    import io
    taken = []
    while True:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            main(skip | set(taken))
        line = out.getvalue().strip()
        if not line:
            return
        rid, problem = line.split("\t")[:2]
        taken.append(rid)
        print(f"{rid:6} {problem[:110]}")


def still(rid):
    """Row RID is still a bot's to take: exit 0; else say why and exit 1."""
    try:
        text = open("docs/gaps.md", encoding="utf-8").read()
    except OSError:
        return 0  # no list to judge by: leave the issue alone
    m = re.search(r"^\|\s*" + re.escape(rid) + r"\s*\|[^|]*\|([^|]*)", text, re.M)
    if not m:
        print(f"row {rid} is no longer in docs/gaps.md: done or removed")
        return 1
    state = m.group(1)
    if not re.match(r"\s*(OPEN|PARTIAL)\b", state):
        print(f"row {rid} is no longer OPEN or PARTIAL")
        return 1
    if re.search(r"·\s*split\b", state):
        print(f"row {rid} has been split into steps ({rid}a, {rid}b, …): the bots take those")
        return 1
    if re.search(r"·\s*person\b", state):
        print(f"row {rid} is now a person's")
        return 1
    return 0


if __name__ == "__main__":
    if "--still" in sys.argv:
        sys.exit(still(sys.argv[sys.argv.index("--still") + 1]))
    if "--all" in sys.argv:
        every({a for a in sys.argv[1:] if a != "--all"})
    else:
        main(set(sys.argv[1:]))
