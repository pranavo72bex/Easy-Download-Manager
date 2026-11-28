#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Whether a bot's change leaves EDM's window looking as it does on main (scripts/ai-merge.sh).

    python3 scripts/ai-looks-gate.py BRANCH-PICTURES MAIN-PICTURES [MAIN-PICTURES...]
    python3 scripts/ai-looks-gate.py --self-test

Each folder is a contract-linux.yml run's record/pictures (its `contract-looks` artifact):
the window pictured by human-qa.py for row B25 (empty-window, add-url, download-details), in
the same container on the same kind of runner. Row B25 holds a person's computer to the
maintainer's own pictures (docs/behaviour-baseline/), which CI's runner never matches; here
the change is held to main's own pictures instead, drawn where it is drawn. A bot may not
change how the window looks (CLAUDE.md), so a screen that differs from every one of main's
last runs by more than B25's own 1% (measured as human-qa.py measures it) stops the merge.

Prints each screen and its share of differing pixels; exits 1 when one differs, 0 when none
does, 2 when it cannot judge for want of main's pictures, 3 when the change's run pictured
nothing (a run to try again, not a look to judge). The pixels are
read by human-qa.py's own png_pixels and differing, taken from its source (it runs when
imported). Standard library only.
"""
import ast
import pathlib
import struct
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCREENS = ("empty-window", "add-url", "download-details")
LIMIT = 0.01  # B25's own: at most 1% of pixels differ


def _human_qa_pixels():
    """png_pixels and differing from human-qa.py, without running human-qa.py."""
    source = (ROOT / "scripts" / "human-qa.py").read_text(encoding="utf-8")
    tree = ast.parse(source)
    wanted = {"png_pixels", "differing"}
    nodes = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in wanted]
    if {n.name for n in nodes} != wanted:
        raise SystemExit("human-qa.py has no png_pixels and differing to measure with")
    space = {"struct": struct, "zlib": zlib, "pathlib": pathlib}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), "human-qa.py", "exec"), space)
    return space["differing"]


def judge(branch, mains, differing):
    """[(screen, share or None)], and whether it can judge at all."""
    mains = [m for m in mains if any((m / f"{s}.png").is_file() for s in SCREENS)]
    out = []
    for s in SCREENS:
        now = branch / f"{s}.png"
        was = [m / f"{s}.png" for m in mains if (m / f"{s}.png").is_file()]
        if not now.is_file() or not was:
            out.append((s, None))
            continue
        shares = [differing(w, now) for w in was]
        known = [x for x in shares if x is not None]
        out.append((s, min(known) if known else 1.0))
    return out, bool(mains)


def self_test():
    import tempfile

    def png(path, w, h, colour):
        raw = b"".join(b"\x00" + bytes(colour) * w for _ in range(h))
        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
        path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                         + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))

    differing = _human_qa_pixels()
    with tempfile.TemporaryDirectory() as t:
        t = pathlib.Path(t)
        for d, c in (("main", (10, 10, 10)), ("same", (10, 10, 10)), ("other", (200, 30, 30))):
            (t / d).mkdir()
            for s in SCREENS:
                png(t / d / f"{s}.png", 20, 10, c)
        out, ok = judge(t / "same", [t / "main"], differing)
        assert ok and all(x == 0 for _, x in out), out
        out, ok = judge(t / "other", [t / "main"], differing)
        assert ok and all(x is not None and x > LIMIT for _, x in out), out
        # Held to the closest of main's runs: one of them like the change is enough.
        out, _ = judge(t / "other", [t / "main", t / "other"], differing)
        assert all(x == 0 for _, x in out), out
        (t / "empty").mkdir()
        assert judge(t / "same", [t / "empty"], differing)[1] is False
        out, _ = judge(t / "empty", [t / "main"], differing)
        assert all(x is None for _, x in out), out
    print("ai-looks-gate.py self-test: ok")


def main(argv):
    if argv[1:] == ["--self-test"]:
        return self_test()
    if len(argv) < 3:
        sys.exit(__doc__)
    differing = _human_qa_pixels()
    out, can = judge(pathlib.Path(argv[1]), [pathlib.Path(a) for a in argv[2:]], differing)
    if not can:
        print("Cannot judge: main's runs left no pictures of the window.")
        return 2
    if all(x is None for _, x in out):
        print("Cannot judge: the change's run left no pictures of the window.")
        return 3
    changed = False
    for s, x in out:
        if x is None:
            print(f"- {s}: not pictured with the change")
            changed = True
        elif x > LIMIT:
            print(f"- {s}: {x * 100:.2f}% of pixels differ from main's (over {LIMIT * 100:.0f}%)")
            changed = True
        else:
            print(f"  {s}: as on main ({x * 100:.2f}% differ)")
    return 1 if changed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
