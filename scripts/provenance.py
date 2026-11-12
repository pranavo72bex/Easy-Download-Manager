#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""What produced a measurement, for the results files of ui-bench.py and memory-check.py
(gaps row 188; bench/bench.mjs's meta() records the same in JavaScript).

    python3 scripts/provenance.py [BINARY]      # prints it, as JSON

A number without these is a number nobody can compare: the commit and whether it had changes
not committed, `rustc -V`, the target, the build profile (the folder the binary was built in),
the binary's size, the system, its CPU and memory, its GPU and its displays with their scale.
Each part is read where the system says it without asking for a password, and is left out
(None) where it does not; nothing here fails a measurement. No serial number or name of the
person's is kept.
"""

import json
import os
import pathlib
import platform
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def _out(*args, cwd=None):
    """What `args` prints, or None if it cannot be run or fails."""
    try:
        r = subprocess.run(args, capture_output=True, text=True, timeout=30, cwd=cwd)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return r.stdout.strip() if r.returncode == 0 else None


def commit():
    head = _out("git", "rev-parse", "HEAD", cwd=ROOT)
    if head is None:
        return None
    # Changes not committed make the commit only an approximation of what was built.
    changed = _out("git", "status", "--porcelain", "--untracked-files=no", cwd=ROOT)
    return {"head": head, "dirty": bool(changed)}


def rust():
    cargo_bin = pathlib.Path.home() / ".cargo" / "bin"
    rustc = shutil.which("rustc") or (str(cargo_bin / "rustc") if (cargo_bin / "rustc").exists() else None)
    if rustc is None:
        return {"rustc": None, "host": None}
    verbose = _out(rustc, "-vV") or ""
    host = next((line.split(":", 1)[1].strip() for line in verbose.splitlines() if line.startswith("host:")), None)
    return {"rustc": _out(rustc, "-V"), "host": host}


def build(binary):
    """The binary's size and date, and the profile and target its path says: cargo puts a
    build in target/<profile>/, or target/<triple>/<profile>/ for a named target."""
    if binary is None:
        return None
    path = pathlib.Path(binary)
    if not path.exists():
        return {"path": str(path), "exists": False}
    stat = path.stat()
    profile = path.parent.name
    parts = path.parts
    triple = None
    if "target" in parts:
        after = parts[parts.index("target") + 1:-1]
        if len(after) == 2:
            triple = after[0]
    return {"path": str(path), "bytes": stat.st_size, "modified": int(stat.st_mtime),
            "profile": profile, "target": triple or rust()["host"]}


def system():
    info = {"os": platform.platform(), "machine": platform.machine(),
            "logical_cores": os.cpu_count(), "cpu": None, "memory_gb": None}
    if sys.platform == "darwin":
        info["cpu"] = _out("sysctl", "-n", "machdep.cpu.brand_string")
        mem = _out("sysctl", "-n", "hw.memsize")
        info["memory_gb"] = round(int(mem) / 2**30, 1) if mem else None
    elif sys.platform.startswith("linux"):
        try:
            text = pathlib.Path("/proc/cpuinfo").read_text()
            m = re.search(r"^model name\s*:\s*(.+)$", text, re.M)
            info["cpu"] = m.group(1).strip() if m else None
            m = re.search(r"^MemTotal:\s*(\d+) kB", pathlib.Path("/proc/meminfo").read_text(), re.M)
            info["memory_gb"] = round(int(m.group(1)) / 2**20, 1) if m else None
        except OSError:
            pass
    elif sys.platform == "win32":
        info["cpu"] = _out("powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_Processor).Name")
        mem = _out("powershell", "-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory")
        info["memory_gb"] = round(int(mem) / 2**30, 1) if mem and mem.isdigit() else None
    return info


def graphics():
    """The GPUs and the displays, each display with its pixels, what it looks like, and the
    scale between them."""
    gpus, displays = [], []
    if sys.platform == "darwin":
        raw = _out("system_profiler", "SPDisplaysDataType", "-json")
        try:
            cards = json.loads(raw or "{}").get("SPDisplaysDataType", [])
        except ValueError:
            cards = []
        for card in cards:
            gpus.append(card.get("sppci_model") or card.get("_name"))
            for d in card.get("spdisplays_ndrvs", []):
                pixels = re.match(r"(\d+) x (\d+)", d.get("_spdisplays_pixels", ""))
                looks = re.match(r"(\d+) x (\d+)", d.get("_spdisplays_resolution", ""))
                displays.append({
                    "pixels": d.get("_spdisplays_pixels"),
                    "looks_like": d.get("_spdisplays_resolution"),
                    "scale": round(int(pixels.group(1)) / int(looks.group(1)), 2) if pixels and looks else None,
                    "main": d.get("spdisplays_main") == "spdisplays_yes",
                })
    elif sys.platform.startswith("linux"):
        for line in (_out("lspci") or "").splitlines():
            if re.search(r"VGA|3D controller|Display controller", line):
                gpus.append(line.split(": ", 1)[-1])
        for line in (_out("xrandr", "--current") or "").splitlines():
            m = re.match(r"(\S+) connected.*?(\d+)x(\d+)\+", line)
            if m:
                displays.append({"pixels": f"{m.group(2)} x {m.group(3)}", "looks_like": None,
                                 "scale": float(os.environ["GDK_SCALE"]) if os.environ.get("GDK_SCALE") else None,
                                 "main": " primary " in line})
    elif sys.platform == "win32":
        names = _out("powershell", "-NoProfile", "-Command",
                     "(Get-CimInstance Win32_VideoController).Name") or ""
        gpus = [n.strip() for n in names.splitlines() if n.strip()]
        # The scale of each screen is the system's DPI over 96.
        dpi = _out("powershell", "-NoProfile", "-Command",
                   "(Get-ItemProperty 'HKCU:\\Control Panel\\Desktop\\WindowMetrics').AppliedDPI")
        if dpi and dpi.isdigit():
            displays.append({"pixels": None, "looks_like": None, "scale": round(int(dpi) / 96, 2), "main": True})
    return {"gpus": gpus or None, "displays": displays or None}


def provenance(binary=None, dataset=None):
    """Everything above, for one results file. `dataset` says what was measured on (rows,
    files), as the caller knows it."""
    r = rust()
    return {"commit": commit(), "rustc": r["rustc"], "host": r["host"], "build": build(binary),
            "system": system(), **graphics(), "dataset": dataset}


def line(p):
    """One line of it for a terminal."""
    c = p.get("commit") or {}
    b = p.get("build") or {}
    scale = ", ".join(str(d.get("scale")) for d in p.get("displays") or [])
    return (f"commit {c.get('head', '?')[:10]}{' (with changes)' if c.get('dirty') else ''}; "
            f"{p.get('rustc') or 'rustc ?'}; {b.get('target') or p.get('host')}; profile {b.get('profile')}; "
            f"{b.get('bytes', '?')} bytes; {p['system'].get('cpu')}; GPU {', '.join(p.get('gpus') or ['?'])}; "
            f"display scale {scale or '?'}")


if __name__ == "__main__":
    print(json.dumps(provenance(sys.argv[1] if len(sys.argv) > 1 else None), indent=1))
