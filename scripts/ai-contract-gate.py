#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Whether a bot's pull request keeps every contract row main keeps (scripts/ai-merge.sh).

    python3 scripts/ai-contract-gate.py BRANCH.txt MAIN.txt [MAIN.txt...] [--contract BRANCH-CONTRACT.md]
    python3 scripts/ai-contract-gate.py --self-test

BRANCH.txt and MAIN.txt are what human-qa.py said on the pull request's commit and on main
(contract-linux.yml's `contract-words` artifact, human-qa.txt), each judged row by row as
scripts/human-qa-report.py judges them. Given main's last few runs, a row is held to only when
each of them held it (a run that broke is passed over): a row that fails now and then on CI's
runner does not stop a change. The bot's change is held to main, not to the whole
contract: a row main does not hold on CI's runner (a live site, a browser panel) does not
stop it, but every row main holds must hold with the change, and a row the change adds to
the contract (--contract, the branch's docs/behaviour-contract.md) must hold too.

Prints each row that does not, and exits 1 when there is one; 0 when the change keeps every
row; 2 when it cannot judge (main's words missing or naming no row held: a run that broke is
not a measure). Standard library only.
"""
import argparse
import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location("human_qa_report", ROOT / "scripts" / "human-qa-report.py")
report = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(report)


def read(path):
    p = pathlib.Path(path)
    return p.read_text(encoding="utf-8", errors="replace") if p.is_file() else None


def verdicts(rows, said):
    judged, _, _ = report.judge(rows, said)
    return {r: (v, w) for r, _, v, w in judged}


def gate(main_rows, branch_rows, branch_said, main_said):
    """(lost, reason): the rows that do not hold with the change, or reason when it cannot judge.

    main_said is one run's words, or a list of main's last runs': a row is held to only when
    every one of them that did not break held it, so a row that fails now and then on CI's
    runner (a slow site, a busy display) never blocks a change it has nothing to do with."""
    runs = main_said if isinstance(main_said, list) else [main_said]
    held_sets = []
    for said in runs:
        if said is None:
            continue
        held = {r for r, (v, _) in verdicts(main_rows, said).items() if v == "held"}
        if held:  # a run that held no row broke, and is no measure
            held_sets.append(held)
    if not held_sets:
        return None, "main's contract runs left no words, or held no row (they broke)"
    steady = set.intersection(*held_sets)
    held_on_main = [r for r, _ in main_rows if r in steady]
    on_branch = verdicts(branch_rows, branch_said)
    known = {r for r, _ in main_rows}
    added = [r for r, _ in branch_rows if r not in known]
    lost = []
    for r in held_on_main + added:
        v, why = on_branch.get(r, ("not checked", "the row is gone from the contract"))
        if v != "held":
            lost.append((r, v, why, r in added))
    return lost, None


def self_test():
    rows = [("B01", "a"), ("B02", "b"), ("B03", "c")]
    main = "The contract, row by row\n  FAIL         B03  a live site\n\n2 of 3 rows hold\n"
    same = main
    worse = "The contract, row by row\n  FAIL         B02  the dialog closed\n  FAIL         B03  a live site\n\n1 of 3 rows hold\n"
    better = "The contract, row by row\n\n3 of 3 rows hold\n"
    broke = "STOPPED, no row checked: the window is black\n"
    assert gate(rows, rows, same, main) == ([], None)
    assert gate(rows, rows, better, main) == ([], None)
    lost, why = gate(rows, rows, worse, main)
    assert why is None and [l[0] for l in lost] == ["B02"] and lost[0][2] == "the dialog closed", lost
    # A branch whose run broke loses every row main held.
    lost, _ = gate(rows, rows, broke, main)
    assert [l[0] for l in lost] == ["B01", "B02"], lost
    lost, _ = gate(rows, rows, None, main)
    assert [l[0] for l in lost] == ["B01", "B02"], lost
    # Main's own run broke, or left nothing: no measure.
    assert gate(rows, rows, same, broke)[0] is None
    assert gate(rows, rows, same, None)[0] is None
    # A row the change adds must hold.
    more = rows + [("B04", "d")]
    lost, _ = gate(rows, more, "The contract, row by row\n  NOT CHECKED  B04\n  FAIL         B03  x\n", main)
    assert [(l[0], l[3]) for l in lost] == [("B04", True)], lost
    assert gate(rows, more, "The contract, row by row\n  FAIL         B03  x\n\n3 of 4 rows hold\n", main) == ([], None)
    # Main's last runs: a row one of them failed (flaky on the runner) is not held to; a run
    # that broke is passed over, not taken as every row failing.
    flaky = "The contract, row by row\n  FAIL         B02  slow\n  FAIL         B03  a live site\n\n1 of 3 rows hold\n"
    assert gate(rows, rows, worse, [main, flaky]) == ([], None)
    lost, _ = gate(rows, rows, worse, [main, main, broke])
    assert [l[0] for l in lost] == ["B02"], lost
    assert gate(rows, rows, worse, [broke, None])[0] is None
    print("ai-contract-gate.py self-test: ok")


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("branch", nargs="?", help="human-qa.txt from the pull request's commit")
    p.add_argument("main", nargs="*", help="human-qa.txt from main's last runs, the newest first")
    p.add_argument("--contract", help="the branch's docs/behaviour-contract.md (else main's, this checkout's)")
    p.add_argument("--self-test", action="store_true")
    a = p.parse_args()
    if a.self_test:
        return self_test()
    if not a.branch or not a.main:
        p.error("name the branch's and main's human-qa.txt, or --self-test")
    main_rows = report.contract_rows(report.CONTRACT.read_text(encoding="utf-8"))
    branch_text = read(a.contract) if a.contract else None
    branch_rows = report.contract_rows(branch_text) if branch_text else main_rows
    if not main_rows or not branch_rows:
        print("no rows in the contract", file=sys.stderr)
        return 2
    lost, why = gate(main_rows, branch_rows, read(a.branch), [read(m) for m in a.main])
    if lost is None:
        print(f"Cannot judge: {why}.")
        return 2
    if not lost:
        print("Every contract row main holds holds with this change.")
        return 0
    for r, v, w, new in lost:
        what = "a row this change adds" if new else "held on main"
        print(f"- {r} ({what}): {v}{': ' + report.cell(w, 200) if w else ''}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
