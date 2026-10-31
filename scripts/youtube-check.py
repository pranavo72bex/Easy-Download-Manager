#!/usr/bin/env python3
"""The "Download video" panel on a real YouTube page, by hand only.

Every other check runs against servers on loopback; this one goes to
youtube.com, so it is not in CI and runs only when someone asks it to. In a window rather than with
DM_CHROME_HEADLESS=1, a pointer resting over the browser can fail its
pointer checks (the panel hid, or a drag ended at the pointer): the browser
likely sends a mouse move at the real pointer after the page changes. It
runs chrome-check.py's setup and its local checks, but replaces the panel
step with a real watch page ("Me at the zoo", 19 seconds):

    python3 scripts/youtube-check.py [watch-url] [screenshot.png]

It hovers the playing player, checks the panel is there and in its corner,
rests the pointer for five seconds (YouTube restarts playback and fades its
controls beneath it, which once hid the panel), clicks it, checks the list
of qualities the app's yt-dlp found, picks the first, and waits for the
video to land in dm's folder.
"""
import base64
import importlib.util
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("chrome_check", HERE / "chrome-check.py")
cc = importlib.util.module_from_spec(spec)
sys.modules["chrome_check"] = cc
spec.loader.exec_module(cc)

URL = sys.argv[1] if len(sys.argv) > 1 else "https://www.youtube.com/watch?v=jNQXAC9IVRw"
SHOT = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else None
RECT = """(() => { const v = [...document.querySelectorAll('video')]
    .sort((a, b) => b.clientWidth - a.clientWidth)[0];
  if (!v) return null; const r = v.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, paused: v.paused }; })()"""


def youtube(base, origin, engine_dl, engine_home, worker):
    tab = cc.open_tab(base, URL)
    tab.call("Runtime.enable")
    cc.loaded(tab, "/watch")
    cc.check("YouTube: the page script loads",
             cc.wait_for(lambda: tab.evaluate("!!document.querySelector('dm-panel')"), 20))

    # The player resizes while the page settles; hover once it has.
    def settled():
        a = tab.evaluate(RECT)
        time.sleep(1)
        b = tab.evaluate(RECT)
        same = a and b and b["w"] > 200 and [a[k] for k in "xywh"] == [b[k] for k in "xywh"]
        return same and b
    rect = cc.wait_for(settled, 30)
    if not cc.check("YouTube: a player is on the page", bool(rect), tab.evaluate("document.title")):
        return
    # The panel is offered once a video plays. Where
    # the browser's autoplay policy holds it (Chromium on Linux does), the
    # person presses play first, and so does this.
    if rect.get("paused"):
        tab.evaluate("(document.querySelector('.ytp-play-button') || document.querySelector('video')).click(), 1",
                     gesture=True)
        playing = cc.wait_for(lambda: not (tab.evaluate(RECT) or {}).get("paused", True), 10)
        cc.check("YouTube: pressing play on the paused player plays it", playing, str(tab.evaluate(RECT)))
        tab.evaluate("window.scrollTo(0, 0), 1")
        rect = cc.wait_for(settled, 15) or rect
    cc.hover(tab, rect)
    shown = cc.wait_for(lambda: (cc.panel(tab) or {}).get("shown") and cc.panel(tab), 5) or {}
    cc.check("YouTube: hovering the playing player shows 'Download video'",
             shown.get("title") == "Download video", str(shown))
    if not shown:
        return
    # Judged against where the player is now, once the panel has had a
    # moment to follow: the page lays the player out again after the panel
    # is up (YouTube's goes 8 px left and 12 px up, then widens, a second
    # after its first layout), and a panel judged against the old box was
    # called wrong while sitting on the player.
    def in_corner():
        r = tab.evaluate(RECT)
        p = cc.panel(tab) or {}
        ok = (r and p.get("shown") and r["x"] <= p["x"] and p["x"] + p["w"] <= r["x"] + r["w"] + 1
              and r["y"] <= p["y"] < r["y"] + 60)
        return ok and (p, r)
    corner = cc.wait_for(in_corner, 4)
    cc.check("YouTube: it sits in the player's top corner", bool(corner),
             f"{cc.panel(tab)} over {tab.evaluate(RECT)}")
    rect = tab.evaluate(RECT) or rect
    time.sleep(5)
    cc.check("YouTube: it stays while the pointer rests on the playing video",
             (cc.panel(tab) or {}).get("shown"), str(cc.panel(tab)))
    cc.click_panel(tab, rect)
    # The qualities the video comes in, from the app's yt-dlp.
    listed = cc.wait_for(lambda: len((cc.panel(tab) or {}).get("lines") or []) >= 2 and cc.panel(tab), 60) or {}
    cc.check("YouTube: its click lists the video's qualities with their sizes",
             bool(listed.get("lines")), str(cc.panel(tab)))
    print("  listed:", listed.get("lines"))
    if SHOT:
        SHOT.write_bytes(base64.b64decode(tab.call("Page.captureScreenshot", format="png")["data"]))
    if listed.get("at"):
        x, y = listed["at"][0]
        cc.mouse(tab, "mouseMoved", x, y)
        cc.mouse(tab, "mousePressed", x, y)
        cc.mouse(tab, "mouseReleased", x, y)
    said = cc.wait_for(lambda: (cc.panel(tab) or {}).get("title") == "Sent to dm" and cc.panel(tab), 15)
    cc.check("YouTube: its click is taken by the app", bool(said), str(cc.panel(tab)))
    got = cc.wait_for(lambda: [p for p in engine_dl.rglob("*")
                               if p.is_file() and p.suffix in (".mp4", ".webm", ".mkv")], 240)
    cc.check("YouTube: the video lands in dm's folder, fetched by yt-dlp", bool(got), str(got))
    played = cc.plays(got[0]) if got else None
    cc.check("YouTube: the downloaded video is whole (media_check)", bool(played and played[0] > 0 and played[1]),
             f"{played[0]}s, {played[1]}p" if played else "not a playable video")
    # The line names its container ("240p MP4 — 744 KB"), wherever the word is.
    first = (listed.get("lines") or [""])[0].lower().split()
    picked = next((w for w in first if w in ("mp4", "webm", "mkv")), "")
    cc.check("YouTube: in the container its line named", bool(got) and got[0].suffix == f".{picked}",
             f"picked {picked}, got {got[0].name if got else None}")
    errors = [e for e in tab.logged("error") if "dm" in e.lower()]
    cc.check("YouTube: no errors from dm on the page", not errors, "; ".join(errors)[:300])


cc.panel_checks = youtube
cc.main()
