#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Names each test a CI run failed as a GitHub annotation.

    python3 scripts/ci-failed-tests.py desktop/target/nextest/ci/junit.xml

A job's log is open to the repository's administrators only; its annotations
are open to everyone (the checks API), so a failure is named where whoever
works on it can read it. Reads nextest's JUnit report and prints one
`::error` line for each test whose every try failed, and one `::warning` for a
test that failed and then passed (flaky). Prints nothing for a clean run.
"""
import sys
import xml.etree.ElementTree as ET


def main(path):
    try:
        root = ET.parse(path).getroot()
    except (OSError, ET.ParseError) as e:
        print(f"::warning title=No test report::{path}: {e}")
        return
    named = 0
    for case in root.iter("testcase"):
        failed = case.find("failure") if case.find("failure") is not None else case.find("error")
        flaky = case.find("flakyFailure")
        if failed is None and flaky is None:
            continue
        name = f"{case.get('classname', '')} {case.get('name', '')}".strip()
        said = ((failed if failed is not None else flaky).get("message") or "").splitlines()
        first = said[0][:300] if said else ""
        level = "error" if failed is not None else "warning"
        print(f"::{level} title=Test {'failed' if failed is not None else 'flaky'}::{name}: {first}")
        named += 1
        if named >= 40:
            print("::warning title=More failures::only the first 40 are named here")
            break


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "target/nextest/ci/junit.xml")
