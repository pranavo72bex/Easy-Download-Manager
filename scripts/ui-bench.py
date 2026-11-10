#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What dm's window costs, measured through the control port only (gaps row G5).

    python3 scripts/ui-bench.py [--rows 100,1000,10000] [--runs 3] [--idle 60] [BINARY]

Talks to the window as `human-qa.py` does, over `--control`, so a GPUI window is measured
the same way as the egui one. For each list size, a home is seeded once with that many rows
(added for later, against a local server), then each run starts dm on a copy of it and takes:

  start     launch to the first `ui.texts` answer that has words in it (a drawn frame)
  idle      the process's CPU over `--idle` seconds with nothing happening, from `ps`'s
            CPU time, in percent of one core
  memory    resident size after the idle spell (and the macOS footprint where `footprint` is)
  scroll    a `ui.scroll` over the top row until the words on screen change
  find      Find (Cmd-F / Ctrl-F), the last row's name typed, Find next, until it is selected
  busy      the CPU over 30 s with 20 downloads running at 1 MB/s each from the local server

Each figure is the median of the runs. A release build is what the numbers are for
(`cargo build --release -p dm-desktop`); BINARY or DM_BINARY names it, else the release
build, else the debug one. Needs a display. Prints a Markdown table for
docs/migration/GPUI_TEST_PLAN.md.
"""

import argparse
import importlib.util
import json
import os
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
spec = importlib.util.spec_from_file_location("stress_check", HERE / "stress-check.py")
sc = importlib.util.module_from_spec(spec)
sys.modules["stress_check"] = sc
spec.loader.exec_module(sc)

MB = 1024 * 1024


def binary(named):
    for candidate in (named, os.environ.get("DM_BINARY"),
                      ROOT / "desktop/target/release/dm-desktop",
                      ROOT / "desktop/target/debug/dm-desktop"):
        if candidate and pathlib.Path(candidate).exists():
            return pathlib.Path(candidate)
    sys.exit("no dm binary: build one, or name it")


def cpu_seconds(pid):
    """The process's CPU time so far, in seconds."""
    if sys.platform == "win32":
        out = subprocess.run(["powershell", "-NoProfile", "-Command",
                              f"(Get-Process -Id {pid}).TotalProcessorTime.TotalSeconds"],
                             capture_output=True, text=True).stdout.strip()
        return float(out or 0)
    out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    # [[dd-]hh:]mm:ss[.ss]
    days, _, clock = out.rpartition("-")
    parts = [float(p) for p in clock.split(":")]
    seconds = 0.0
    for p in parts:
        seconds = seconds * 60 + p
    return seconds + (float(days) * 86400 if days else 0)


def resident_mb(pid):
    rss = sc.proc_stats(pid)[0]
    footprint = None
    if sys.platform == "darwin" and shutil.which("footprint"):
        out = subprocess.run(["footprint", "-p", str(pid)], capture_output=True, text=True).stdout
        for line in out.splitlines():
            if "phys_footprint:" in line:
                amount, unit = line.split("phys_footprint:")[1].split()[:2]
                footprint = float(amount) * {"KB": 2**-10, "MB": 1, "GB": 2**10}.get(unit, 1)
    return rss, footprint


def texts(dm):
    r = dm.call("ui.texts")
    return [t["text"] for t in r.get("result", {}).get("texts", [])] if r.get("ok") else []


def until(what, seconds=60, step=0.02):
    deadline = time.time() + seconds
    while time.time() < deadline:
        got = what()
        if got:
            return got
        time.sleep(step)
    return None


def seed(root, base, rows):
    """A home holding `rows` rows added for later; its folder, to copy for each run."""
    dm = sc.DM(root, name=f"seed{rows}").start()
    try:
        for k in range(0, rows, 500):
            n = min(500, rows - k)
            r = dm.call("downloads.add", urls=[f"{base}/f/b{i}/1024/bench{i:05}.bin" for i in range(k, k + n)],
                        later=True)
            assert r.get("ok"), r
        assert until(lambda: len(dm.rows()) >= rows, 120), f"only {len(dm.rows())} of {rows} rows"
    finally:
        dm.stop()
    return dm.home, dm.out


def run_once(root, base, home, out, rows, idle, run):
    name = f"bench{rows}-{run}"
    dm = sc.DM(root, name=name, window=True)
    shutil.rmtree(dm.home, ignore_errors=True)
    shutil.copytree(home, dm.home)
    for stale in ("control.port", "control.token", "instance.lock"):
        (dm.home / stale).unlink(missing_ok=True)
    took = {}
    started = time.time()
    dm.start()
    try:
        assert until(lambda: texts(dm), 120), "the window never drew"
        took["start"] = time.time() - started
        time.sleep(3)
        before = cpu_seconds(dm.proc.pid)
        time.sleep(idle)
        took["idle"] = 100 * (cpu_seconds(dm.proc.pid) - before) / idle
        took["rss"], took["footprint"] = resident_mb(dm.proc.pid)

        shown = texts(dm)
        first = next((t for t in shown if t.startswith("bench")), None)
        if first:
            t = time.time()
            dm.call("ui.scroll", text=first, lines=10)
            until(lambda: texts(dm) != shown, 20)
            took["scroll"] = time.time() - t

        last = f"bench{rows - 1:05}"
        ids = [r["id"] for r in dm.rows() if r["url"].endswith(f"/{last}.bin")]
        t = time.time()
        dm.call("ui.key", key="F", modifiers=["cmd" if sys.platform == "darwin" else "ctrl"])
        until(lambda: any(w == "Find next" for w in texts(dm)), 20)
        dm.call("ui.type", text=last)
        dm.call("ui.click", text="Find next")
        until(lambda: dm.call("ui.inspect").get("result", {}).get("selected") == ids, 30)
        took["find"] = time.time() - t
        dm.call("ui.key", key="Escape")

        dm.add([f"{base}/f/busy{run}-{i}/{200 * MB}/busy{i}.bin?rate={MB}" for i in range(20)])
        time.sleep(5)
        before = cpu_seconds(dm.proc.pid)
        time.sleep(30)
        took["busy"] = 100 * (cpu_seconds(dm.proc.pid) - before) / 30
    finally:
        dm.stop(kill=True)
        shutil.rmtree(dm.home, ignore_errors=True)
        shutil.rmtree(dm.out, ignore_errors=True)
    return took


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary", nargs="?")
    ap.add_argument("--rows", default="100,1000,10000")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--idle", type=int, default=60)
    args = ap.parse_args()
    sc.BINARY = binary(args.binary)
    print(f"binary: {sc.BINARY}", flush=True)
    server, base = sc.start_server()
    root = pathlib.Path(tempfile.mkdtemp(prefix="dm-ui-bench-"))
    results = {}
    try:
        for rows in [int(r) for r in args.rows.split(",")]:
            home, out = seed(root, base, rows)
            runs = []
            for run in range(args.runs):
                took = run_once(root, base, home, out, rows, args.idle, run)
                print(f"  {rows} rows, run {run + 1}: " + json.dumps({k: round(v, 3) if v is not None else None
                                                                      for k, v in took.items()}), flush=True)
                runs.append(took)
            results[rows] = {k: statistics.median([r[k] for r in runs if r.get(k) is not None])
                             for k in runs[0] if any(r.get(k) is not None for r in runs)}
    finally:
        for dm in list(sc.LIVE_DMS):
            dm.stop(kill=True)
        server.shutdown()
        shutil.rmtree(root, ignore_errors=True)

    print("\n| Rows | Start to first frame | Idle CPU | Resident | Footprint | Scroll | Find | 20 downloads CPU |")
    print("|---|---|---|---|---|---|---|---|")
    for rows, m in results.items():
        cell = lambda k, f: f.format(m[k]) if k in m else "—"
        print(f"| {rows:,} | {cell('start', '{:.2f} s')} | {cell('idle', '{:.1f} %')} | {cell('rss', '{:.0f} MB')} | "
              f"{cell('footprint', '{:.0f} MB')} | {cell('scroll', '{:.2f} s')} | {cell('find', '{:.2f} s')} | "
              f"{cell('busy', '{:.1f} %')} |")


if __name__ == "__main__":
    main()
