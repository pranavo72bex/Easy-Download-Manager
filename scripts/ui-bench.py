#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What dm's window costs, measured through the control port only (gaps rows G5, 187, 188).

    python3 scripts/ui-bench.py [--rows 100,10000] [--runs 3] [--idle 60]
                                [--only start,idle,list,busy,many,transfer]
                                [--json FILE] [--save-baseline] [BINARY]

Talks to the window as `human-qa.py` does, over `--control`, so the GPUI window and the egui
one are measured the same way. For each list size a home is seeded once with that many rows
(added for later, against a local server); each run then starts dm on a copy of it and takes,
group by group (`--only` picks groups; all of them by default):

  start    cold   launch to the first `ui.texts` answer with words in it (a drawn frame), after
                  the system's file cache is emptied: `purge` (macOS) or `drop_caches` (Linux)
                  where sudo needs no password, else dm's own binary evicted from the cache (the
                  system's libraries then stay cached; the JSON says which was done). Not on
                  Windows, where only a restart empties it
           start  the same, started again at once (warm)
  idle     idle   the process's CPU over `--idle` seconds with nothing happening, in percent of
                  one core, from the CPU time the system counts
           rss    resident size after the idle spell; footprint, the macOS physical footprint
  list     scroll one 10-line scroll over the top row, until other rows are on screen
           page   the list paged through from top to bottom in about 50 scrolls: the median
                  scroll's answer (page) and the worst (page_worst)
           sort   a click on each column's heading, until it shows its arrow: the median
                  heading (sort) and the slowest (sort_worst)
           filter the last row's name typed into the search box (Find, Cmd-F or Ctrl-F), until
                  the list shows that row alone; find, then Find next (F3) until it is chosen;
                  unfilter, Escape in the box until the whole list is back. The egui window's
                  Find dialog is driven as before, and gives find only
  busy     busy   the CPU over 30 s with 20 downloads queued at 1 MB/s each (dm runs as many at
                  once as its setting allows, 3 by default)
           busy_page, busy_page_worst: with `list` too, the list paged through in a dm started
                  again with 60 downloads at 1 MB/s running at once
  idle     idle_frames: how many times the window was drawn in the idle spell (GPUI's
                  `ui.frames`), none where nothing changed
  many     300 downloads added at once at 256 KB/s each, 1 MB apiece, 10 at a time (dm's
           most): many_answer and many_worst, the window's answer to `ui.texts` over 20 asks
           while the queue turns over; many_cpu over 15 s; how many finished meanwhile
  transfer one 512 MB file at full speed from the local server, 8 connections: its seconds
           (transfer_window), dm's CPU seconds (transfer_window_cpu) and the window's answer
           meanwhile (transfer_answer, transfer_worst); then the same file with `--headless`
           (transfer_headless, transfer_headless_cpu), so the window's cost to the engine is
           the difference. The local server is Python: past a few hundred MB/s it, not dm,
           sets the speed

Every figure is lower-is-better. Each is the median of the runs, printed with the spread (the
fastest and slowest run). The results, with what produced them (`provenance.py`: commit, rustc,
target, profile, binary size, CPU, GPU, display scale, the rows), are written as JSON
(`--json`, else desktop/target/ui-bench/<time>.json) and compared with the stored baseline for
this system, scripts/ui-bench-baselines/<system>-<machine>.json, where there is one;
`--save-baseline` makes this run the baseline. Numbers taken while other work runs on the
computer are upper bounds: say so where they are written.

A release build is what the numbers are for (`cargo build --release -p dm-gpui`); BINARY or
DM_BINARY names it, else the release build of dm-gpui, else of dm-desktop, else a debug one.
Needs a display. Frame times are not taken here: neither window reports them over the control
port (docs/testing.md, Profiling, says how to take them with the system's profiler).
"""

import argparse
import ctypes
import ctypes.util
import importlib.util
import json
import math
import os
import pathlib
import platform
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
spec = importlib.util.spec_from_file_location("provenance", HERE / "provenance.py")
prov = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prov)

MB = 1024 * 1024
GROUPS = ("start", "idle", "list", "busy", "many", "transfer")
BASELINES = HERE / "ui-bench-baselines"
# A wheel's line, as the GPUI window turns one (`ui.scroll`'s `lines`).
LINE_PX = 40
# What each figure is, for the table: its label and its unit.
METRICS = {
    "cold": ("Start, file cache emptied", "s"),
    "start": ("Start, warm", "s"),
    "idle": ("Idle CPU", "%"),
    "idle_frames": ("Frames drawn while idle", ""),
    "rss": ("Resident", "MB"),
    "footprint": ("Footprint", "MB"),
    "scroll": ("One scroll", "s"),
    "page": ("Paging through, median scroll", "s"),
    "page_worst": ("Paging through, worst scroll", "s"),
    "sort": ("Sort by a heading, median", "s"),
    "sort_worst": ("Sort by a heading, slowest", "s"),
    "filter": ("Search box filter", "s"),
    "find": ("Find next", "s"),
    "unfilter": ("Search box emptied", "s"),
    "busy": ("CPU, 20 downloads", "%"),
    "busy_page": ("Paging, 60 downloading, median", "s"),
    "busy_page_worst": ("Paging, 60 downloading, worst", "s"),
    "many_answer": ("Answer, 300 downloads, median", "s"),
    "many_worst": ("Answer, 300 downloads, worst", "s"),
    "many_cpu": ("CPU, 300 downloads", "%"),
    "transfer_window": ("512 MB with the window", "s"),
    "transfer_window_cpu": ("CPU seconds, with the window", "s"),
    "transfer_answer": ("Answer at full speed, median", "s"),
    "transfer_worst": ("Answer at full speed, worst", "s"),
    "transfer_headless": ("512 MB, --headless", "s"),
    "transfer_headless_cpu": ("CPU seconds, --headless", "s"),
}


def binary(named):
    for candidate in (named, os.environ.get("DM_BINARY"),
                      ROOT / "desktop/target/release/dm-gpui",
                      ROOT / "desktop/target/release/dm-desktop",
                      ROOT / "desktop/target/debug/dm-gpui",
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


def evict(path):
    """Empty the system's file cache before a cold start, as far as can be done without a
    password; says how far, or None where nothing could be done."""
    if sys.platform == "darwin":
        if subprocess.run(["sudo", "-n", "purge"], capture_output=True).returncode == 0:
            return "purge"
        # Without root: dm's own pages dropped. A shared mapping synced with MS_INVALIDATE
        # leaves none of the file in the cache (mincore says so, and its next read is the
        # disk's, twice as slow).
        libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
        libc.mmap.restype = ctypes.c_void_p
        libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                              ctypes.c_long]
        libc.msync.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
        libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        size = os.path.getsize(path)
        fd = os.open(path, os.O_RDONLY)
        try:
            at = libc.mmap(None, size, 1, 1, fd, 0)       # PROT_READ, MAP_SHARED
            if at in (None, ctypes.c_void_p(-1).value):
                return None
            ok = libc.msync(at, size, 2) == 0              # MS_INVALIDATE
            libc.munmap(at, size)
        finally:
            os.close(fd)
        return "binary evicted" if ok else None
    if sys.platform.startswith("linux"):
        if subprocess.run(["sudo", "-n", "sh", "-c", "sync; echo 3 > /proc/sys/vm/drop_caches"],
                          capture_output=True).returncode == 0:
            return "drop_caches"
        fd = os.open(path, os.O_RDONLY)
        try:
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        finally:
            os.close(fd)
        return "binary evicted"
    return None


def drawn(dm):
    """The words on screen, with where they are."""
    r = dm.call("ui.texts")
    return r.get("result", {}).get("texts", []) if r.get("ok") else []


def texts(dm):
    return [t["text"] for t in drawn(dm)]


def names(dm):
    """The seeded rows' names on screen, top to bottom: what a scroll changes, and a
    download's progress does not."""
    return [t["text"] for t in sorted(drawn(dm), key=lambda t: t["rect"][1]) if t["text"].startswith("bench")]


def until(what, seconds=60, step=0.02):
    deadline = time.time() + seconds
    while time.time() < deadline:
        got = what()
        if got:
            return got
        time.sleep(step)
    return None


def answers(dm, asks, gap):
    """How long the window takes to answer `ui.texts`, `asks` times, `gap` seconds apart."""
    took = []
    for _ in range(asks):
        t = time.time()
        dm.call("ui.texts")
        took.append(time.time() - t)
        time.sleep(gap)
    return took


def seed(root, base, rows):
    """A home holding `rows` rows added for later, to copy for each run."""
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
    return dm.home


def fresh(root, home, name, settings=None, window=True):
    """A dm on a copy of the seeded home, not yet started."""
    dm = sc.DM(root, settings, name=name, window=window)
    shutil.rmtree(dm.home, ignore_errors=True)
    shutil.copytree(home, dm.home)
    for stale in ("control.port", "control.token", "instance.lock"):
        (dm.home / stale).unlink(missing_ok=True)
    return dm


def finish(dm):
    dm.stop(kill=True)
    shutil.rmtree(dm.home, ignore_errors=True)
    shutil.rmtree(dm.out, ignore_errors=True)


def first_frame(dm):
    started = time.time()
    dm.start()
    assert until(lambda: texts(dm), 120), "the window never drew"
    return time.time() - started


def frames(dm):
    """How many times the window has been drawn, where it says (`ui.frames`, answered without
    drawing); None where it does not."""
    r = dm.call("ui.frames")
    return r["result"]["frames"] if r.get("ok") else None


def to_top(dm, rows):
    """The list scrolled back to its first row, by the wheel: a key would go to whatever has
    the keyboard, the search box included."""
    shown = names(dm)
    if shown:
        dm.call("ui.scroll", text=shown[0], lines=-(rows + 10))
    def settled():
        was = names(dm)
        time.sleep(0.2)
        return was == names(dm)
    until(settled, 20)


def page_through(dm, rows):
    """The list scrolled from its top to its end in about 50 steps, each timed until other
    rows are on screen; the times, and whether its end was reached (a scroll that moves
    nothing)."""
    to_top(dm, rows)
    seeded = sorted((t for t in drawn(dm) if t["text"].startswith("bench")), key=lambda t: t["rect"][1])
    if len(seeded) < 2:
        return [], False
    row_px = max(1.0, seeded[1]["rect"][1] - seeded[0]["rect"][1])
    total_px = row_px * rows
    step_px = max(row_px * len(seeded), total_px / 50)
    lines = max(1.0, math.floor(step_px / LINE_PX))
    took = []
    for _ in range(int(math.ceil(total_px / (lines * LINE_PX))) + 2):
        before = names(dm)
        if not before:
            break
        t = time.time()
        dm.call("ui.scroll", text=before[len(before) // 2], lines=lines)
        if not until(lambda: names(dm) != before, 3, 0.005):
            return took, True                       # the end: nothing more moves
        took.append(time.time() - t)
    return took, False


def sort_each(dm):
    """A click on each heading on screen, timed until it shows its arrow."""
    headings = ["File Name", "Size", "Status", "Time left", "Transfer rate", "Last Try Date",
                "Description", "Referrer", "Folder"]
    took = {}
    shown = texts(dm)
    for heading in [h for h in headings if h in shown]:
        t = time.time()
        if not dm.call("ui.click", text=heading).get("ok"):
            continue
        if until(lambda: f"{heading} ▲" in texts(dm), 20, 0.005):
            took[heading] = time.time() - t
    return took


def filter_and_find(dm, rows, took):
    last = f"bench{rows - 1:05}"
    ids = [r["id"] for r in dm.rows() if r["url"].endswith(f"/{last}.bin")]
    mod = "cmd" if sys.platform == "darwin" else "ctrl"

    def chosen():
        return dm.call("ui.inspect").get("result", {}).get("selected") == ids

    t = time.time()
    pressed = dm.call("ui.key", key="F", modifiers=[mod])
    if not pressed.get("ok"):
        # A window with no keys says so, and nothing is measured.
        print(f"  find: {pressed.get('error')}", flush=True)
        return
    # The egui window opens a Find dialog; the GPUI window puts the keyboard in its search
    # box, which filters the list as it is typed into.
    if until(lambda: "Find next" in texts(dm), 1.5):
        dm.call("ui.type", text=last)
        dm.call("ui.click", text="Find next")
        if until(chosen, 30):
            took["find"] = time.time() - t
        dm.call("ui.key", key="Escape")
        return
    whole = len(names(dm))
    t = time.time()
    dm.call("ui.type", text=last)
    if until(lambda: names(dm) == [f"{last}.bin"], 30, 0.005):
        took["filter"] = time.time() - t
    t = time.time()
    dm.call("ui.key", key="F3")
    if until(chosen, 30, 0.005):
        took["find"] = time.time() - t
    # Escape in the box empties it and gives the keyboard back to the list.
    t = time.time()
    dm.call("ui.key", key="Escape")
    if until(lambda: len(names(dm)) >= whole, 30, 0.005):
        took["unfilter"] = time.time() - t


def file_done(path, size):
    """A download is done when its file is in the folder whole: dm stages the parts apart
    and renames them in at the end. Asking the queue instead would cost the window a
    10,000-row answer each time."""
    try:
        return path.stat().st_size == size
    except OSError:
        return False


def transfer(root, base, home, rows, run, window, took):
    size = 512 * MB
    key = "window" if window else "headless"
    dm = fresh(root, home, f"tx{key}{rows}-{run}", {"connections": 8}, window=window)
    try:
        dm.start()
        if window:
            until(lambda: texts(dm), 120)
        time.sleep(2)
        name = f"tx{key}{run}.bin"
        before = cpu_seconds(dm.proc.pid)
        t = time.time()
        dm.add([f"{base}/f/tx{key}{rows}-{run}/{size}/{name}"])
        asked = []
        while not file_done(dm.out / name, size) and time.time() - t < 300:
            if window:
                a = time.time()
                dm.call("ui.texts")
                asked.append(time.time() - a)
            time.sleep(0.05)
        if file_done(dm.out / name, size):
            took[f"transfer_{key}"] = time.time() - t
            took[f"transfer_{key}_cpu"] = cpu_seconds(dm.proc.pid) - before
        if asked:
            took["transfer_answer"] = statistics.median(asked)
            took["transfer_worst"] = max(asked)
    finally:
        finish(dm)


def run_once(root, base, home, rows, idle, run, groups):
    took = {"notes": {}}
    if "start" in groups and sys.platform != "win32":
        dm = fresh(root, home, f"cold{rows}-{run}")
        try:
            how = evict(sc.BINARY)
            if how:
                took["cold"] = first_frame(dm)
                took["notes"]["cold"] = how
        finally:
            finish(dm)
    dm = fresh(root, home, f"bench{rows}-{run}")
    try:
        took["start"] = first_frame(dm)
        time.sleep(3)
        if "idle" in groups:
            before = cpu_seconds(dm.proc.pid)
            drawn_before = frames(dm)
            time.sleep(idle)
            took["idle"] = 100 * (cpu_seconds(dm.proc.pid) - before) / idle
            # Nothing changed, so nothing is drawn (the GPUI window counts its frames, gaps
            # row 181; the egui window does not say).
            drawn_after = frames(dm)
            if drawn_before is not None and drawn_after is not None:
                took["idle_frames"] = drawn_after - drawn_before
            took["rss"], took["footprint"] = resident_mb(dm.proc.pid)

        if "list" in groups:
            before = names(dm)
            if before:
                t = time.time()
                dm.call("ui.scroll", text=before[0], lines=10)
                if until(lambda: names(dm) != before, 20, 0.005):
                    took["scroll"] = time.time() - t
            steps, reached = page_through(dm, rows)
            if steps:
                took["page"], took["page_worst"] = statistics.median(steps), max(steps)
                took["notes"]["page"] = f"{len(steps)} scrolls, the end {'reached' if reached else 'not reached'}"
            sorted_by = sort_each(dm)
            if sorted_by:
                took["sort"], took["sort_worst"] = statistics.median(sorted_by.values()), max(sorted_by.values())
                took["notes"]["sort"] = {k: round(v, 3) for k, v in sorted_by.items()}
            filter_and_find(dm, rows, took)

        if "busy" in groups:
            dm.add([f"{base}/f/busy{rows}-{run}-{i}/{200 * MB}/busy{i}.bin?rate={MB}" for i in range(20)])
            time.sleep(5)
            before = cpu_seconds(dm.proc.pid)
            time.sleep(30)
            took["busy"] = 100 * (cpu_seconds(dm.proc.pid) - before) / 30
    finally:
        finish(dm)

    if "busy" in groups and "list" in groups:
        # Paged through while 60 downloads update at once (gaps row 182).
        dm = fresh(root, home, f"busy60-{rows}-{run}", {"concurrency": 60})
        try:
            first_frame(dm)
            dm.add([f"{base}/f/busy60-{rows}-{run}-{i}/{200 * MB}/busy{i}.bin?rate={MB}" for i in range(60)])
            time.sleep(5)
            steps, _ = page_through(dm, rows)
            if steps:
                took["busy_page"], took["busy_page_worst"] = statistics.median(steps), max(steps)
        finally:
            finish(dm)

    if "many" in groups:
        dm = fresh(root, home, f"many{rows}-{run}", {"concurrency": 10, "connections": 1})
        try:
            dm.start()
            until(lambda: texts(dm), 120)
            got = dm.add([f"{base}/f/many{rows}-{run}-{i}/{MB}/many{i}.bin?rate={256 * 1024}" for i in range(300)])
            assert got.get("ok"), got
            time.sleep(5)
            asked = answers(dm, 20, 0.25)
            took["many_answer"], took["many_worst"] = statistics.median(asked), max(asked)
            before = cpu_seconds(dm.proc.pid)
            time.sleep(15)
            took["many_cpu"] = 100 * (cpu_seconds(dm.proc.pid) - before) / 15
            took["notes"]["many_done"] = len([p for p in dm.out.glob("many*.bin") if file_done(p, MB)])
        finally:
            finish(dm)

    if "transfer" in groups:
        transfer(root, base, home, rows, run, True, took)
        transfer(root, base, home, rows, run, False, took)
    return took


def summarise(runs):
    """Each figure's median, fastest and slowest over the runs that have it."""
    out = {}
    for key in METRICS:
        values = [r[key] for r in runs if r.get(key) is not None]
        if values:
            out[key] = {"median": statistics.median(values), "min": min(values), "max": max(values),
                        "runs": values}
    return out


def baseline_path():
    machine = platform.machine().lower().replace("aarch64", "arm64").replace("amd64", "x86_64")
    return BASELINES / f"{sys.platform}-{machine}.json"


def table(results, baseline):
    """The figures, with their spread and the change from the baseline."""
    lines = ["| Rows | Figure | Median | Spread | Baseline | Change |", "|---|---|---|---|---|---|"]
    base = (baseline or {}).get("results", {})
    for rows, figures in results.items():
        for key, m in figures.items():
            label, unit = METRICS[key]
            digits = 3 if unit == "s" else 1

            def fmt(v):
                return f"{v:.{digits}f} {unit}"
            was = base.get(str(rows), {}).get(key, {}).get("median")
            change = f"{100 * (m['median'] - was) / was:+.0f}%" if was else "—"
            lines.append(f"| {rows:,} | {label} | {fmt(m['median'])} | {fmt(m['min'])} to {fmt(m['max'])} | "
                         f"{fmt(was) if was is not None else '—'} | {change} |")
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary", nargs="?")
    ap.add_argument("--rows", default="100,10000")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--idle", type=int, default=60)
    ap.add_argument("--only", default=",".join(GROUPS), help="groups: " + ",".join(GROUPS))
    ap.add_argument("--json", help="where the results go (default desktop/target/ui-bench/<time>.json)")
    ap.add_argument("--save-baseline", action="store_true", help="make this run the stored baseline")
    args = ap.parse_args()
    groups = {g.strip() for g in args.only.split(",") if g.strip()}
    unknown = groups - set(GROUPS)
    if unknown:
        sys.exit(f"no such group: {', '.join(sorted(unknown))}")
    sc.BINARY = binary(args.binary)
    sizes = [int(r) for r in args.rows.split(",")]
    made_by = prov.provenance(sc.BINARY, {"rows": sizes, "runs": args.runs, "idle_s": args.idle,
                                          "groups": sorted(groups)})
    print(f"binary: {sc.BINARY}\n{prov.line(made_by)}", flush=True)
    server, base = sc.start_server()
    root = pathlib.Path(tempfile.mkdtemp(prefix="dm-ui-bench-"))
    results, every = {}, {}
    try:
        for rows in sizes:
            home = seed(root, base, rows)
            runs = []
            for run in range(args.runs):
                # The local server's ceiling on what it serves holds for each run, not the whole
                # bench: without this the later runs' downloads were cut off.
                sc.Stats.reset()
                took = run_once(root, base, home, rows, args.idle, run, groups)
                print(f"  {rows} rows, run {run + 1}: " + json.dumps(
                    {k: (round(v, 3) if isinstance(v, float) else v) for k, v in took.items()}), flush=True)
                runs.append(took)
            results[rows] = summarise(runs)
            every[rows] = runs
    finally:
        for dm in list(sc.LIVE_DMS):
            dm.stop(kill=True)
        server.shutdown()
        shutil.rmtree(root, ignore_errors=True)

    stored = baseline_path()
    baseline = json.loads(stored.read_text()) if stored.exists() else None
    print(f"\nBaseline: {stored.relative_to(ROOT) if baseline else 'none for this system'}"
          + (f" ({prov.line(baseline['provenance'])})" if baseline else ""))
    print(table(results, baseline))
    record = {"provenance": made_by, "results": {str(k): v for k, v in results.items()},
              "runs": {str(k): v for k, v in every.items()}}
    out = pathlib.Path(args.json) if args.json else \
        ROOT / "desktop/target/ui-bench" / time.strftime("%Y-%m-%dT%H-%M-%S.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(record, indent=1))
    print(f"\nwritten to {out}")
    if args.save_baseline:
        BASELINES.mkdir(exist_ok=True)
        stored.write_text(json.dumps(record, indent=1) + "\n")
        print(f"saved as the baseline for this system: {stored.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
