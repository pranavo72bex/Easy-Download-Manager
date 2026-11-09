#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The browser panel's "Part" line on real streaming pages (gaps row 45, contract row B59).

    python3 scripts/part-check.py [hls] [dash]

Like sites-check.py it goes to public pages, so it is not in CI. It reuses chrome-check.py's
setup (a visible browser with dm's extension and the app behind it; DM_BINARY on Windows), with
the app's Download details on, as a new install has it, and its control port open, so the check
presses Start now there as a person does. For each page it plays the stream, opens the panel's
list, types a part (0:02 to 0:06) on its Part line, picks the stream's line, checks Download
details opens with "Only a part of the video" and those ends, presses Start now, and reads the
file that lands: a picture, and about as long as the part. An HLS stream in transport-stream
pieces is joined at the pieces the part overlaps, so its file is no shorter than the part and far
shorter than the whole; a DASH stream in MP4 is trimmed, so its file is within a second or two of
the part.

A plain MP4 file has no list in the panel (the click hands the file itself), so it has no Part
line there; its part is Download details' own, checked by the window tests.

Leave the mouse and keyboard alone while it runs.
"""
import importlib.util
import json
import os
import pathlib
import socket
import sys
import time
import urllib.parse

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("chrome_check", HERE / "chrome-check.py")
cc = importlib.util.module_from_spec(spec)
sys.modules["chrome_check"] = cc
spec.loader.exec_module(cc)
sys.path.insert(0, str(HERE))
import media_check  # noqa: E402

PART = ("0:02", "0:06")
SPAN = 4.0
# Public test streams on pages that play them with the Media Source API, as a site's player does.
PAGES = {
    "hls": {
        "url": "https://hlsjs.video-dev.org/demo/?src="
               + urllib.parse.quote("https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8", safe=""),
        "line": "hls",
        # Joined at the pieces the part overlaps: at least the part, far less than the whole.
        "ok": lambda seconds: SPAN - 0.5 <= seconds <= 40,
    },
    "dash": {
        "url": "https://reference.dashif.org/dash.js/latest/samples/getting-started/auto-load-single-video.html",
        "line": "dash",
        # Trimmed in MP4: the part, give or take the frames at its ends.
        "ok": lambda seconds: SPAN - 1.0 <= seconds <= SPAN + 2.5,
    },
    # A plain MP4 file in a <video>: the panel hands the file itself, with no list, and the part
    # is set in Download details; dm cuts it from the file's own sample tables.
    "mp4": {
        "url": "https://www.w3.org/2010/05/video/mediaevents.html",
        "line": "mp4",
        "ok": lambda seconds: SPAN - 1.0 <= seconds <= SPAN + 2.5,
    },
}
RECT = """(() => { const v = [...document.querySelectorAll('video')]
    .sort((a, b) => b.clientWidth - a.clientWidth)[0];
  if (!v) return null; const r = v.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, paused: v.paused, t: v.currentTime }; })()"""
PLAY = """(() => { const v = [...document.querySelectorAll('video')]
    .sort((a, b) => b.clientWidth - a.clientWidth)[0];
  if (v) { v.muted = true; v.play().catch(() => {}); } return 1; })()"""
SET_PART = """(() => { const r = document.querySelector('dm-panel')?.shadowRoot;
  const a = r?.querySelector('.part .from'), b = r?.querySelector('.part .to');
  if (!a || !b) return 'none'; a.value = %s; b.value = %s; return a.value + '-' + b.value; })()"""


def streams_in(state):
    """The panel's lines that are streams, with their places in its list."""
    return [(i, line) for i, line in enumerate(state.get("lines") or [])
            if not line.startswith(("Hide for now", "Never on"))]


class Control:
    """dm's control port, as a script reaches it: Download details is answered there."""

    def __init__(self, home):
        port = cc.wait_for(lambda: (home / "control.port").exists() and (home / "control.port").read_text().strip(), 60)
        if not port:
            raise RuntimeError("dm opened no control port (was it running before its settings were changed?)")
        self.conn = socket.create_connection(("127.0.0.1", int(port)), timeout=30)
        self.replies = self.conn.makefile("r", encoding="utf-8")
        self.call("auth", token=(home / "control.token").read_text().strip())

    def call(self, method, **params):
        self.conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
        return json.loads(self.replies.readline())

    def dialog_texts(self):
        texts = (self.call("ui.texts").get("result") or {}).get("texts", [])
        return [t["text"] for t in texts if t.get("window") == "dialog"]


def part_of(name, page, base, engine_dl, engine_home, worker):
    tab = cc.open_tab(base, page["url"])
    tab.call("Runtime.enable")
    cc.wait_for(lambda: tab.evaluate("document.readyState") == "complete", 40)
    cc.check(f"{name}: the page script loads",
             cc.wait_for(lambda: tab.evaluate("!!document.querySelector('dm-panel')"), 20))
    rect = cc.wait_for(lambda: (tab.evaluate(RECT) or {}).get("w", 0) > 150 and tab.evaluate(RECT), 40)
    if not cc.check(f"{name}: a player is on the page", bool(rect), tab.evaluate("document.title")):
        return
    tab.evaluate(PLAY, gesture=True)
    # A few seconds in, its playlist or manifest and first pieces are fetched, and heard.
    playing = cc.wait_for(lambda: (tab.evaluate(RECT) or {}).get("t", 0) > 3 and tab.evaluate(RECT), 40)
    cc.check(f"{name}: the stream plays", bool(playing), str(tab.evaluate(RECT)))
    # The player at the window's top, so the list that opens below the panel is on screen: a
    # demo page's player sits under its form, and a click past the window's edge lands nowhere.
    tab.evaluate("(() => { const v = [...document.querySelectorAll('video')]"
                 ".sort((a, b) => b.clientWidth - a.clientWidth)[0]; v.scrollIntoView({block: 'start'}); return 1; })()")
    time.sleep(1.5)
    rect = tab.evaluate(RECT) or rect
    cc.hover(tab, rect)
    shown = cc.wait_for(lambda: (cc.panel(tab) or {}).get("shown") and cc.panel(tab), 10) or {}
    # A stream's page has a list behind the panel's arrow; a plain file has none, by design.
    cc.check(f"{name}: hovering the player shows the panel", bool(shown.get("shown")), str(shown))
    if not shown:
        return
    # A stream's page is clicked once the extension has heard the stream (its playlist or
    # manifest, described): clicked before, the panel hands the page, which only YouTube's is.
    if page["line"] != "mp4":
        page_url = tab.evaluate("location.href")
        ask = ("chrome.tabs.query({}).then(ts => { const t = ts.find(t => t.url === %s); return t"
               " ? dmReady.then(m => m.on_page_message({type: 'dm.formats', src: '', frame: %s},"
               " {id: chrome.runtime.id, tab: {id: t.id, url: %s}, url: %s})).then(r => (r.choices || []).length)"
               " : 0; })") % ((json.dumps(page_url),) * 4)
        heard = cc.wait_for(lambda: worker.evaluate(ask), 30)
        cc.check(f"{name}: the extension has heard the stream", bool(heard), f"{heard} lines")
    before = {p for p in engine_dl.rglob("*") if p.is_file()}
    cc.click_panel(tab, rect)
    # Two streams or more: the list, with its Part line above them. One: handed at once, and the
    # part is set in Download details, which opens for it (as a new install has it).
    outcome = cc.wait_for(lambda: (lambda st: (st.get("title") == "Sent to dm" or streams_in(st)) and st)(
        cc.panel(tab) or {}), 60) or {}
    streams = streams_in(outcome)
    print(f"  {name} the panel: {outcome.get('title')!r}, lines {outcome.get('lines')}", flush=True)
    if streams:
        typed = tab.evaluate(SET_PART % (json.dumps(PART[0]), json.dumps(PART[1])))
        cc.check(f"{name}: its list has a Part line, and the part is typed", typed == f"{PART[0]}-{PART[1]}", str(typed))
        pick = next((i for i, line in streams if page["line"] in line.lower()), streams[0][0])
        x, y = outcome["at"][pick]
        high = tab.evaluate("innerHeight")
        if not cc.check(f"{name}: the stream's line is on screen", 0 <= y < high - 4, f"line at y={y:.0f}, window {high}"):
            return
        cc.click_at(tab, x, y)
        sent = cc.wait_for(lambda: (cc.panel(tab) or {}).get("title") == "Sent to dm" and cc.panel(tab), 20)
        cc.check(f"{name}: its line is taken by the app", bool(sent), str(cc.panel(tab)))
        control = Control(engine_home)
        words = cc.wait_for(lambda: (lambda w: "Start now" in w and w)(control.dialog_texts()), 30) or control.dialog_texts()
        cc.check(f"{name}: Download details opens with the part on and its ends",
                 "Only a part of the video" in words and PART[0] in words and PART[1] in words, str(words[:30]))
    else:
        cc.check(f"{name}: its one stream is handed at once", outcome.get("title") == "Sent to dm", str(outcome))
        control = Control(engine_home)
        words = cc.wait_for(lambda: (lambda w: "Start now" in w and w)(control.dialog_texts()), 30) or control.dialog_texts()
        if not cc.check(f"{name}: Download details opens and offers a part", "Only a part of the video" in words,
                        str(words[:30])):
            return
        # As a person: the box ticked, then each end typed into its field ("m:ss" until typed).
        # The words on screen come front first, the end's field before the start's: with both
        # empty the start is the second "m:ss", and once it is typed the end's is the only one.
        for step in (("ui.click", {"text": "Only a part of the video"}), ("ui.click", {"text": "m:ss", "nth": 1}),
                     ("ui.type", {"text": PART[0]}), ("ui.click", {"text": "m:ss"}), ("ui.type", {"text": PART[1]})):
            control.call(step[0], **step[1])
            time.sleep(0.6)
        words = control.dialog_texts()
        cc.check(f"{name}: the part's ends are typed in Download details", PART[0] in words and PART[1] in words,
                 str(words[:30]))
    control.call("ui.click", text="Start now")
    landed = cc.wait_for(lambda: [p for p in engine_dl.rglob("*") if p.is_file() and p not in before
                                  and p.suffix.lower() in (".mp4", ".ts", ".mkv", ".webm")], 240) or []
    if not cc.check(f"{name}: the part lands in dm's folder", bool(landed), str(sorted(engine_dl.rglob("*"))[-5:])):
        return
    info = media_check.inspect(str(landed[0]))
    seconds = float(info.get("duration") or 0)
    kinds = sorted({t.get("kind") for t in info.get("tracks", [])})
    cc.check(f"{name}: it is a picture, about as long as the part ({SPAN:.0f} s)",
             not info.get("problems") and "video" in kinds and page["ok"](seconds),
             f"{landed[0].name}: {seconds:.1f} s, {kinds}, {info.get('problems', [])[:2]}")


def parts(base, origin, engine_dl, engine_home, worker):
    # Download details on, as a new install has it, and the port the check answers it through;
    # written before the app first starts, at the first hand-off below.
    settings = engine_home / "settings.json"
    kept = json.loads(settings.read_text())
    kept.update({"ask_panel_downloads": True, "allow_command_line": True})
    settings.write_text(json.dumps(kept))
    wanted = [a for a in sys.argv[1:] if a in PAGES] or list(PAGES)
    for name in wanted:
        print(f"\n{name}: {PAGES[name]['url']}", flush=True)
        part_of(name, PAGES[name], base, engine_dl, engine_home, worker)


os.environ["DM_SKIP_LOCAL"] = "1"
cc.panel_checks = parts
cc.main()
