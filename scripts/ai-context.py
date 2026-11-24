#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What a free model is handed before it works: `python3 scripts/ai-context.py [FILE]`.

Writes .bot/context.md from what the task says (.bot/issue.json, .bot/failing-tests.txt,
.bot/row.md, or the FILE a nightly test run is for), so a model that would not find its way
alone starts with:

1. which part of dm the task is about (the desktop window, the phone, the engine, the
   browser extension, video pages), from its words and the crates its tests name;
2. the skill pages to read for that part (docs/ai-skills/, .claude/skills/), in the order
   they win, only when scripts/ai-skills-check.py finds them clean;
3. a map of the crates concerned: each file with the first line of its module comment;
4. the files that mention the task's own words and names, with the lines;
5. the last commits on those files (how such changes are written here);
6. how to run the tests for that part.

The task's words are only searched for, as fixed strings (git grep -F), never run.
Standard library and git only.
"""

import json
import os
import re
import subprocess
import sys
from collections import Counter

LIMIT = 45000
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
        parts.append(sys.argv[1])
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


def skills_clean():
    r = subprocess.run([sys.executable, "-I", "scripts/ai-skills-check.py"], capture_output=True, text=True)
    return r.returncode == 0, r.stdout.strip()


def main():
    text = task_text()
    areas = areas_of(text)
    crates = []
    for a in areas:
        for c in AREAS[a]["crates"]:
            if c not in crates:
                crates.append(c)
    clean, why = skills_clean()
    out = ["# What this task is about (made by scripts/ai-context.py)", ""]
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
