#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Whether the skills the bots read are clean: `python3 scripts/ai-skills-check.py`.

Run before any model reads docs/ai-skills/ or .claude/skills/ (scripts/ai-context.py) and in
CI. Fails (exit 1, each problem said) when:

- a file in docs/ai-skills/vendor/ is not in its MANIFEST.tsv, or a skill copied into
  .claude/skills/ from outside is not in .claude/skills/UPSTREAM.tsv (any file in a copied
  skill's folder), or either no longer has the SHA-256 written there: what was read and
  checked when it was taken is what stays;
- any file holds a hidden or direction-changing character (zero-width, bidirectional
  overrides, Unicode tags), with which a page can say one thing to a person and another to a
  model;
- any line is written to steer a model ("ignore previous instructions", "do not tell the
  user", "you are now …", a request to send or reveal a secret, a token or a key);
- a file is a link, executable, or of a kind a skill has no need of (only Markdown, CSV, TSV
  and licence texts);
- one of dm's own pages (docs/ai-skills/*.md, not vendor/) holds a web address: a page tells,
  it never sends a model somewhere;
- `.mcp.json`, or `mcpServers` in `.claude/settings.json`, names a server that is not a program
  of this repository's own (gaps row H39): a remote registry, a URL, or a command fetched to run
  (`npx`, `uvx`, …) puts text that changes after review in front of every session and bot. A
  skill from outside comes in as a pinned copy (UPSTREAM.tsv), never as a server.
"""

import hashlib
import json
import os
import re
import stat
import sys

ROOTS = ["docs/ai-skills", ".claude/skills"]
VENDOR = "docs/ai-skills/vendor"
COPIED = ".claude/skills"  # its UPSTREAM.tsv names each file copied in from outside
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


def pins(path, problems):
    """path -> SHA-256, from a manifest whose first two columns they are."""
    out = {}
    try:
        with open(path, encoding="utf-8") as f:
            for line in f.read().splitlines()[1:]:
                if line.strip():
                    rel, digest = line.split("\t")[:2]
                    out[rel] = digest
    except OSError as e:
        problems.append(f"{path}: {e}")
    return out


MCP_FILES = [".mcp.json", ".claude/settings.json"]


def mcp_servers(problems):
    """Each MCP server the repository's own files name: only a program inside this repository,
    by a relative path to a file that is here, with no URL (gaps row H39)."""
    root = os.path.realpath(".")
    for path in MCP_FILES:
        if not os.path.isfile(path):
            continue
        try:
            with open(path, encoding="utf-8") as f:
                servers = (json.load(f) or {}).get("mcpServers") or {}
        except (OSError, ValueError) as e:
            problems.append(f"{path}: not read as JSON: {e}")
            continue
        if not isinstance(servers, dict):
            problems.append(f"{path}: mcpServers is not a table of servers")
            continue
        for name, server in servers.items():
            server = server if isinstance(server, dict) else {}
            command = str(server.get("command") or "")
            if server.get("url") or server.get("type") in ("http", "sse", "ws") or not command:
                problems.append(f"{path}: MCP server {name!r} is not a program of this repository")
                continue
            here = os.path.realpath(command)
            inside = not os.path.isabs(command) and here.startswith(root + os.sep)
            if not inside or not os.path.isfile(here):
                problems.append(
                    f"{path}: MCP server {name!r} runs {command!r}, not a file of this repository"
                )


def main():
    problems = []
    mcp_servers(problems)
    seen_vendor = set()
    seen_copied = set()
    manifest = pins(os.path.join(VENDOR, "MANIFEST.tsv"), problems)
    upstream = pins(os.path.join(COPIED, "UPSTREAM.tsv"), problems)
    copied_dirs = {k.split("/")[0] for k in upstream if "/" in k}

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
                if rel.endswith("/MANIFEST.tsv") or rel == f"{COPIED}/UPSTREAM.tsv":
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
                if rel.startswith(COPIED + "/"):
                    key = rel[len(COPIED) + 1 :]
                    if key in upstream:
                        seen_copied.add(key)
                        if hashlib.sha256(data).hexdigest() != upstream[key]:
                            problems.append(f"{path}: changed since it was copied and checked (not UPSTREAM.tsv's SHA-256)")
                    elif key.split("/")[0] in copied_dirs:
                        problems.append(f"{path}: in a skill copied from outside, but not in {COPIED}/UPSTREAM.tsv")
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
    for key in upstream:
        if key not in seen_copied:
            problems.append(f"{COPIED}/{key}: in UPSTREAM.tsv but missing")

    for p in problems:
        print(p)
    if problems:
        print(f"ai-skills-check: {len(problems)} problem(s): the skills are not given to a model.")
        return 1
    print("ai-skills-check: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
