#!/usr/bin/env python3
"""dm as a person meets it after installing it: nothing else on the computer.

The real dm-desktop binary, headless, with a dm home of its own made for
this run (no tools fetched before), told to see only the tools it fetches
itself (DM_ONLY_FETCHED_TOOLS, so nothing installed on this computer is
used) and given the PATH a browser or the Dock gives an app. Real
YouTube videos are handed to it through its inbox, as the browser's
"Download video" panel hands them, at each kind of quality the panel
offers. Each must finish as one file holding picture and sound at the
quality asked for (the tallest picture yt-dlp lists, for "best"), which
is whole from end to end.

The results are judged by media_check.py beside this script, which reads
each file's own structure (its tracks, the picture's height, the length,
and that every sample the file points at is in it); it needs nothing
installed. Needs the network; not run in CI.

    python3 scripts/out-of-box-check.py            # builds dm if needed
    DM_BINARY=path/to/dm-desktop python3 scripts/out-of-box-check.py
"""

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import media_check  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINARY = pathlib.Path(os.environ.get("DM_BINARY", ROOT / "desktop/target/release/dm-desktop"))

# (what is asked, the quality word the panel sends, what must arrive)
CASES = [
    ("the best, as the panel's first line asks", "https://www.youtube.com/watch?v=jNQXAC9IVRw", None, ("mp4", "best")),
    ("the best of a 1080p video", "https://www.youtube.com/watch?v=9bZkp7q19f0", None, ("mp4", "best")),
    ("the best of a 4K video", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", None, ("mp4", "best")),
    ("720p MP4", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "720/mp4", ("mp4", 720)),
    ("480p WebM", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "480/webm", ("webm", 480)),
    ("the sound alone", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "audio/m4a", ("m4a", "audio")),
]

failures = []


def check(name, ok, detail=""):
    print(f"  {'ok  ' if ok else 'FAIL'}  {name}" + ("" if ok else f"  ({detail})"))
    if not ok:
        failures.append(name)


def rows(home):
    try:
        data = json.loads((home / "queue.json").read_text())
    except (OSError, ValueError):
        return []
    return data if isinstance(data, list) else data.get("entries", data.get("rows", []))


def main():
    if not BINARY.exists():
        print("building dm…")
        subprocess.run(["cargo", "build", "--release", "-p", "dm-desktop"], cwd=ROOT / "desktop", check=True)
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-fresh-"))
    out = home / "out"
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(out),
        "categorize": False,
        "show_progress": False,
        "show_complete": False,
    }))
    env = {
        "HOME": os.environ["HOME"],
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
        "PATH": "/usr/bin:/bin",
        "DM_HOME": str(home),
        "DM_ONLY_FETCHED_TOOLS": "1",
    }
    app = subprocess.Popen([str(BINARY), "--headless"], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(2)
        inbox = home / "inbox"
        inbox.mkdir(exist_ok=True)
        handed = time.time()
        for n, (_, url, quality, _) in enumerate(CASES):
            request = {"url": url, "referrer": url}
            if quality:
                request["quality"] = quality
            # Whole, then named: dm reads only finished files.
            tmp = inbox / f".case{n}.tmp"
            tmp.write_text(json.dumps(request))
            tmp.rename(inbox / f"case{n}.json")
            # One at a time, so each row is known by its order.
            time.sleep(1.5)
        started = time.time()
        while time.time() - started < 1800:
            states = [r.get("state", {}).get("kind") for r in rows(home)]
            if len(states) >= len(CASES) and all(s in ("completed", "failed", "cancelled") for s in states):
                break
            time.sleep(3)
        listed = rows(home)
        tools = sorted(p.name for p in (home / "tools").iterdir()) if (home / "tools").exists() else []
        print(f"tools dm fetched for itself: {', '.join(tools) or 'none'}")
        # dm's tools folder (dm-core's tools.rs) holds yt-dlp (or, on macOS,
        # its unpacked yt-dlp-macos folder) and qjs, and nothing else.
        check("dm fetched yt-dlp and QuickJS, and nothing else", any(t.startswith("yt-dlp") for t in tools)
              and any(t.startswith("qjs") for t in tools)
              and all(t.startswith(("yt-dlp", "qjs")) for t in tools), tools)
        for n, (name, url, quality, (container, height)) in enumerate(CASES):
            print(name)
            row = listed[n] if n < len(listed) else None
            state = (row or {}).get("state", {})
            check("finishes", state.get("kind") == "completed", state)
            if state.get("kind") != "completed":
                continue
            path = pathlib.Path(row["path"])
            if row.get("finished_at"):
                # How long a person waited, the first video's tool fetch
                # included.
                print(f"        done {row['finished_at'] - handed:.0f} s after it was handed over")
            check(f"as a .{container}", path.suffix == f".{container}", path.name)
            info = media_check.inspect(path)
            video = [t for t in info["tracks"] if t["kind"] == "video"]
            audio = [t for t in info["tracks"] if t["kind"] == "audio"]
            codecs = media_check.summary(info)
            print(f"        {path.name}: {codecs}")
            if height == "audio":
                check("holds the sound and no picture", audio and not video, codecs)
            else:
                check("holds picture and sound", len(video) == 1 and len(audio) == 1, codecs)
                got = (video[0]["height"] or 0) if video else 0
                if height == "best":
                    listed_formats = subprocess.run(
                        [str(next((home / "tools").glob("yt-dlp*/yt-dlp*"), None) or next((home / "tools").glob("yt-dlp*"))),
                         "-J", "--no-warnings", "--no-js-runtimes", "--js-runtimes", f"quickjs:{next((home / 'tools').glob('qjs*'))}", "--", url],
                        env=env, capture_output=True, text=True, timeout=300).stdout
                    formats = json.loads(listed_formats or "{}").get("formats", [])
                    tallest = max((f.get("height") or 0 for f in formats if f.get("vcodec") not in (None, "none")), default=0)
                    check(f"at the tallest picture on offer ({tallest}p)", got == tallest, f"{got}p")
                else:
                    check(f"at {height}p", got == height, f"{got}p")
            check("whole from end to end", not info["problems"], "; ".join(info["problems"]))
    finally:
        app.terminate()
        try:
            app.wait(10)
        except subprocess.TimeoutExpired:
            app.kill()
        if not failures:
            shutil.rmtree(home, ignore_errors=True)
        else:
            print(f"dm's home is kept for a look: {home}")
    if failures:
        print(f"\n{len(failures)} check(s) failed")
        sys.exit(1)
    print("\nout of the box, every quality arrived as asked")


if __name__ == "__main__":
    main()
