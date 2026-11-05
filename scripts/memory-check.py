#!/usr/bin/env python3
"""dm's memory and threads at rest, against a budget (gaps row 137).

    python scripts/memory-check.py            # the release build
    DM_BINARY=path\\to\\dm-desktop.exe python scripts/memory-check.py

Starts dm with a home of its own (nothing of the person's is read or written),
waits for its window to settle, then samples it for a while and prints the
working set, the private bytes, the threads and the handles. Fails when the
average working set or the private bytes go over the budget
(`DM_MEMORY_BUDGET_MB`, `DM_PRIVATE_BUDGET_MB`). On Windows it reads the
process through PowerShell's Get-Process; on macOS the working set is the
resident size (`ps`), the private bytes the physical footprint (`footprint`),
and the handles the open files (`lsof`).
"""
import os
import pathlib
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or (
    "D:/dm-target/release/dm-desktop.exe" if sys.platform == "win32"
    else ROOT / "desktop/target/release/dm-desktop"))
# On macOS a third more than measured at rest on 2026-11-06 (112 MB resident, 75 MB
# footprint, 37 MB of which the window's drawing surfaces): a regression, not
# the system's own share, is what fails it.
WORKING_BUDGET_MB = float(os.environ.get("DM_MEMORY_BUDGET_MB", "150" if sys.platform == "darwin" else "100"))
PRIVATE_BUDGET_MB = float(os.environ.get("DM_PRIVATE_BUDGET_MB", "100" if sys.platform == "darwin" else "60"))
SETTLE_S = float(os.environ.get("DM_MEMORY_SETTLE_S", "15"))
SAMPLES = int(os.environ.get("DM_MEMORY_SAMPLES", "20"))


def sample(pid):
    """Working set, private bytes (MB), threads and handles of `pid`, or None once it is gone."""
    if sys.platform == "darwin":
        return sample_mac(pid)
    script = (f"$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; "
              "if ($p) { '{0} {1} {2} {3}' -f $p.WorkingSet64, $p.PrivateMemorySize64, $p.Threads.Count, $p.HandleCount }")
    out = subprocess.run(["powershell", "-NoProfile", "-Command", script],
                         capture_output=True, text=True, timeout=30).stdout.split()
    if len(out) != 4:
        return None
    working, private, threads, handles = (int(v) for v in out)
    return working / 2**20, private / 2**20, threads, handles


def sample_mac(pid):
    def run(*args):
        return subprocess.run(args, capture_output=True, text=True, timeout=30).stdout
    resident = run("ps", "-o", "rss=", "-p", str(pid)).strip()
    if not resident:
        return None
    # `ps -M` prints a heading and then one line for each thread.
    threads = max(len(run("ps", "-M", "-p", str(pid)).splitlines()) - 1, 0)
    handles = max(len(run("lsof", "-n", "-P", "-p", str(pid)).splitlines()) - 1, 0)
    footprint = 0.0
    for line in run("footprint", "-p", str(pid)).splitlines():
        # "dm-desktop [123]: 64-bit    Footprint: 41 MB (16384 bytes per page)"
        if "Footprint:" in line:
            amount, unit = line.split("Footprint:")[1].split()[:2]
            footprint = float(amount) * {"KB": 2**-10, "MB": 1, "GB": 2**10}.get(unit, 1)
            break
    return int(resident) / 2**10, footprint, threads, handles


def main():
    if sys.platform not in ("win32", "darwin"):
        sys.exit("memory-check.py reads the process on Windows and macOS only")
    if not BINARY.exists():
        sys.exit(f"no dm at {BINARY}: cd desktop && cargo build --release -p dm-desktop")
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-memory-", dir=os.environ.get("DM_MEMORY_TMP")))
    env = dict(os.environ, DM_HOME=str(home), DM_NO_SECRET_STORE="1")
    dm = subprocess.Popen([str(BINARY)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(SETTLE_S)
        rows = []
        for _ in range(SAMPLES):
            got = sample(dm.pid)
            if got is None:
                sys.exit("dm closed while it was measured")
            rows.append(got)
            time.sleep(1)
    finally:
        dm.terminate()
        try:
            dm.wait(timeout=10)
        except subprocess.TimeoutExpired:
            dm.kill()
    working = [r[0] for r in rows]
    private = [r[1] for r in rows]
    threads = [r[2] for r in rows]
    handles = [r[3] for r in rows]
    print(f"dm at rest, {len(rows)} samples after {SETTLE_S:.0f} s ({BINARY}):")
    print(f"  working set  {statistics.mean(working):6.1f} MB average, {max(working):6.1f} MB peak")
    print(f"  private      {statistics.mean(private):6.1f} MB average, {max(private):6.1f} MB peak")
    print(f"  threads      {min(threads)} to {max(threads)}")
    print(f"  handles      {min(handles)} to {max(handles)}")
    over = []
    if statistics.mean(working) > WORKING_BUDGET_MB:
        over.append(f"working set {statistics.mean(working):.1f} MB over {WORKING_BUDGET_MB:.0f} MB")
    if statistics.mean(private) > PRIVATE_BUDGET_MB:
        over.append(f"private bytes {statistics.mean(private):.1f} MB over {PRIVATE_BUDGET_MB:.0f} MB")
    if over:
        print("FAIL: " + "; ".join(over))
        sys.exit(1)
    print("PASS: within the budget")


if __name__ == "__main__":
    main()
