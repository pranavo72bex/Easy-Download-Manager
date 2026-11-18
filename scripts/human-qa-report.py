#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What a run of human-qa.py said, row by row, as a table (gaps row R5).

    python3 scripts/human-qa-report.py human-shots/linux-qa/human-qa.txt
    python3 scripts/human-qa-report.py <human-qa.txt> --summary "$GITHUB_STEP_SUMMARY" --annotate
    python3 scripts/human-qa-report.py --self-test

Reads the words human-qa.py printed (linux-human-qa.sh brings them back as
human-qa.txt) and docs/behaviour-contract.md's rows, and counts each row as
held, failed or not checked: from human-qa.py's own "The contract, row by
row" list when the run got that far, else from the checks it printed before
it stopped. Every row the contract has and the run did not reach is not
checked.

It only reports: it changes no rule of human-qa.py or the contract, and it
exits 0 whatever the rows say (2 only when it cannot read the contract).
CI's run of the rows on Linux is reported, not gating. The gate is still a
person's run of human-qa.py, in sight.

Standard library only. It never imports human-qa.py, which runs when
imported.
"""
import argparse, pathlib, re, sys

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTRACT = ROOT / "docs" / "behaviour-contract.md"
# The same rows human-qa.py reads (its `rows`), with what the person does.
ROW = re.compile(r"^\| (B\d\d) \|([^|]*)\|", re.M)
CHECK = re.compile(r"^\s+(PASS|FAIL)\s+(B\d\d|WALK)\s+(.*)$")
LISTED_FAIL = re.compile(r"^\s+FAIL\s+(B\d\d)\s+(.*)$")
LISTED_UNCHECKED = re.compile(r"^\s+NOT CHECKED\s+(B\d\d)\s*$")
HOLD = re.compile(r"^(\d+) of (\d+) rows hold")


def contract_rows(text):
    return [(r, what.strip()) for r, what in ROW.findall(text)]


def judge(rows, said):
    """Each row's verdict ('held', 'failed', 'not checked') and why, and a note on the run."""
    lines = said.splitlines() if said is not None else []
    verdict, why, note = {}, {}, None
    if said is None:
        note = "human-qa.py wrote nothing: the build or the container stopped before it ran."
    elif "The contract, row by row" in said:
        at = lines.index(next(l for l in lines if "The contract, row by row" in l))
        for line in lines[at + 1:]:
            if m := LISTED_UNCHECKED.match(line):
                verdict[m.group(1)] = "not checked"
            elif m := LISTED_FAIL.match(line):
                verdict[m.group(1)], why[m.group(1)] = "failed", m.group(2).strip()
        for r, _ in rows:
            verdict.setdefault(r, "held")
        if not any(HOLD.match(l) for l in lines[at + 1:]):
            note = "human-qa.py listed the rows but did not reach its count."
    else:
        # It stopped before its own list: each row from the checks it printed.
        got = {}
        for line in lines:
            if m := CHECK.match(line):
                got.setdefault(m.group(2), []).append((m.group(1) == "PASS", m.group(3).split(" — ")[0].strip()))
        for r, _ in rows:
            checks = got.get(r)
            if not checks:
                verdict[r] = "not checked"
            elif all(ok for ok, _ in checks):
                verdict[r] = "held"
            else:
                verdict[r], why[r] = "failed", "; ".join(w for ok, w in checks if not ok)
        stopped = next((l for l in lines if l.startswith("STOPPED")), None)
        note = (stopped or "human-qa.py ended before its row-by-row list") + \
            " (counted from the checks it printed before that)."
    walk = [l.strip() for l in lines if re.match(r"^\s+FAIL\s+WALK\s", l)]
    return [(r, what, verdict.get(r, "not checked"), why.get(r, "")) for r, what in rows], note, walk


def warning(title, message):
    """A GitHub warning: its title escaped as a property (a comma ends one), its message as data."""
    def data(s):
        return s.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    print(f"::warning title={data(title).replace(':', '%3A').replace(',', '%2C')}::{data(message)}")


def cell(text, limit=160):
    text = " ".join(text.split()).replace("|", "\\|")
    return text if len(text) <= limit else text[:limit - 1] + "…"


def markdown(judged, note, walk):
    held = [j for j in judged if j[2] == "held"]
    failed = [j for j in judged if j[2] == "failed"]
    unchecked = [j for j in judged if j[2] == "not checked"]
    out = [
        "## The contract's rows on Linux (reported, not gating)",
        "",
        "`scripts/linux-human-qa.sh` on CI's Linux runner: human-qa.py in a Debian 12 container on an "
        "X display nobody watches, with Chromium in Edge's place. **This does not gate a merge.** "
        "The gate is still a person's run of `python3 scripts/human-qa.py`, in sight "
        "(docs/behaviour-contract.md).",
        "",
        "| Rows | Held | Failed | Not checked |",
        "|-----:|-----:|-------:|------------:|",
        f"| {len(judged)} | {len(held)} | {len(failed)} | {len(unchecked)} |",
        "",
    ]
    if note:
        out += [f"> {cell(note, 400)}", ""]
    if walk:
        out += [f"> The walk through the window broke: {cell(walk[0], 400)}", ""]
    if failed or unchecked:
        out += ["| Row | What a person does | Result | What failed |", "|-----|-----|-----|-----|"]
        out += [f"| {r} | {cell(what, 90)} | {v} | {cell(w)} |" for r, what, v, w in failed + unchecked]
        out.append("")
    if held:
        out += [f"Held: {', '.join(r for r, _, _, _ in held)}", ""]
    return "\n".join(out) + "\n"


def self_test():
    rows = [("B01", "Clicks Add URL"), ("B02", "Types an address"), ("B03", "Presses Start now")]
    whole = "\n".join([
        "  PASS  B01  the dialog stays",
        "  FAIL  B02  details open — took 9 s",
        "",
        "The contract, row by row",
        "  FAIL         B02  details open",
        "  NOT CHECKED  B03",
        "",
        "1 of 3 rows hold; failing or not checked: B02, B03",
    ])
    j, note, walk = judge(rows, whole)
    assert [v for _, _, v, _ in j] == ["held", "failed", "not checked"], j
    assert j[1][3] == "details open" and note is None and not walk, (j, note)
    partial = "  PASS  B01  a\n  PASS  B02  b\n  FAIL  B02  c | d — why\n  FAIL  WALK  the walk ran — boom\n"
    j, note, walk = judge(rows, partial)
    assert [v for _, _, v, _ in j] == ["held", "failed", "not checked"], j
    assert j[1][3] == "c | d" and note and walk, (j, note, walk)
    j, note, _ = judge(rows, "STOPPED, no row checked: the window is black\n")
    assert all(v == "not checked" for _, _, v, _ in j) and note.startswith("STOPPED"), (j, note)
    j, note, _ = judge(rows, None)
    assert all(v == "not checked" for _, _, v, _ in j) and "wrote nothing" in note
    md = markdown(*judge(rows, whole))
    assert "| 3 | 1 | 1 | 1 |" in md and "not gating" in md and "Held: B01" in md, md
    assert "c \\| d" in markdown(*judge(rows, partial))
    real = contract_rows(CONTRACT.read_text(encoding="utf-8"))
    assert len(real) >= 60 and real[0][0] == "B01" and real[0][1], real[:2]
    print(f"human-qa-report.py self-test: ok ({len(real)} rows in the contract)")


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("said", nargs="?", help="human-qa.py's words (linux-human-qa.sh's human-qa.txt)")
    p.add_argument("--summary", help="append the Markdown table to this file (CI: $GITHUB_STEP_SUMMARY)")
    p.add_argument("--annotate", action="store_true", help="a GitHub warning for each row that failed, and one for those not checked")
    p.add_argument("--self-test", action="store_true")
    a = p.parse_args()
    if a.self_test:
        return self_test()
    if not a.said:
        p.error("name human-qa.txt, or --self-test")
    try:
        rows = contract_rows(CONTRACT.read_text(encoding="utf-8"))
    except OSError as e:
        print(f"cannot read {CONTRACT}: {e}", file=sys.stderr)
        return 2
    if not rows:
        print(f"no rows in {CONTRACT}", file=sys.stderr)
        return 2
    path = pathlib.Path(a.said)
    said = path.read_text(encoding="utf-8", errors="replace") if path.is_file() else None
    judged, note, walk = judge(rows, said)
    md = markdown(judged, note, walk)
    print(md, end="")
    if a.summary:
        with open(a.summary, "a", encoding="utf-8") as f:
            f.write(md)
    if a.annotate:
        # GitHub shows ten warnings a step: the failed rows first, the rows not checked as one.
        for r, _, v, w in judged:
            if v == "failed":
                warning(f"Contract row {r} failed on Linux (reported, not gating)", cell(w or v, 300))
        unchecked = [r for r, _, v, _ in judged if v == "not checked"]
        if unchecked:
            warning(f"{len(unchecked)} contract rows not checked on Linux (reported, not gating)", ", ".join(unchecked))
        if note:
            warning("The contract on Linux (reported, not gating)", cell(note, 300))


if __name__ == "__main__":
    sys.exit(main())
