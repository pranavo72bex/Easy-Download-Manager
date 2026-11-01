#!/usr/bin/env python3
"""The "Download video" panel on real video sites other than YouTube, by hand only.

Like youtube-check.py this goes to the real sites, so it is not in CI. It
reuses chrome-check.py's setup (a visible browser with dm's extension and the
app behind it) and, for each site, opens a public page that plays without a
sign-in, plays the video, hovers it, reads the panel and its list, clicks the
first line and waits for the file to land in dm's folder, then reads the
file's own structure: is there a picture, is there a sound, how long is it.

    python3 scripts/sites-check.py [site ...]        # vimeo facebook tiktok x x2 reddit instagram

No credentials are used and nobody is signed in: a site that puts a login wall
in front of its video is reported as that, not worked around.
"""
import importlib.util
import base64
import json
import os
import pathlib
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

# Found on the listing page itself, for sites whose posts change daily.
REDDIT_JS = """(() => { const t = [...document.querySelectorAll('shreddit-post')].find(p =>
    p.getAttribute('post-type') === 'video' || (p.getAttribute('content-href') || '').includes('v.redd.it'));
  return t ? 'https://www.reddit.com' + t.getAttribute('permalink') : null; })()"""
INSTAGRAM_JS = """(() => { const a = [...document.querySelectorAll('a[href*="/reel/"], a[href*="/p/"]')][0];
  return a ? a.href : null; })()"""

SITES = {
    "vimeo": {"url": "https://vimeo.com/513177164"},
    "facebook": {"url": "https://www.facebook.com/NASA/videos/watch-nasas-perseverance-rover-land-on-mars/438313177583525/"},
    "tiktok": {"url": "https://www.tiktok.com/@nasa/video/7675478347278863629"},
    # A GIF-style clip: the post's media is a short silent MP4.
    "x": {"url": "https://x.com/NASA/status/1882872996031066248", "silent_ok": True},
    "x2": {"url": "https://x.com/NASA/status/2042756933686337713"},
    "reddit": {"listing": "https://www.reddit.com/r/oddlysatisfying/top/?t=week", "discover": REDDIT_JS},
    "instagram": {"listing": "https://www.instagram.com/nasa/reels/", "discover": INSTAGRAM_JS},
}
WANT = sys.argv[1:] or list(SITES)

RECT = """(() => { const v = [...document.querySelectorAll('video')]
    .sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0];
  if (!v) return null; v.scrollIntoView({block: 'center'}); const r = v.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, paused: v.paused, src: (v.currentSrc || v.src || '').slice(0, 120) }; })()"""
CONSENT = """(() => { const words = /^(accept all|accept|allow all|allow all cookies|agree|i agree|ok|got it|accept cookies|decline optional cookies|continue)$/i;
  for (const b of document.querySelectorAll('button, [role=button]')) {
    if (words.test((b.innerText || '').trim())) { b.click(); return (b.innerText || '').trim(); } }
  return null; })()"""
COVER = """(() => { const v = [...document.querySelectorAll('video')]
    .sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0];
  if (!v) return null; const r = v.getBoundingClientRect();
  const desc = e => { if (!e) return null; const b = e.getBoundingClientRect(); const st = getComputedStyle(e);
    return `${e.tagName.toLowerCase()}.${(e.className || '').toString().slice(0, 40)} ${Math.round(b.width)}x${Math.round(b.height)} pos=${st.position} op=${st.opacity} pe=${st.pointerEvents}`; };
  let top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
  const chain = []; for (let e = v.parentElement, i = 0; e && i < 5; e = e.parentElement, i++) chain.push(desc(e));
  const topChain = []; for (let e = top, i = 0; e && i < 4; e = e.parentElement, i++) topChain.push(desc(e));
  return { video: desc(v), top: topChain, around: chain, inVideo: top === v || v.contains(top) }; })()"""
PLAY = """(() => { const v = document.querySelector('video'); if (!v) return 'none'; v.muted = true;
  const p = v.play(); return p && p.then ? 'asked' : 'played'; })()"""


class FrameView:
    """A frame of the page, reached through the page's own DevTools target
    (a same-site iframe is not a target of its own): the DOM is shared with
    the extension's page script, so its panel can be read from here."""

    def __init__(self, tab, want):
        def find(node):
            if want in node["frame"]["url"]:
                return node["frame"]["id"]
            for child in node.get("childFrames", []):
                found = find(child)
                if found:
                    return found
        frame_id = find(tab.call("Page.getFrameTree")["frameTree"])
        if not frame_id:
            raise LookupError(want)
        self.tab = tab
        self.context = tab.call("Page.createIsolatedWorld", frameId=frame_id, worldName="dm-sites")["executionContextId"]

    def evaluate(self, expression, gesture=False):
        result = self.tab.call("Runtime.evaluate", expression=expression, awaitPromise=True, returnByValue=True,
                               userGesture=gesture, contextId=self.context)
        if "exceptionDetails" in result:
            raise RuntimeError(result["exceptionDetails"].get("text", "evaluate failed"))
        return result["result"].get("value")


MEDIA = (".mp4", ".webm", ".mkv", ".m4a", ".mp3", ".ts")


def inspect(path):
    info = media_check.inspect(str(path))
    kinds = sorted({t.get("kind") for t in info["tracks"]})
    heights = [t.get("height") for t in info["tracks"] if t.get("kind") == "video"]
    return {"problems": info["problems"][:2], "seconds": info["duration"], "kinds": kinds, "height": heights[0] if heights else None}


PROBE = bool(os.environ.get("SITES_PROBE"))


def media_seen(tab):
    """What the page loaded that looks like media, for working out why the
    panel is silent: the address without its query values (they carry
    signatures), the type, the status, and the few parameters that name a
    piece of a stream. Needs `Network.enable` before the page loads."""
    tab.drain(1)
    rows, seen = [], set()
    for event in tab.events:
        if event.get("method") != "Network.responseReceived":
            continue
        r = event["params"]["response"]
        url, mime = r.get("url", ""), (r.get("mimeType") or "").lower()
        path = url.split("?")[0].lower()
        if not (mime.startswith(("video/", "audio/")) or "mpegurl" in mime or "dash" in mime
                or path.endswith((".m3u8", ".mpd", ".mp4", ".m4s", ".webm", ".ts", ".m4a"))
                or event["params"].get("type") == "Media"):
            continue
        parts = urllib.parse.urlsplit(url)
        q = urllib.parse.parse_qs(parts.query)
        keep = {k: q[k][0][:24] for k in ("bytestart", "byteend", "range", "rn", "itag", "mime") if k in q}
        efg = q.get("efg", [""])[0]
        if efg:
            try:
                keep["efg"] = json.loads(base64.urlsafe_b64decode(efg + "=" * (-len(efg) % 4))).get("vencode_tag", "?")
            except Exception:
                keep["efg"] = "?"
        key = (parts.netloc, parts.path, tuple(sorted(keep.items())))
        if key in seen:
            continue
        seen.add(key)
        length = (r.get("headers") or {}).get("content-length") or (r.get("headers") or {}).get("Content-Length")
        rows.append(f"{r.get('status')} {mime or '-'} len={length} {parts.netloc}{parts.path[:70]} {keep or ''}")
    return rows[:30]


def one(base, name, site, engine_dl, worker=None):
    out = {"site": name}
    url = site.get("url")
    tab = None
    if not url:
        listing = cc.open_tab(base, site["listing"])
        listing.call("Runtime.enable")
        cc.wait_for(lambda: listing.evaluate("document.readyState") == "complete", 30)
        url = cc.wait_for(lambda: listing.evaluate(site["discover"]), 25)
        out["found"] = url
        if not url:
            try:
                out["body"] = listing.evaluate("document.body.innerText.slice(0, 200)")
            except RuntimeError:
                out["body"] = "(the page changed under the check)"
            out["result"] = "no public video post found on the listing (login wall or bot check)"
            return out
    tab = cc.open_tab(base, url)
    tab.call("Runtime.enable")
    if PROBE:
        tab.call("Network.enable")
        tab.call("Page.reload")
    out["url"] = url
    cc.wait_for(lambda: tab.evaluate("document.readyState") == "complete", 40)
    time.sleep(3)
    out["consent"] = tab.evaluate(CONSENT)
    view, ox, oy = tab, 0, 0
    rect = cc.wait_for(lambda: (tab.evaluate(RECT) or {}).get("w", 0) > 150 and tab.evaluate(RECT), 12 if site.get("frame") else 40)
    if not rect and site.get("frame"):
        want = site["frame"]
        frame = cc.wait_for(lambda: next((t for t in cc.targets(base) if t["type"] == "iframe" and want in t["url"]), None), 3)
        try:
            view = cc.Target(frame["webSocketDebuggerUrl"]) if frame else FrameView(tab, want)
            if frame:
                view.call("Runtime.enable")
        except LookupError:
            view = tab
        if view is not tab:
            at = tab.evaluate("(() => { const f = [...document.querySelectorAll('iframe')].find(f => f.src.includes(%r));"
                              " if (!f) return null; f.scrollIntoView({block: 'center'}); const r = f.getBoundingClientRect();"
                              " return [r.left, r.top]; })()" % want)
            ox, oy = at or (0, 0)
            rect = cc.wait_for(lambda: (view.evaluate(RECT) or {}).get("w", 0) > 150 and view.evaluate(RECT), 40)
            out["in_frame"] = True
    out["title"] = tab.evaluate("document.title")[:80]
    if not rect:
        if PROBE:
            out["net"] = media_seen(tab)
            out["frames"] = tab.evaluate("[...document.querySelectorAll('iframe')].map(f => f.src.split('?')[0]).slice(0, 6)")
            out["body"] = tab.evaluate("document.body.innerText.slice(0, 160)")
        out["result"] = "no video player appeared (login wall, bot check or the page needs a click)"
        return out
    out["player"] = {k: rect[k] for k in ("w", "h", "src")}
    view.evaluate(PLAY, gesture=True)
    time.sleep(3)
    rect = view.evaluate(RECT) or rect
    page_rect = lambda r: {**r, "x": r["x"] + ox, "y": r["y"] + oy}
    out["playing"] = not rect.get("paused", True)
    if PROBE:
        time.sleep(5)
        out["net"] = media_seen(tab)
    if PROBE and worker is not None:
        out["worker_saw"] = worker.evaluate("globalThis.__dmseen.slice(-14)")
    cc.hover(tab, page_rect(rect))
    state = cc.wait_for(lambda: (cc.panel(view) or {}).get("shown") and cc.panel(view), 10)
    if not state:
        cc.hover(tab, page_rect(rect))
        time.sleep(2)
        state = cc.wait_for(lambda: (cc.panel(view) or {}).get("shown") and cc.panel(view), 10)
    out["panel_shown"] = bool(state)
    if not state:
        out["cover"] = view.evaluate(COVER)
        out["result"] = "the panel did not appear"
        return out
    before = {p for p in engine_dl.rglob("*") if p.is_file()}
    cc.click_at(tab, state["x"] + 60 + ox, state["y"] + state["h"] / 2 + oy)
    # A player on a plain file address is handed over by the click itself; a
    # blob or stream player lists its qualities first.
    def landed():
        return [p for p in engine_dl.rglob("*") if p.is_file() and p not in before and p.suffix in MEDIA]

    after = cc.wait_for(lambda: (((cc.panel(view) or {}).get("lines") or (cc.panel(view) or {}).get("title") == "Sent to dm")
                                 and cc.panel(view)) or (landed() and {"landed": True}), 60) or {}
    out["lines"] = after.get("lines")
    if after.get("lines"):
        x, y = after["at"][0]
        cc.click_at(tab, x + ox, y + oy)
    elif after.get("title") != "Sent to dm" and not after.get("landed"):
        out["panel_after"] = cc.panel(view)
        out["result"] = "the panel listed no qualities"
        return out
    said = cc.wait_for(lambda: (cc.panel(view) or {}).get("title") == "Sent to dm" and cc.panel(view), 20)
    out["sent"] = bool(said)
    got = cc.wait_for(landed, 240)
    if not got:
        out["result"] = "the download did not finish into dm's folder"
        return out
    out["file"] = got[0].name
    out["size"] = got[0].stat().st_size
    out["media"] = inspect(got[0])
    m = out["media"]
    sound = "audio" in m["kinds"] or site.get("silent_ok")
    out["result"] = "OK" if (not m["problems"] and m["seconds"] and "video" in m["kinds"] and sound) else (
        f"saved but not a whole video with sound: {m}")
    return out


def sites(base, origin, engine_dl, engine_home, worker):
    results = []
    if PROBE:
        # What the extension's own worker is told, tab or no tab.
        worker.evaluate("""(() => { globalThis.__dmseen = []; chrome.webRequest.onResponseStarted.addListener(d => {
          const u = d.url.split('?')[0];
          if (/[.](m3u8|mpd|mp4|m4s|ts)$/i.test(u) || d.type === 'media')
            globalThis.__dmseen.push([d.tabId, d.type, d.statusCode, d.frameId, (d.initiator || '').slice(0, 40), u.slice(0, 100)]); },
          { urls: ['<all_urls>'] }, []); return 1; })()""")
    for name in WANT:
        try:
            r = one(base, name, SITES[name], engine_dl, worker)
        except Exception as e:  # one site breaking must not stop the others
            r = {"site": name, "result": f"check failed: {e!r}"}
        results.append(r)
        print("SITE", json.dumps(r, default=str), flush=True)
        cc.check(f"{name}: a whole video with sound lands in dm's folder", r.get("result") == "OK", r.get("result", ""))
    print("\nSUMMARY")
    for r in results:
        print(f"  {r['site']:12} {r.get('result')}   lines={r.get('lines')}")


cc.panel_checks = sites
cc.main()
