#!/usr/bin/env python3
"""dm as someone new meets it: a fresh dm home, nothing set, real X11
input. Run by scripts/linux-newcomer-check.sh inside the Linux container.

Each step is what a person who has used other download managers would try
first, and each leaves a screenshot in /shots to be looked at, not only a
check: the first window, the browser bar and its buttons, a file added by
hand, a link that is broken, words that are not a link, a real YouTube
page (the first video, with dm fetching what it needs), Options › Browser,
and saying not now to the bar. The checks are what a person would notice.
"""
import os
import pathlib
import subprocess
import sys
import tempfile
import time

HERE = pathlib.Path(__file__).parent
# gui.py's helpers (real clicks over AT-SPI, typing, the local server),
# without running its own session.
exec(compile((HERE / "gui.py").read_text().replace("\nmain()\n", "\n"), str(HERE / "gui.py"), "exec"))

SHOTS = pathlib.Path("/shots")
VIDEO = os.environ.get("DM_VIDEO", "https://www.youtube.com/watch?v=jNQXAC9IVRw")


def shot(name):
    subprocess.run(["import", "-window", "root", str(SHOTS / f"{name}.png")], capture_output=True)


def row_text(home, url):
    """What the window says on the row, as the control protocol reads it."""
    if not ORACLE:
        return ""
    for r in ORACLE("ui.inspect").get("rows") or []:
        if r.get("url") == url:
            return r.get("status") or ""
    return ""


def newcomer():
    global ORACLE
    # Toolkits publish their windows over AT-SPI only with a screen
    # reader said to be running, and the clicks find controls that way.
    screen_reader_on()
    home = pathlib.Path(tempfile.mkdtemp(prefix="dm-new-"))
    env = {**os.environ, "DM_HOME": str(home)}
    app = dm_popen([str(BINARY), "--control", "0"], env=env,
                   stdout=subprocess.DEVNULL, stderr=open(SHOTS / "dm.log", "w"))
    try:
        check("dm opens a window", wait_for(dm_windows, 30))
        ORACLE = oracle(home)
        time.sleep(2)
        shot("01-first-start")
        check("the first window offers the browser extension", where("push button", "Microsoft Edge"))

        clicked = click("push button", "Firefox")
        time.sleep(2)
        shot("02-firefox-button")
        check("its Firefox button can be clicked", clicked)
        check("and dm is still running", app.poll() is None)

        files = SlowFiles()
        small = files.url("small-file.zip")
        check("Add URL with a file's address starts it", add_url_by_hand(small))
        check("and it finishes", finished(home, small, 60))
        shot("03-file-done")

        missing = files.url("broken-link.zip")
        added = add_url_by_hand(missing)
        time.sleep(4)
        shot("04-broken-link-retrying")
        check("a broken link is added", added)
        check("and it fails in the end, after its retries",
              wait_for(lambda: state_of(home, missing) == "failed", 240))
        said = row_text(home, missing)
        shot("04-broken-link-failed")
        print(f"        the broken link's row: {said!r}", flush=True)
        check("and its row says why in plain words",
              said and "answered" not in said and "http" not in said, said)
        settle()

        # Words that are no link, typed where the address goes.
        settle()
        click("push button", "Add URL")
        click_field("Address")
        type_text("my holiday photos")
        time.sleep(0.5)
        click("push button", "OK")
        time.sleep(1)
        shot("05-not-a-link")
        check("words that are not a link are not added",
              not any(r.get("url") == "my holiday photos" for r in queue_rows(home)))
        key("Escape")
        time.sleep(0.5)

        settle()
        click("push button", "Add URL")
        click_field("Address")
        type_text(VIDEO)
        click("push button", "OK")
        first = wait_for(lambda: where("push button", "Start now"), 10)
        time.sleep(3)
        later = where("push button", "Start now")
        print(f"        Start now at {first}, and 3 s on at {later}", flush=True)
        check("Start now stays where it was while dm looks at the page", first == later, f"{first} then {later}")
        click("push button", "Start now")
        time.sleep(1)
        check("a YouTube page's Start now takes the first click",
              any(r.get("url") == VIDEO for r in queue_rows(home)))
        if where("push button", "Start now"):
            print("        Download details still open; clicked again", flush=True)
            click("push button", "Start now")
        time.sleep(3)
        shot("06-video-first-seconds")
        early = row_text(home, VIDEO)
        print(f"        the video's row, 4 s in: {early!r}", flush=True)
        check("the first video says what it waits for", early and early != "Starting…", early)
        # The container has Debian's own yt-dlp, months old: dm is to pass
        # it over for its own, as for anyone who installed it with apt.
        done = finished(home, VIDEO, 300)
        shot("07-video-done")
        check("the video finishes with nothing installed", done,
              row_text(home, VIDEO))
        # A row saying "Complete" is not a file: look where the row says it is.
        saved = pathlib.Path((row_of(home, VIDEO) or {}).get("path") or "/nonexistent")
        check("and the video is a real file where the row says", saved.is_file() and saved.stat().st_size > 100_000,
              f"{saved} {saved.stat().st_size if saved.is_file() else 'missing'}")

        click("push button", "Not now")
        time.sleep(1)
        shot("08-not-now")
        check("not now puts the bar away", not where("push button", "Microsoft Edge"))

        # Then Options › Browser, where the same buttons stay: the only
        # "Chrome and others" left is the one in Options.
        settle()
        click("push button", "Options")
        time.sleep(1)
        tab = click("*", "Browser")
        time.sleep(1)
        shot("09-options-browser")
        check("Options has a Browser tab", tab)
        check("and it offers the extension", where("push button", "Chrome and others"))
        key("Escape")
        time.sleep(0.5)
        check("dm ran throughout", app.poll() is None)
    finally:
        end(app)
    print(f"\n{sum(RESULTS)} of {len(RESULTS)} newcomer checks passed", flush=True)
    return all(RESULTS)


if __name__ == "__main__":
    sys.exit(0 if newcomer() else 1)
