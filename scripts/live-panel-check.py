#!/usr/bin/env python3
"""The "Download video" panel on a real site's HTML5 player, by hand only.

    python3 scripts/live-panel-check.py [page-url]

Like youtube-check.py, it runs chrome-check.py's setup and its local
checks, then replaces the panel step with a live page: by default
W3Schools' HTML5 video example, a plain `<video>` with an MP4 source. It
plays the video, hovers it, checks the panel is there, clicks it, and waits
for the file in dm's folder; on a page whose player yt-dlp reads, it picks
the smallest quality listed. Not in CI: it goes to a real site.
"""
import importlib.util
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("chrome_check", HERE / "chrome-check.py")
cc = importlib.util.module_from_spec(spec)
sys.modules["chrome_check"] = cc
spec.loader.exec_module(cc)

URL = sys.argv[1] if len(sys.argv) > 1 else "https://www.w3schools.com/html/html5_video.asp"
# The largest video on the page, brought into view first.
# Every video, including those inside web components' open shadow roots.
ALL_VIDEOS = """(() => { const out = []; const walk = root => { for (const e of root.querySelectorAll('*')) {
  if (e.tagName === 'VIDEO') out.push(e); if (e.shadowRoot) walk(e.shadowRoot); } }; walk(document); return out; })()"""
RECT = """(() => { const v = """ + ALL_VIDEOS + """
    .sort((a, b) => b.clientWidth - a.clientWidth)[0];
  if (!v) return null; v.scrollIntoView({block: 'center'}); const r = v.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, paused: v.paused,
           src: v.currentSrc || v.src || (v.querySelector('source') || {}).src || '' }; })()"""


def live(base, origin, engine_dl, engine_home, worker):
    tab = cc.open_tab(base, URL)
    tab.call("Runtime.enable")
    path = "/" + URL.split("://", 1)[1].split("/", 1)[1].split("?")[0] if "/" in URL.split("://", 1)[1] else "/"
    # Any page but the blank one a tab starts on: sites redirect to an
    # address of their own (TED and Pexels both do).
    cc.wait_for(lambda: (lambda v: v is True)(
        (lambda: tab.evaluate("location.href !== 'about:blank' && document.readyState === 'complete'"))()
        if True else None), 40)
    cc.check(f"{URL}: the page script loads",
             cc.wait_for(lambda: tab.evaluate("!!document.querySelector('dm-panel')"), 20))
    rect = cc.wait_for(lambda: tab.evaluate(RECT), 20)
    # Where the site actually put the tab: a consent page or a bot check is
    # said as such, rather than as a missing player.
    at = tab.evaluate("location.href + ' — ' + document.title")
    if not cc.check("a player is on the page", bool(rect), f"none, at {at}"):
        return
    # Offered once it plays; a person presses play.
    tab.evaluate("(() => { const v = " + ALL_VIDEOS + ".sort((a, b) => b.clientWidth - a.clientWidth)[0];"
                 " v.muted = true; v.play().catch(() => 0); return 1; })()", gesture=True)
    time.sleep(1)
    rect = tab.evaluate(RECT) or rect
    cc.hover(tab, rect)
    shown = cc.wait_for(lambda: (cc.panel(tab) or {}).get("shown") and cc.panel(tab), 8) or {}
    cc.check("hovering the playing player shows 'Download video'",
             shown.get("title") == "Download video", str(shown))
    if not shown:
        return
    inside = rect["x"] <= shown["x"] and shown["x"] + shown["w"] <= rect["x"] + rect["w"] + 1
    cc.check("it sits in the player's top corner", inside, f"{shown} over {rect}")
    before = {p.name for p in engine_dl.rglob("*") if p.is_file()}
    cc.click_panel(tab)
    # A player page lists its qualities (the app's yt-dlp reads them); the
    # smallest is picked, to keep the download short.
    listed = cc.wait_for(lambda: ((cc.panel(tab) or {}).get("lines") or (cc.panel(tab) or {}).get("title") == "Sent to dm")
                         and cc.panel(tab), 60) or {}
    if listed.get("lines") and listed.get("at"):
        print("  listed:", listed["lines"])
        units = {"B": 1, "KB": 1 << 10, "MB": 1 << 20, "GB": 1 << 30}
        def size(line):
            try:
                number, unit = line.rsplit("— ", 1)[1].split()
                return float(number) * units.get(unit, 1 << 40)
            except (IndexError, ValueError):
                return float("inf")
        pick = min(range(len(listed["lines"])), key=lambda i: size(listed["lines"][i]))
        cc.check("its click lists the video's qualities with their sizes", True, str(listed["lines"]))
        cc.click_at(tab, *listed["at"][pick])
    said = cc.wait_for(lambda: (cc.panel(tab) or {}).get("title") == "Sent to dm" and cc.panel(tab), 30)
    cc.check("its click is taken by the app", bool(said), str(cc.panel(tab)))
    name = rect.get("src", "").split("?")[0].rsplit("/", 1)[-1]
    got = cc.wait_for(lambda: [p for p in engine_dl.rglob("*")
                               if p.is_file() and p.name not in before and p.stat().st_size > 0], 120)
    rows = [r for r in cc.queue_rows(engine_home) if (name and name in r.get("url", "")) or r.get("url") == URL]
    cc.check("the video lands in dm's folder", bool(got),
             f"{[p.name for p in got] if got else 'nothing'} (the player's source: {name}; "
             f"dm's row: {[(r.get('state'), r.get('filename')) for r in rows][-1:]})")
    # dm's own errors: its page script logs as "dm:" and runs from its module.
    # The finished file, once dm has moved it into place, opens as a video.
    finished = cc.wait_for(lambda: next((r for r in cc.queue_rows(engine_home)
                                         if r.get("state", {}).get("kind") == "completed" and r.get("path")
                                         and (name and name in r.get("url", "") or r.get("url") == URL)), None), 180)
    played = cc.plays(finished["path"]) if finished else None
    cc.check("the downloaded video plays (ffprobe)", bool(played and played[0] > 0),
             f"{pathlib.Path(finished['path']).name}: {played[0]}s, {played[1]}p" if played and finished
             else f"not finished or not playable: {finished and finished.get('state')}")
    errors = [e for e in tab.logged("error") if e.startswith("dm:") or "dm_catcher" in e]
    cc.check("no errors from dm on the page", not errors, "; ".join(errors)[:300])


cc.panel_checks = live
cc.main()
