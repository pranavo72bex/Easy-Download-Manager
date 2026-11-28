#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Whether docs/gaps.md is well formed: `python3 scripts/gaps-check.py [FILE]`.

People and the free-model bots both read and edit the work list, and the bots read it by
program (scripts/ai-improve-pick.py, scripts/ai-context.py), so each row must keep its shape
(docs/gaps.md, "How to read this file"):

- a row is `| ID | the gap | its state | how it closes |`: four cells (a `|` inside `code`
  is not a cell's edge), and no ID twice;
- a step (a row whose parent, its ID less the last letter, is marked `· split`) says whose
  it is, `· bot` or `· person`, and what to do: "Do:", "Where:", "Test:" and "Done when:";
- a step's `· after X` names a step (of its own row or another): X's row deleted means X is
  done.

Exits 1 with each problem and its line, 0 when the file keeps its shape.
"""

import re
import sys
from collections import Counter

ROW = re.compile(r"^\|\s*([A-Z]+[0-9]+[a-z]?)\s*\|")


def cells(line):
    out, cur, code = [], "", False
    for ch in line.strip():
        if ch == "`":
            code = not code
        if ch == "|" and not code:
            out.append(cur)
            cur = ""
        else:
            cur += ch
    out.append(cur)
    return [c.strip() for c in out[1:-1]]


def main(path):
    lines = open(path, encoding="utf-8").read().splitlines()
    rows = {}
    seen = Counter()
    problems = []
    for n, line in enumerate(lines, 1):
        m = ROW.match(line)
        if not m:
            continue
        rid = m.group(1)
        seen[rid] += 1
        c = cells(line)
        rows[rid] = (n, c)
        if len(c) != 4:
            problems.append(f"{path}:{n}: row {rid} has {len(c)} cells, not 4 (a | outside `code`?)")
    for rid, count in seen.items():
        if count > 1:
            problems.append(f"{path}: row {rid} is there {count} times")
    for rid, (n, c) in rows.items():
        if len(c) != 4 or not re.fullmatch(r"[A-Z]+[0-9]+[a-z]", rid):
            continue
        parent = rows.get(rid[:-1])
        if not parent or len(parent[1]) != 4 or not re.search(r"·\s*split\b", parent[1][2]):
            continue
        state, what = c[2], c[3]
        if not re.search(r"·\s*(bot|person)\b", state):
            problems.append(f"{path}:{n}: step {rid} says neither · bot nor · person")
        for word in ("Do:", "Where:", "Test:", "Done when:"):
            if word not in what:
                problems.append(f"{path}:{n}: step {rid} has no \"{word}\"")
        after = " ".join(re.findall(r"·\s*after ([^·|]+)", state))
        if after and not re.search(r"[A-Z]+[0-9]+[a-z]", after):
            problems.append(f"{path}:{n}: step {rid}'s · after names no step")
    for p in problems:
        print(p)
    if problems:
        print(f"gaps-check: {len(problems)} problem(s)")
        return 1
    print(f"gaps-check: {len(rows)} rows, well formed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "docs/gaps.md"))
