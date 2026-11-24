#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Whether the skills the bots read are clean: `python3 scripts/ai-skills-check.py`.

Run before any model reads docs/ai-skills/ or .claude/skills/ (scripts/ai-context.py) and in
CI. Fails (exit 1, each problem said) when:

- a file in docs/ai-skills/vendor/ is not in its MANIFEST.tsv, or no longer has the SHA-256
  written there (what was read and checked when it was taken is what stays);
- any file holds a hidden or direction-changing character (zero-width, bidirectional
  overrides, Unicode tags), with which a page can say one thing to a person and another to a
  model;
- any line is written to steer a model ("ignore previous instructions", "do not tell the
  user", "you are now …", a request to send or reveal a secret, a token or a key);
- a file is a link, executable, or of a kind a skill has no need of (only Markdown, CSV, TSV
  and licence texts);
- one of dm's own pages (docs/ai-skills/*.md, not vendor/) holds a web address: a page tells,
  it never sends a model somewhere.
"""

import hashlib
import os
import re
import stat
import sys

ROOTS = ["docs/ai-skills", ".claude/skills"]
VENDOR = "docs/ai-skills/vendor"
HIDDEN = re.compile("[​-‏‪-‮⁠-⁤⁦-⁩﻿\U000e0000-\U000e007f]")
STEERING = re.compile(
    r"ignore (all |any |the )?(previous|prior|above|earlier) (instructions|rules|prompts?)"
    r"|disregard (all |any |the )?(previous|prior|above|earlier|your)"
    r"|do not (tell|inform|show|mention (it |this )?to) the user"
    r"|without (telling|asking|informing) the user"
    r"|you are now (a |an |in )"
    r"|new instructions:"
    r"|(send|post|upload|reveal|print|exfiltrate|leak)[^.\n]{0,40}(secret|token|api[ _-]?key|password|credential)",
    re.I,
)
URL = re.compile(r"https?://", re.I)
KINDS = re.compile(r"(\.md|\.csv|\.tsv)$|(^|/)LICENSE[^/]*$")


def main():
    problems = []
    seen_vendor = set()
    manifest = {}
    try:
        with open(os.path.join(VENDOR, "MANIFEST.tsv"), encoding="utf-8") as f:
            for line in f.read().splitlines()[1:]:
                if line.strip():
                    path, digest = line.split("\t")[:2]
                    manifest[path] = digest
    except OSError as e:
        problems.append(f"{VENDOR}/MANIFEST.tsv: {e}")

    for root in ROOTS:
        if not os.path.isdir(root):
            continue
        for folder, dirs, files in os.walk(root):
            for name in dirs + files:
                path = os.path.join(folder, name)
                if os.path.islink(path):
                    problems.append(f"{path}: a symbolic link")
            for name in files:
                path = os.path.join(folder, name)
                if os.path.islink(path):
                    continue
                if os.stat(path).st_mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH):
                    problems.append(f"{path}: executable")
                rel = path.replace(os.sep, "/")
                if rel.endswith("/MANIFEST.tsv"):
                    continue
                if not KINDS.search(rel):
                    problems.append(f"{path}: not a kind of file a skill needs (Markdown, CSV, TSV, a licence)")
                    continue
                with open(path, "rb") as f:
                    data = f.read()
                if rel.startswith(VENDOR + "/"):
                    key = rel[len(VENDOR) + 1 :]
                    seen_vendor.add(key)
                    want = manifest.get(key)
                    if want is None:
                        problems.append(f"{path}: not in {VENDOR}/MANIFEST.tsv")
                    elif hashlib.sha256(data).hexdigest() != want:
                        problems.append(f"{path}: changed since it was checked (its SHA-256 is not the manifest's)")
                text = data.decode("utf-8", "replace")
                for n, line in enumerate(text.splitlines(), 1):
                    if HIDDEN.search(line):
                        problems.append(f"{path}:{n}: a hidden or direction-changing character")
                    if STEERING.search(line):
                        problems.append(f"{path}:{n}: a line written to steer a model: {line.strip()[:120]}")
                    if rel.startswith("docs/ai-skills/") and not rel.startswith(VENDOR + "/") and URL.search(line):
                        problems.append(f"{path}:{n}: a web address in one of dm's own pages")
    for key in manifest:
        if key not in seen_vendor:
            problems.append(f"{VENDOR}/{key}: in the manifest but missing")

    for p in problems:
        print(p)
    if problems:
        print(f"ai-skills-check: {len(problems)} problem(s): the skills are not given to a model.")
        return 1
    print("ai-skills-check: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
