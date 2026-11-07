#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Keeps the pin of YouTube's challenge scripts current.

dm answers the signature and speed-limit checks of YouTube's player with two small scripts it
fetches once, pinned by version and checksum in desktop/crates/dm-core/src/youtube.rs
(SOLVER_VERSION, SOLVER_LIB, SOLVER_CORE). When YouTube changes its player the pinned scripts stop
matching and videos stop downloading or crawl; the cure is a newer release of the scripts and its
checksums. This script does that bookkeeping:

    python3 scripts/update-solver.py --check    # exit 0: current; 10: a newer release exists
    python3 scripts/update-solver.py --apply    # rewrite the pin to the latest release

--apply downloads both scripts of the latest release, refuses anything that is not a script, works
out their SHA-256 and edits the three constants; it changes nothing if anything fails. It never
decides that a release is good: a person runs a real YouTube video before merging the change (see
docs/decisions.md, "The YouTube challenge scripts stay fetched from their project's release page").

The project's address is read from youtube.rs, so it is named in one place only. Standard library
only. --api and --assets point it at a stand-in server (scripts/test-update-solver.py).
"""
import argparse, hashlib, json, pathlib, re, sys, urllib.error, urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "desktop/crates/dm-core/src/youtube.rs"
MAX_ASSET = 2 * 1024 * 1024
TIMEOUT = 30

VERSION = re.compile(r'(const SOLVER_VERSION: &str = ")([^"]+)(";)')
PAIR = r'(const {name}: \(&str, &str\) = \(\s*")([^"]+)(",\s*")([0-9a-f]{{64}})(",\s*\);)'
BASE = re.compile(r'format!\("(https://github\.com/([^/"]+)/([^/"]+)/releases/download)/\{SOLVER_VERSION\}"\)')


class Fail(Exception):
    pass


def read_pin(text):
    """(version, {name: (file, digest)}, download base, owner, repo) from youtube.rs's text."""
    version = VERSION.search(text)
    base = BASE.search(text)
    pairs = {}
    for name in ("SOLVER_LIB", "SOLVER_CORE"):
        found = re.search(PAIR.format(name=name), text)
        if not found:
            raise Fail(f"{name} not found in the source: its layout changed, fix this script")
        pairs[name] = (found.group(2), found.group(4))
    if not version or not base:
        raise Fail("SOLVER_VERSION or the release address not found in the source")
    return version.group(2), pairs, base.group(1), base.group(2), base.group(3)


def numeric(tag):
    """A release tag as a tuple of numbers; anything else is refused (it ends up in a branch name)."""
    tag = tag.strip()
    if tag.startswith("v"):
        tag = tag[1:]
    if not re.fullmatch(r"[0-9]+(\.[0-9]+){0,3}", tag):
        raise Fail(f"a release tag this script does not understand: {tag!r}")
    return tag, tuple(int(part) for part in tag.split("."))


def get(url, limit):
    request = urllib.request.Request(url, headers={"User-Agent": "dm-solver-update"})
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as answer:
            body = answer.read(limit + 1)
    except (urllib.error.URLError, OSError) as why:
        raise Fail(f"{url}: {why}")
    if len(body) > limit:
        raise Fail(f"{url}: more than {limit} bytes")
    return body


def latest_tag(api):
    body = get(api, 1024 * 1024)
    try:
        return json.loads(body)["tag_name"]
    except (ValueError, KeyError, TypeError):
        raise Fail(f"{api}: no tag_name in the answer")


def fetch_script(url):
    body = get(url, MAX_ASSET)
    head = body.lstrip()[:200].lower()
    if not body.strip() or head.startswith((b"<", b'{"message"')):
        raise Fail(f"{url}: not a script (an error page?)")
    return body


def rewrite(text, new_version, digests):
    text, hits = VERSION.subn(lambda m: m.group(1) + new_version + m.group(3), text, count=1)
    if hits != 1:
        raise Fail("SOLVER_VERSION not rewritten")
    for name, digest in digests.items():
        text, hits = re.subn(PAIR.format(name=name),
                             lambda m, d=digest: m.group(1) + m.group(2) + m.group(3) + d + m.group(5), text, count=1)
        if hits != 1:
            raise Fail(f"{name} not rewritten")
    return text


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--apply", action="store_true")
    parser.add_argument("--source", type=pathlib.Path, default=SOURCE)
    parser.add_argument("--api", help="the releases answer to read the latest tag from (default: derived from the source)")
    parser.add_argument("--assets", help="the download address to fetch the scripts from (default: derived from the source)")
    parser.add_argument("--summary-file", type=pathlib.Path, help="write a pull request description here")
    args = parser.parse_args(argv)

    try:
        text = args.source.read_text(encoding="utf-8")
        version, pairs, base, owner, repo = read_pin(text)
        api = args.api or f"https://api.github.com/repos/{owner}/{repo}/releases/latest"
        new_tag, new_number = numeric(latest_tag(api))
        _, old_number = numeric(version)
        if new_number <= old_number:
            print(f"current: pinned {version}, latest {new_tag}")
            return 0
        if args.check:
            print(f"a newer release exists: pinned {version}, latest {new_tag}")
            return 10
        download = args.assets or base
        digests = {}
        for name, (file, _) in pairs.items():
            digests[name] = hashlib.sha256(fetch_script(f"{download}/{new_tag}/{file}")).hexdigest()
        args.source.write_text(rewrite(text, new_tag, digests), encoding="utf-8")
        print(f"updated: {version} -> {new_tag}")
        if args.summary_file:
            lines = [f"The YouTube challenge scripts have a newer release: **{version}** -> **{new_tag}**.", ""]
            for name, (file, old) in pairs.items():
                lines.append(f"- `{file}`: `{old}` -> `{digests[name]}`")
            lines += ["", f"Changes of the release: https://github.com/{owner}/{repo}/compare/{version}...{new_tag}", "",
                      "Opened by `scripts/update-solver.py` and never merged by it. Before merging, run a real YouTube",
                      "video with this change (`SITES=youtube python3 scripts/real-sites.py`, or a download in the app):",
                      "the offline tests do not use the real scripts."]
            args.summary_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return 0
    except Fail as why:
        print(f"error: {why}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
