#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Line coverage of what the extension and the app say to each other, held to a floor.

    python3 scripts/contract-coverage.py              # measure and hold to the floor
    python3 scripts/contract-coverage.py --floor 97   # another floor, for trying one

The extension does the bare minimum and the app decides (gaps row A22): an app update must
never stop an extension a store has not yet updated, and the other way round. What carries
that promise is three parts, each measured with its own tests and each held, file by file,
to the floor in docs/contract-coverage-floor.txt (95% of lines, the maintainer's, 2026-11-15):

  * dm-protocol, the frames both sides read and write, the catch rule and the panel's list
    (desktop/crates/dm-protocol, its unit tests and tests/wire.rs against every released
    extension's frozen frames);
  * the extension's deciding parts (browser/, everything compiled for the host; the browser
    side is WebAssembly and is driven by scripts/chrome-check.py instead);
  * the native host that answers the extension (desktop/crates/dm-shell/src/host*).

A file under the floor fails the run, naming the file and its lines not run, so code added
untested to the contract shows before a merge. Test helpers shared between crates
(desktop/crates/dm-test-support) are not the contract and are not counted. Needs cargo-llvm-cov.
Standard library only.
"""
import json, os, pathlib, subprocess, sys

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

ROOT = pathlib.Path(__file__).resolve().parent.parent
FLOOR_FILE = ROOT / "docs" / "contract-coverage-floor.txt"

# (what it is, the directory cargo runs in, its arguments, which files count)
PARTS = [
    ("dm-protocol", ROOT / "desktop", ["-p", "dm-protocol"], "/crates/dm-protocol/src/"),
    ("extension", ROOT / "desktop", ["-p", "dm-catcher"], "/browser/src/"),
    ("native host", ROOT / "desktop", ["-p", "dm-shell"], "/crates/dm-shell/src/host"),
]
NOT_COUNTED = ("/dm-test-support/", "/tests/", "tests.rs", "_tests.rs")


def floor():
    for i, arg in enumerate(sys.argv):
        if arg == "--floor" and i + 1 < len(sys.argv):
            return float(sys.argv[i + 1])
    return float(FLOOR_FILE.read_text(encoding="utf-8").strip())


def cleared(cwd):
    """What `cargo llvm-cov` kept from earlier runs in `cwd`'s workspace, cleared: a first run
    after a change once read dm-protocol's lib.rs at 49.87% on an earlier build's data and
    99.74% when run again (gaps row 207)."""
    subprocess.run(["cargo", "llvm-cov", "clean", "--workspace"], cwd=cwd,
                   capture_output=True, text=True, check=False, encoding="utf-8", errors="replace")


def measured(cwd, args):
    """llvm-cov's per-file summary for one crate's tests, as JSON."""
    run = subprocess.run(
        ["cargo", "llvm-cov", *args, "--json", "--summary-only"],
        cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    if run.returncode != 0:
        sys.stderr.write(run.stderr[-4000:])
        sys.exit(f"the tests of {cwd.name} {' '.join(args)} failed or could not be measured")
    return json.loads(run.stdout)["data"][0]["files"]


def missed_lines(cwd, args, filename):
    """The line numbers of `filename` no test ran, for the report."""
    run = subprocess.run(
        ["cargo", "llvm-cov", "report", *args, "--text"],
        cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    lines, inside = [], False
    for row in run.stdout.splitlines():
        if row.endswith(".rs:"):
            inside = row[:-1] == filename
            continue
        if inside:
            parts = row.split("|")
            if len(parts) >= 3 and parts[1].strip() == "0" and parts[0].strip().isdigit():
                lines.append(int(parts[0]))
    return lines


def main():
    least = floor()
    failed = []
    total_lines = total_covered = 0
    for workspace in {cwd for _, cwd, _, _ in PARTS}:
        cleared(workspace)
    for name, cwd, args, counted in PARTS:
        print(f"{name}:")
        for entry in measured(cwd, args):
            path = entry["filename"].replace("\\", "/")
            if counted not in path or any(n in path for n in NOT_COUNTED):
                continue
            lines = entry["summary"]["lines"]
            count, covered = lines["count"], lines["covered"]
            if count == 0:
                continue
            percent = 100.0 * covered / count
            total_lines += count
            total_covered += covered
            short = path.split(str(ROOT).replace("\\", "/") + "/", 1)[-1]
            ok = percent >= least
            print(f"  {'ok  ' if ok else 'LOW '} {percent:6.2f}%  {covered:5}/{count:<5} {short}")
            if not ok:
                failed.append((short, percent, missed_lines(cwd, args, entry["filename"])))
    percent = 100.0 * total_covered / max(total_lines, 1)
    print(f"contract: {percent:.2f}% of {total_lines} lines (floor {least:g}% for each file)")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as out:
            out.write(f"Contract coverage: {percent:.2f}% of {total_lines} lines, floor {least:g}%\n")
    if failed:
        for short, got, lines in failed:
            shown = ", ".join(str(n) for n in lines[:40]) + (" …" if len(lines) > 40 else "")
            # A line counted as missed for a part of it only: the full report shows which part.
            shown = shown or "parts of lines; see `cargo llvm-cov report --text`"
            print(f"under the floor: {short} at {got:.2f}%; lines not run: {shown}")
        sys.exit(1)


if __name__ == "__main__":
    main()
