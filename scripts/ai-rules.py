#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The rules of AGENTS.md and docs/ai-skills/ a bot's change must keep, checked by machine.

    python3 scripts/ai-rules.py [BASE]   # the working tree against BASE (HEAD)

Only what the change adds is judged, so old code is not blamed. Each rule is one a person
reading a free model's change would otherwise have to catch (AGENTS.md "Rules for a change";
docs/ai-skills/engine.md, testing.md, desktop.md):

- shipped code (a crate's src/, not its tests) adds no `unwrap()`, `expect(`, `panic!`,
  `todo!`, `unimplemented!` or `unreachable!`, and no `eprintln!` or `dbg!` (dm logs with
  `note!`);
- a wait or a timeout is a named const: no `Duration::from_…(<number>)` written inline in
  shipped code, but on a `const` line;
- no `foo/mod.rs`: a module with children is `foo.rs` beside `foo/`;
- a test reaches only 127.0.0.1: no address of another host in a test;
- no `#[allow(...)]` that turns a lint off for shipped code, and no `unsafe`.

Exits 1 with each broken rule and its line, 0 when the change keeps them all. Reads git's
diff only; runs nothing of the change.
"""

import re
import subprocess
import sys

TEST_PATH = re.compile(r"(^|/)(tests?|benches|examples|fuzz)/|(^|/)tests\.rs$|_tests?\.rs$|(^|/)window_tests/|(^|/)harness")
SHIPPED = re.compile(r"^crates/[^/]+/src/.*\.rs$")
PANICS = re.compile(r"\.unwrap\(\)|\.expect\(|\bpanic!\(|\btodo!\(|\bunimplemented!\(|\bunreachable!\(")
PRINTS = re.compile(r"\beprintln!\(|\bdbg!\(")
INLINE_WAIT = re.compile(r"Duration::from_(secs|millis|micros|nanos|secs_f32|secs_f64)\(\s*[0-9]")
ALLOW = re.compile(r"#!?\[allow\(")
UNSAFE = re.compile(r"\bunsafe\b")
OUTSIDE_HOST = re.compile(r"https?://(?!127\.0\.0\.1|localhost|\[::1\]|example\.(com|org|net)|[a-z0-9.-]*\.example\b|[a-z0-9.-]*\.test\b|[a-z0-9.-]*\.invalid\b)[a-z0-9]", re.I)


def git(*args):
    r = subprocess.run(["git", *args], capture_output=True, text=True)
    return r.stdout


def test_region_start(path):
    """The line from which a shipped file is its own tests (its #[cfg(test)]), or None."""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for n, line in enumerate(f, 1):
                if line.strip().startswith("#[cfg(test)]"):
                    return n
    except OSError:
        pass
    return None


def main(argv):
    # New files count once git knows of them: ai_guard (scripts/ai-agent.sh) has run
    # `git add -N .` before this, in the bot's own checkout; this changes no index.
    base = argv[1] if len(argv) > 1 else "HEAD"
    problems = []
    for path in git("diff", "--name-only", "--diff-filter=A", base).split():
        if path.endswith("/mod.rs") and path.startswith("crates/"):
            problems.append(f"{path}: a module with children is foo.rs beside foo/, never foo/mod.rs")
    diff = git("diff", "-U0", "--no-color", base, "--", "*.rs")
    path, line_no, tests_from = None, 0, None
    for raw in diff.splitlines():
        if raw.startswith("+++ "):
            path = raw[6:] if raw.startswith("+++ b/") else None
            tests_from = test_region_start(path) if path else None
            continue
        m = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)", raw)
        if m:
            line_no = int(m.group(1))
            continue
        if not path or not raw.startswith("+") or raw.startswith("+++"):
            continue
        text = raw[1:]
        where = f"{path}:{line_no}"
        line_no += 1
        # A comment ends the code (but `://` in an address is not one).
        code = re.sub(r"(?<!:)//.*$", "", text)
        in_tests = bool(TEST_PATH.search(path)) or (tests_from is not None and line_no - 1 >= tests_from)
        if in_tests:
            if OUTSIDE_HOST.search(code):
                problems.append(f"{where}: a test reaches only 127.0.0.1: {text.strip()[:100]}")
            continue
        if not SHIPPED.match(path):
            continue
        if PANICS.search(code):
            problems.append(f"{where}: shipped code does not unwrap, expect or panic: {text.strip()[:100]}")
        if PRINTS.search(code):
            problems.append(f"{where}: shipped code logs with note!, never eprintln! or dbg!: {text.strip()[:100]}")
        if INLINE_WAIT.search(code) and not re.match(r"\s*(pub(\([a-z ]+\))?\s+)?const\b", code):
            problems.append(f"{where}: a wait or a timeout is a named const: {text.strip()[:100]}")
        if ALLOW.search(code):
            problems.append(f"{where}: no lint turned off in shipped code: {text.strip()[:100]}")
        if UNSAFE.search(code):
            problems.append(f"{where}: no unsafe: {text.strip()[:100]}")
    for p in problems:
        print(p)
    if problems:
        print(f"ai-rules: {len(problems)} rule(s) broken")
        return 1
    print("ai-rules: the change keeps the rules")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
