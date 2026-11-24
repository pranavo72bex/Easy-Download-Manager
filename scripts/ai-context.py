#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What a free model is handed before it works: `python3 scripts/ai-context.py [FILE]`.

Writes .bot/context.md from what the task says (.bot/issue.json, .bot/failing-tests.txt,
.bot/row.md, or the FILE a nightly test run is for), so a model that would not find its way
alone starts with:

1. the task itself, when it is a step of docs/gaps.md (K3a): its Do, Where, Test and Done
   when, word for word, first;
2. how to work: the test first, then make it build (`cargo test -p CRATE --no-run` after each
   edit), then make it pass, then the checks;
3. which part of dm the task is about (the desktop window, the phone, the engine, the
   browser extension, video pages), from its words and the crates its tests name, and the
   skill pages for it (docs/ai-skills/, .claude/skills/), in the order they win, only when
   scripts/ai-skills-check.py finds them clean;
4. an outline of each file the task names: every fn, struct, enum, trait, impl and const
   with its line and whether it is public, so a model calls what exists and is reachable;
5. a test to copy: the head of the test file the new test goes in (its imports, its helpers,
   one whole test), or the nearest one;
6. for the window, the public methods of its test harness (crates/dm-gpui/src/harness.rs);
7. the files that mention the task's words, their last commits, and a map of the crates
   (cut first when the whole is too long).

The task's words are only searched for, as fixed strings (git grep -F), never run.
Standard library and git only.
"""

import json
import os
import re
import subprocess
import sys
from collections import Counter

LIMIT = 60000
ITEM = re.compile(r"^\s*(pub(\([^)]*\))?\s+)?(async\s+)?(unsafe\s+)?(const\s+fn|fn|struct|enum|trait|impl|const|static|type|mod)\b")
AREAS = {
    "android": {
        "words": r"android|phone|mobile|apk|notification|tap|touch|swipe|back button|gradle|dm-android",
        "crates": ["dm-android", "dm-gpui"],
        "skills": ["docs/ai-skills/android.md", "docs/ai-skills/desktop.md", "docs/ai-skills/ui-ux.md",
                   ".claude/skills/dm-design/SKILL.md", ".claude/skills/break/scenarios.md"],
        "tests": ["cargo test -p dm-android", "cargo nextest run -p dm-gpui -E 'test(~phone)'"],
    },
    "desktop": {
        "words": r"window|dialog|button|menu|toolbar|layout|column|theme|dark mode|font|icon|label|word|"
                 r"translat|language|i18n|scheduler|options|settings|progress window|tray|dm-gpui",
        "crates": ["dm-gpui", "dm-desktop-base"],
        "skills": ["docs/ai-skills/desktop.md", ".claude/skills/dm-design/SKILL.md", "docs/ai-skills/ui-ux.md",
                   ".claude/skills/break/scenarios.md"],
        "tests": ["cargo nextest run -p dm-gpui -E 'test(~NAME)'", "cargo test -p dm-gpui window_tests"],
    },
    "engine": {
        "words": r"download|resume|range|connection|segment|speed|http|https|ftp|torrent|magnet|redirect|"
                 r"server|header|proxy|archive|zip|rar|7z|checksum|queue|dm-core|dm-net|dm-shell",
        "crates": ["dm-core", "dm-net", "dm-shell"],
        "skills": ["docs/ai-skills/engine.md"],
        "tests": ["cargo nextest run -p dm-core -E 'test(~NAME)'", "cargo test -p dm-core --test zoo"],
    },
    "extension": {
        "words": r"browser|extension|chrome|firefox|edge|safari|catch|hand ?off|native host|dm-catcher|dm-protocol",
        "crates": ["dm-catcher", "dm-protocol"],
        "skills": [],
        "tests": ["cd crates/dm-catcher && cargo test"],
    },
    "video": {
        "words": r"youtube|video|stream|hls|dash|m3u8|caption|subtitle|playlist|site|dm-media|dm-js",
        "crates": ["dm-media", "dm-js", "dm-core"],
        "skills": ["docs/ai-skills/engine.md"],
        "tests": ["cargo nextest run -p dm-media", "cargo nextest run -p dm-js"],
    },
}
STOP = set("""the a an and or of to in on for with is are was were be been it this that dm not no but if
then when what which who how why from by at as can cannot does do did has have had will would should
could there their they them its into out up down over under more most less very just also only than
so too all any some each every one two new old get got set use used using make made like""".split())


def git(*args):
    r = subprocess.run(["git", *args], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else ""


def read(path, limit=None):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            text = f.read()
    except OSError:
        return ""
    return text[:limit] if limit else text


def task_text():
    parts = []
    try:
        issue = json.loads(read(".bot/issue.json") or "{}")
        parts.append(issue.get("title", ""))
        parts.append(issue.get("body", "") or "")
        for c in issue.get("comments", [])[-8:]:
            parts.append(c.get("body", "") or "")
    except ValueError:
        pass
    for extra in (".bot/failing-tests.txt", ".bot/row.md", ".bot/feedback.md", ".bot/missing-lines.txt"):
        parts.append(read(extra, 8000))
    if len(sys.argv) > 1:
        parts.append(f"`{sys.argv[1]}`")  # the nightly tests' file: named, so it is outlined
    return "\n".join(p for p in parts if p)


def areas_of(text):
    scores = Counter()
    low = text.lower()
    for name, area in AREAS.items():
        scores[name] += len(re.findall(area["words"], low))
        for crate in area["crates"]:
            scores[name] += 3 * low.count(crate)
    # The crates the failing tests or the file name: the strongest sign.
    for crate in re.findall(r"crates/(dm-[a-z-]+)|\b(dm-[a-z-]+)\t", text):
        name = crate[0] or crate[1]
        for area_name, area in AREAS.items():
            if name in area["crates"]:
                scores[area_name] += 5
    ranked = [a for a, s in scores.most_common() if s > 0]
    return ranked[:2] or ["engine"]


def names_in(text):
    """The task's own names and rarer words, to search the code for."""
    found = Counter()
    for m in re.findall(r"`([^`\n]{3,60})`", text):
        found[m.strip()] += 3
    for m in re.findall(r"\b[a-z][a-z0-9]*(?:_[a-z0-9]+){1,}\b", text):  # snake_case
        found[m] += 2
    for m in re.findall(r"\b[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]+)+\b", text):  # CamelCase
        found[m] += 2
    for m in re.findall(r"[\w/.-]+\.rs\b", text):
        found[m.rsplit("/", 1)[-1][:-3]] += 2
    for m in re.findall(r"\b[a-z]{6,}\b", text.lower()):
        if m not in STOP:
            found[m] += 1
    return [w for w, _ in found.most_common(25) if len(w) >= 4]


def related_files(names, crates):
    hits = Counter()
    lines = {}
    paths = [f"crates/{c}" for c in crates if os.path.isdir(f"crates/{c}")] or ["crates"]
    for name in names:
        out = git("grep", "-n", "-I", "-F", "--", name, *paths)
        for line in out.splitlines()[:60]:
            path, _, rest = line.partition(":")
            if not path.endswith(".rs"):
                continue
            hits[path] += 1
            lines.setdefault(path, [])
            if len(lines[path]) < 4:
                lines[path].append(rest[:160])
    return [(p, lines[p]) for p, _ in hits.most_common(12)]


def crate_map(crate):
    out = []
    for path in sorted(git("ls-files", f"crates/{crate}/src").split()):
        if not path.endswith(".rs"):
            continue
        first = ""
        for line in read(path, 3000).splitlines():
            if line.startswith("//!"):
                first = line[3:].strip()
                break
        out.append(f"- `{path}`" + (f": {first[:140]}" if first else ""))
    return out[:120]


def step_of(text):
    """The docs/gaps.md row the task is, by the issue's gaps_row line: (id, the row's cells)."""
    m = re.search(r"gaps_row:\s*([A-Z]+[0-9]+[a-z]?)", text)
    if not m:
        return None
    rid = m.group(1)
    for line in read("docs/gaps.md").splitlines():
        if re.match(rf"^\|\s*{rid}\s*\|", line):
            cells, cur, code = [], "", False
            for ch in line.strip():
                if ch == "`":
                    code = not code
                if ch == "|" and not code:
                    cells.append(cur.strip())
                    cur = ""
                else:
                    cur += ch
            # cells: "", the id, the problem, the state, what to do.
            return rid, [c for c in cells[2:] if c != ""]
    return None


def named_files(text):
    """The source files the task names, that exist, in the order named."""
    out = []
    for p in re.findall(r"`(crates/[A-Za-z0-9_./-]+\.rs)`", text):
        if os.path.exists(p) and p not in out:
            out.append(p)
    return out[:8]


def outline(path, only_public=False, limit=70):
    """Every item of a Rust file with its line: what exists and what is reachable."""
    out = []
    for n, line in enumerate(read(path).splitlines(), 1):
        if line.strip().startswith("#[cfg(test)]"):
            out.append(f"{n}: #[cfg(test)] (tests from here)")
            continue
        m = ITEM.match(line)
        if not m:
            continue
        public = bool(m.group(1))
        if only_public and not public:
            continue
        out.append(f"{n}: {line.strip()[:150]}" + ("" if public or m.group(5) in ("impl", "mod") else "   [private]"))
        if len(out) >= limit:
            out.append("…")
            break
    return out


def test_template(text, files):
    """The head of the file the new test goes in, or the nearest test file: a test to copy."""
    named = re.findall(r"Test:[^`]*`[^`]+`[^`]*`(crates/[A-Za-z0-9_./-]+\.rs)`", text)
    candidates = [p for p in named if os.path.exists(p)]
    # A named file with tests inside it (its own `#[cfg(test)] mod`): those are the template.
    for f in files:
        lines = read(f).splitlines()
        at = next((i for i, l in enumerate(lines) if l.strip().startswith("#[cfg(test)]")), None)
        if at is not None and any("#[test]" in l for l in lines[at:]):
            body = lines[at:]
            first = next((i for i, l in enumerate(body) if l.strip().startswith("#[test]") or l.strip().startswith("#[tokio::test")), 0)
            depth, opened, end = 0, False, len(body)
            for i in range(first, len(body)):
                depth += body[i].count("{") - body[i].count("}")
                opened = opened or "{" in body[i]
                if opened and depth <= 0 and i > first:
                    end = i + 1
                    break
            return f"{f} (its tests, from line {at + 1})", body[: min(end, 90)]
    # A file the task names that is itself a test file is the best template.
    candidates += [f for f in files if re.search(r"(^|/)(tests?|window_tests)/|tests\.rs$|_tests\.rs$", f)]
    for f in files:
        base = f[:-3]
        d = os.path.dirname(f)
        crate = "/".join(f.split("/")[:2])
        candidates += [f"{base}/tests.rs", *sorted(git("ls-files", f"{base}/").split()),
                       f"{d}/tests.rs"]
        candidates += sorted(git("ls-files", f"{crate}/tests").split())[:3]
    for c in candidates:
        if c.endswith(".rs") and ("test" in c) and os.path.exists(c):
            lines = read(c).splitlines()
            # Its imports and helpers, then the first whole test.
            end = len(lines)
            first = next((i for i, l in enumerate(lines) if l.strip().startswith("#[test]") or l.strip().startswith("#[tokio::test")), None)
            if first is not None:
                depth, opened = 0, False
                for i in range(first, len(lines)):
                    depth += lines[i].count("{") - lines[i].count("}")
                    opened = opened or "{" in lines[i]
                    if opened and depth <= 0:
                        end = i + 1
                        break
            return c, lines[: min(end, 90)]
    return None, []


def skills_clean():
    r = subprocess.run([sys.executable, "-I", "scripts/ai-skills-check.py"], capture_output=True, text=True)
    return r.returncode == 0, r.stdout.strip()


def main():
    text = task_text()
    # A step of docs/gaps.md: its own words decide the part of dm and the files.
    step = step_of(text)
    if step:
        text += "\n" + " ".join(step[1])
    areas = areas_of(text)
    crates = []
    for a in areas:
        for c in AREAS[a]["crates"]:
            if c not in crates:
                crates.append(c)
    clean, why = skills_clean()
    files = named_files(text)
    for f in files:
        c = f.split("/")[1]
        if c not in crates:
            crates.insert(0, c)
    main_crate = next((f.split("/")[1] for f in files), crates[0] if crates else "dm-core")
    out = ["# What this task is about (made by scripts/ai-context.py)", ""]
    if step:
        rid, cells = step
        out.append(f"## Your task: row {rid} of docs/gaps.md (do exactly this, nothing more)")
        out.append("")
        for label, cell in zip(("Problem", "State", "What to do"), cells):
            out.append(f"**{label}:** {cell}")
            out.append("")
    out.append("## How to work, in small steps")
    out.append("")
    out.append(f"1. Write the test first, where the task says (or beside the code, as in the template below).")
    out.append(f"2. After every edit, build: `cargo test -p {main_crate} --no-run`. Fix each compile error before anything else: call only what the outlines below say exists, and only what is public (or in the same module).")
    out.append(f"3. Run the test: `cargo nextest run -p {main_crate} -E 'test(~NAME)'`. For a fix, see it fail first.")
    out.append("4. Make the change, smallest first, and run the test again until it passes.")
    out.append("5. Then `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo xtask test --profile ci`.")
    out.append("")
    out.append(f"Part of dm: **{', '.join(areas)}** (crates: {', '.join(crates)}).")
    out.append("")
    out.append("## Read these first, in this order (the first wins)")
    out.append("")
    out.append("1. `docs/behaviour-contract.md`: never change or remove a row.")
    out.append("2. `AGENTS.md`, its \"Rules for a change\".")
    n = 3
    if clean:
        seen = set()
        # The task's own method first: an improvement follows improving.md.
        if os.environ.get("AI_TASK") == "improve" and os.path.exists("docs/ai-skills/improving.md"):
            out.append(f"{n}. `docs/ai-skills/improving.md` (how to improve dm unattended: this task)")
            seen.add("docs/ai-skills/improving.md")
            n += 1
        for a in areas:
            for s in AREAS[a]["skills"]:
                if s not in seen and os.path.exists(s):
                    out.append(f"{n}. `{s}`")
                    seen.add(s)
                    n += 1
        out.append(f"{n}. `docs/ai-skills/testing.md` and `docs/ai-notes.md` (what earlier runs learned).")
    else:
        out.append(f"{n}. (The skill pages are left out today: scripts/ai-skills-check.py found a problem.)")
        print(why, file=sys.stderr)
    out.append("")
    for f in files:
        out.append(f"## Outline of `{f}` (line: item; [private] cannot be called from another module)")
        out.append("")
        out.append("```")
        out.extend(outline(f))
        out.append("```")
        out.append("")
    where, template = test_template(text, files)
    if template:
        out.append(f"## A test to copy: the head of `{where}`")
        out.append("")
        out.append("```rust")
        out.extend(template)
        out.append("```")
        out.append("")
    if "dm-gpui" in crates or any("dm-gpui" in f for f in files):
        out.append("## The window test harness: its public methods (`crates/dm-gpui/src/harness.rs` and `harness/`)")
        out.append("")
        out.append("```")
        for h in ["crates/dm-gpui/src/harness.rs"] + sorted(git("ls-files", "crates/dm-gpui/src/harness").split()):
            if h.endswith(".rs"):
                out.extend(f"{h.rsplit('/', 1)[-1]}:{l}" for l in outline(h, only_public=True, limit=60))
        out.append("```")
        out.append("")
    files = related_files(names_in(text), crates)
    if files:
        out.append("## The files that mention this task's words")
        out.append("")
        for path, found in files:
            out.append(f"- `{path}`")
            for f in found:
                out.append(f"  - {f}")
        out.append("")
        out.append("## How such changes were written here (the last commits on those files)")
        out.append("")
        for path, _ in files[:6]:
            for line in git("log", "-3", "--format=%h %s", "--", path).splitlines():
                out.append(f"- `{path}`: {line[:150]}")
        out.append("")
    out.append("## How to test this part")
    out.append("")
    for a in areas:
        for t in AREAS[a]["tests"]:
            out.append(f"- `{t}`")
    out.append("- Everything, as CI runs it: `cargo xtask test --profile ci`")
    out.append("")
    for crate in crates[:2]:
        out.append(f"## Map of `crates/{crate}` (each file, its module comment)")
        out.append("")
        out.extend(crate_map(crate))
        out.append("")
    body = "\n".join(out)
    os.makedirs(".bot", exist_ok=True)
    with open(".bot/context.md", "w", encoding="utf-8") as f:
        f.write(body[:LIMIT])
    print(f"ai-context: {', '.join(areas)}; {len(files)} related files; skills {'given' if clean else 'left out'}")


if __name__ == "__main__":
    main()
