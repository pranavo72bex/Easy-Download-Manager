#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The "Download video" panel on a real YouTube page, by hand and in the daily site watch.

Every other check runs against servers on loopback; this one goes to
youtube.com, so it is in no CI run a change starts: it runs when someone asks it to, and
each day in the site watch (.github/workflows/sites.yml, gaps row W6, with DM_SKIP_LOCAL=1
for the YouTube page alone and `--json` for scripts/site-table.py). In a window rather than with
DM_CHROME_HEADLESS=1, a pointer resting over the browser can fail its
pointer checks (the panel hid, or a drag ended at the pointer): the browser
likely sends a mouse move at the real pointer after the page changes. It
runs chrome-check.py's setup and its local checks, but replaces the panel
step with a real watch page ("Me at the zoo", 19 seconds):

    python3 scripts/youtube-check.py [watch-url] [screenshot.png] [--json result.json]

It hovers the playing player, checks the panel is there and in its corner,
rests the pointer for five seconds (YouTube restarts playback and fades its
controls beneath it, which once hid the panel), clicks it, checks the list
of qualities the app found, picks the first, and waits for the
video to land in dm's folder.
"""
import base64
import importlib.util
import pathlib
import sys
import time

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("chrome_check", HERE / "chrome-check.py")
cc = importlib.util.module_from_spec(spec)
sys.modules["chrome_check"] = cc
spec.loader.exec_module(cc)


def arguments(argv):
    """The places given, and --json's file if one is named."""
    places, json_out = [], None
    args = iter(argv)
    for a in args:
        if a == "--json":
            json_out = next(args, None)
        elif a.startswith("--json="):
            json_out = a.split("=", 1)[1]
        else:
            places.append(a)
    return places, json_out


ARGS, JSON_OUT = arguments(sys.argv[1:])
URL = ARGS[0] if ARGS else "https://www.youtube.com/watch?v=jNQXAC9IVRw"
SHOT = pathlib.Path(ARGS[1]) if len(ARGS) > 1 else None
RECT = """(() => { const v = [...document.querySelectorAll('video')]
    .sort((a, b) => b.clientWidth - a.clientWidth)[0];
  if (!v) return null; const r = v.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, paused: v.paused }; })()"""


PLAY_AT = """(() => { const b = document.querySelector('.ytp-play-button');
  if (!b) return null; const r = b.getBoundingClientRect();
  return r.width > 0 ? [r.left + r.width / 2, r.top + r.height / 2] : null; })()"""


# What YouTube's own player says of the video: its answer's playability (`OK`, or
# `LOGIN_REQUIRED` with "Sign in to confirm you're not a bot", which a CI machine's address
# meets, issue #45) and the error it shows over the picture, if any. A player that will not
# play is a wall, not dm's fault: the panel stays off a player covered by its error.
PLAYABILITY = """(() => { const p = document.querySelector('#movie_player'); let r = null;
  try { r = p && p.getPlayerResponse && p.getPlayerResponse(); } catch (e) {}
  const s = (r && r.playabilityStatus) || (window.ytInitialPlayerResponse || {}).playabilityStatus || {};
  const e = document.querySelector('.ytp-error');
  const shown = e && e.getBoundingClientRect().width > 0
    ? (e.innerText || '').trim().split(/\\s+/).join(' ').slice(0, 160) : '';
  return { status: s.status || '', reason: String(s.reason || '').slice(0, 160), error: shown }; })()"""


def wall(tab):
    """What YouTube's player says where it will not play the video, or None where it would."""
    said = tab.evaluate(PLAYABILITY) or {}
    if said.get("status") in ("", "OK") and not said.get("error"):
        return None
    return f"YouTube's player says {said.get('status') or 'no'}: {said.get('reason') or said.get('error')}"


def key(tab, name, code, number):
    """A key pressed and let go, as a person presses it on the focused page."""
    for kind in ("keyDown", "keyUp"):
        tab.call("Input.dispatchKeyEvent", type=kind, key=name, code=code, text=name if kind == "keyDown" else "",
                 windowsVirtualKeyCode=number, nativeVirtualKeyCode=number)


MUTED_PLAY = """(() => { const v = [...document.querySelectorAll('video')]
  .sort((a, b) => b.clientWidth - a.clientWidth)[0]; if (!v) return 'none'; v.muted = true;
  const p = v.play(); return p && p.then ? p.then(() => 'played', e => String(e).slice(0, 120)) : 'played'; })()"""


def press_play(tab):
    """Starts a paused player as a person does, and says whether it plays.

    One press did not always start it (macOS, 2026-11-11): the page's own
    autoplay can begin just as the press lands, which pauses it again, and a
    press on a player still loading does nothing. A person waits a moment,
    clicks the picture, then the play button, and looks again after each;
    so does this, three times at most, with the pointer rather than a
    script's click, so the press is one the autoplay policy honours."""
    def plays():
        return not (tab.evaluate(RECT) or {}).get("paused", True)

    for attempt in range(5):
        # The page may be starting it itself: given a moment first.
        if cc.wait_for(plays, 2):
            return True
        # A player that will not play (a bot check, a sign-in) is not pressed again.
        if wall(tab):
            return False
        rect = tab.evaluate(RECT)
        if not rect:
            return False
        middle = (rect["x"] + rect["w"] / 2, rect["y"] + rect["h"] / 2)
        if attempt == 0:
            # The picture: YouTube's large play button sits there while it
            # holds the video, and a click on the picture plays it.
            cc.click_at(tab, *middle)
        elif attempt < 3:
            # The controls show while the pointer is over the player; then
            # the play button itself.
            cc.mouse(tab, "mouseMoved", *middle)
            time.sleep(0.4)
            at = tab.evaluate(PLAY_AT)
            if at:
                cc.click_at(tab, *at)
            else:
                tab.evaluate("(document.querySelector('.ytp-play-button') || document.querySelector('video'))"
                             ".click(), 1", gesture=True)
        elif attempt == 3:
            # YouTube's own key for play and pause, on the player given the focus.
            tab.evaluate("(document.querySelector('#movie_player') || document.body).focus(), 1")
            key(tab, "k", "KeyK", 75)
        else:
            # Where the browser lets only a silent video start by itself (Edge on a CI
            # machine with no sound card), the picture plays muted; its download is the same.
            tab.evaluate(MUTED_PLAY, gesture=True)
        if cc.wait_for(plays, 6):
            return True
    return plays()


def youtube(base, origin, engine_dl, engine_home, worker):
    tab = cc.open_tab(base, URL)
    tab.call("Runtime.enable")
    # A page YouTube would not serve (Google's "unusual traffic" page, a
    # consent page, no network) fails the YouTube checks, said as such, and
    # the browser checks after this step still run: it ended the whole script
    # here, and the rows checked on loopback after it (B31, B32) went
    # unchecked with it (Linux, 2026-11-11).
    try:
        cc.loaded(tab, "/watch", seconds=40)
    except RuntimeError as e:
        cc.check("YouTube: the watch page loads", False, str(e)[:300])
        return
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
    # A player that says it will not play (Linux CI, 2026-11-19: paused and pressed in vain, the
    # panel then off it) is said as YouTube's wall, and site-table.py tells it from dm's fault.
    refused = cc.wait_for(lambda: wall(tab), 3)
    if refused:
        cc.check("YouTube: its player offers the video", False, refused)
        return
    # The panel is offered once a video plays. Where
    # the browser's autoplay policy holds it (Chromium on Linux does), the
    # person presses play first, and so does this.
    if rect.get("paused"):
        playing = press_play(tab)
        refused = None if playing else wall(tab)
        if refused:
            cc.check("YouTube: its player offers the video", False, refused)
            return
        cc.check("YouTube: pressing play on the paused player plays it", playing,
                 f"{tab.evaluate(RECT)}; YouTube's player: {tab.evaluate(PLAYABILITY)}")
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
    # The qualities the video comes in, from the app.
    listed = cc.wait_for(lambda: len((cc.panel(tab) or {}).get("lines") or []) >= 2 and cc.panel(tab), 60) or {}
    cc.check("YouTube: its click lists the video's qualities with their sizes",
             bool(listed.get("lines")), str(cc.panel(tab)))
    print("  listed:", listed.get("lines"))
    # The list's Part line takes a start and an end; left empty again so the whole video is fetched.
    part = tab.evaluate("(() => { const r = document.querySelector('dm-panel')?.shadowRoot;"
                        " const a = r?.querySelector('.part .from'), b = r?.querySelector('.part .to');"
                        " if (!a || !b) return 'none'; a.value = '0:02'; b.value = '0:06';"
                        " const typed = a.value + '-' + b.value; a.value = ''; b.value = ''; return typed; })()")
    cc.check("YouTube: its list has a Part line that takes a start and an end", part == "0:02-0:06", str(part))
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
    # Where it did not land, dm's own rows say how far it got (Linux, gaps row Q3).
    rows = [] if got else [(r.get("url", "")[:90], r.get("state"), r.get("path"))
                           for r in cc.queue_rows(engine_home)
                           if "youtube" in r.get("url", "") or "googlevideo" in r.get("url", "")]
    cc.check("YouTube: the video lands in dm's folder, fetched by dm itself", bool(got),
             f"{got}; dm's rows: {rows}" if not got else str(got))
    played = cc.plays(got[0]) if got else None
    cc.check("YouTube: the downloaded video is whole (media_check)", bool(played and played[0] > 0 and played[1]),
             f"{played[0]}s, {played[1]}p" if played else "not a playable video")
    # The line names its container ("240p MP4 — 744 KB"), wherever the word is.
    first = (listed.get("lines") or [""])[0].lower().split()
    # The player's own instant list ("240p") names none: dm saves those as MP4.
    picked = next((w for w in first if w in ("mp4", "webm", "mkv")), "mp4")
    cc.check("YouTube: in the container its line named", bool(got) and got[0].suffix == f".{picked}",
             f"picked {picked}, got {got[0].name if got else None}")
    errors = [e for e in tab.logged("error") if "dm" in e.lower()]
    cc.check("YouTube: no errors from dm on the page", not errors, "; ".join(errors)[:300])


def write_json():
    """The YouTube checks as one row of scripts/site-table.py's table."""
    spec = importlib.util.spec_from_file_location("site_table", HERE / "site-table.py")
    table = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(table)
    checks = list(cc.results)
    row = table.youtube_row([{"ok": ok, "name": n, "detail": d} for ok, n, d in checks])
    table.write_result(JSON_OUT, "youtube-check", [{**row, "url": URL}], checks)


cc.panel_checks = youtube
try:
    cc.main()
finally:
    if JSON_OUT:
        write_json()
