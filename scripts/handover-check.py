#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""Real download pages: their own Download buttons, handed to dm by the extension.

    python3 scripts/handover-check.py [site ...]     # every site below, or those named

The other half of docs/site-list.md (gaps row 152) beside `sites-check.py`'s videos:
does the extension take a download a real page starts, out of the box, whatever way the
page starts it? A plain link, a button that runs a script, a page that counts down and
then starts it, a redirect to a mirror or to a signed address on another host. Like
sites-check.py it goes to the real sites, so it is run by hand, never in CI; it reuses
chrome-check.py's setup (a visible browser with dm's extension and the app behind it),
with the app's catch rules put back to a new install's (every type) for these sites.

For each site: the page is opened, its button clicked as a person clicks it (or nothing,
where the page starts the download itself), and the check waits for a row in dm for it
that is receiving bytes, and for the browser to have kept nothing. A file is not waited
for to the end: the first bytes are the hand-over working. No account is used; a page
that answers an automated browser with a challenge is reported as that.
"""
import importlib.util
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


def link(pattern, scroll=True):
    """The first link whose address matches `pattern`, made visible (unless `scroll` is off),
    named by its place, and whether a click at that place reaches it."""
    return ("(() => { const re = new RegExp(%s, 'i'); const a = [...document.querySelectorAll('a[href]')]"
            ".find(a => re.test(a.href)); if (!a) return null;%s"
            " const r = a.getBoundingClientRect(); const x = r.left + r.width / 2, y = r.top + r.height / 2;"
            " return {href: a.href, x, y, w: r.width, h: r.height, hit: a.contains(document.elementFromPoint(x, y))};"
            " })()") % (json.dumps(pattern), " a.scrollIntoView({block: 'center'});" if scroll else "")


# What each page needs: its address and the address pattern of the link its Download button
# is, or None where the page starts the download itself (a thank-you page, a countdown, a
# mirror's refresh).
SITES = {
    "7-zip": {"url": "https://www.7-zip.org/download.html", "click": r"/a/7z[0-9]+(-x64)?\.exe$"},
    "python": {"url": "https://www.python.org/downloads/macos/", "click": r"/ftp/python/[0-9.]+/python-[0-9.]+-macos11\.pkg$"},
    "nodejs": {"url": "https://nodejs.org/en/download/prebuilt-installer", "click": r"nodejs\.org/dist/v[0-9.]+/node-v[0-9.]+\.pkg$"},
    "github-release": {"url": "https://github.com/cli/cli/releases/latest", "click": r"/releases/download/v[0-9.]+/gh_[0-9.]+_macOS_arm64\.zip$"},
    # GitHub draws "Download ZIP" only in the Code menu, which a person opens first.
    "github-zip": {"url": "https://github.com/octocat/Hello-World",
                   "open": "[...document.querySelectorAll('button')].find(b => /^\\s*Code\\s*$/.test(b.innerText || ''))",
                   "click": r"/archive/refs/heads/master\.zip$"},
    "sourceforge": {"url": "https://sourceforge.net/projects/sevenzip/files/latest/download", "click": None},
    "mozilla": {"url": "https://www.mozilla.org/en-US/firefox/download/thanks/", "click": None},
    "videolan": {"url": "https://get.videolan.org/vlc/3.0.21/macosx/vlc-3.0.21-arm64.dmg", "click": None},
    "audacity": {"url": "https://www.audacityteam.org/download/mac/", "click": r"\.dmg$"},
    "gimp": {"url": "https://www.gimp.org/downloads/", "click": r"download\.gimp\.org/.*\.dmg$"},
    "libreoffice": {"url": "https://www.libreoffice.org/download/download-libreoffice/",
                    "click": r"download\.documentfoundation\.org/libreoffice/stable/[0-9.]+/mac/aarch64/.*\.dmg$"},
    # Started by the page's script, through a redirect to Microsoft's CDN.
    "vscode": {"url": "https://code.visualstudio.com/docs/?dv=darwinarm64", "click": None},
}
WANT = sys.argv[1:] or list(SITES)
CONSENT = """(() => { const words = /^(accept all|accept|allow all|allow all cookies|agree|i agree|ok|got it|accept cookies|continue)$/i;
  const b = [...document.querySelectorAll('button, a[role=button], input[type=button]')].find(e => words.test((e.innerText || e.value || '').trim()));
  if (b) { b.click(); return (b.innerText || b.value || '').trim(); } return null; })()"""
BROWSER_KEPT = """chrome.downloads.search({orderBy: ['-startTime'], limit: 30}).then(list => list
  .filter(d => !/127\\.0\\.0\\.1|localhost/.test(d.finalUrl || d.url))
  .map(d => ({url: (d.finalUrl || d.url).slice(0, 140), state: d.state, exists: d.exists, error: d.error || ''})))"""
DEFAULT_CATCH = {"enabled": True, "extensions": [], "defaultsVersion": 2}


def one(base, name, site, engine_home, worker):
    out = {"site": name, "url": site["url"]}
    before = {r["id"] for r in cc.queue_rows(engine_home)}
    kept_before = {d["url"] for d in worker.evaluate(BROWSER_KEPT) or []}
    tab = cc.open_tab(base, site["url"])
    tab.call("Runtime.enable")
    cc.wait_for(lambda: tab.evaluate("document.readyState") == "complete", 40)
    time.sleep(2)
    try:
        out["title"] = (tab.evaluate("document.title") or "")[:80]
        out["consent"] = tab.evaluate(CONSENT)
    except RuntimeError:
        # A page that starts its download navigates away under the check.
        pass
    if site.get("open"):
        # A menu the link is drawn in, opened as a person opens it.
        button = cc.wait_for(lambda: tab.evaluate(
            "(() => { const b = %s; if (!b) return null; b.scrollIntoView({block: 'center'});"
            " const r = b.getBoundingClientRect(); return {x: r.left + r.width / 2, y: r.top + r.height / 2}; })()"
            % site["open"]), 20)
        if button:
            cc.click_at(tab, button["x"], button["y"])
            time.sleep(1.5)
    if site["click"]:
        target = cc.wait_for(lambda: tab.evaluate(link(site["click"])), 20)
        if not target:
            out["result"] = "its Download link was not found (the page changed, or a challenge)"
            out["body"] = (tab.evaluate("document.body ? document.body.innerText.slice(0, 160) : ''") or "")
            return out
        out["link"] = target["href"][:140]
        # Scrolled once, then measured where it rests. A page can move under a scroll:
        # LibreOffice's menu bar, which stays at the top and is made shorter once the page is
        # scrolled, grows back by 63 pixels when the page is scrolled near its top, so the link
        # was measured before it moved and the click landed on the heading above it, which
        # does nothing (gaps row 173). A person clicks what they see, so the click waits for
        # the link to be in the same place twice running with nothing drawn over it, measured
        # without another scroll (which would move it again).
        last = [None]

        def resting():
            now = tab.evaluate(link(site["click"], scroll=False))
            before, last[0] = last[0], now
            if now and before and now["hit"] and abs(now["x"] - before["x"]) < 1 and abs(now["y"] - before["y"]) < 1:
                return now
            return None

        if target["w"] > 0 and target["h"] > 0:
            time.sleep(0.5)
            target = cc.wait_for(resting, 10) or last[0] or target
        if target["w"] > 0 and target["h"] > 0:
            if not target.get("hit"):
                out["covered"] = "the link was not the topmost thing at its middle"
            cc.click_at(tab, target["x"], target["y"])
        else:
            # Behind a menu that is closed (GitHub's Code button): a person opens the menu and
            # clicks the link, which is the link's own click.
            tab.call("Runtime.evaluate", userGesture=True, expression=(
                "(() => { const re = new RegExp(%s, 'i'); [...document.querySelectorAll('a[href]')]"
                ".find(a => re.test(a.href)).click(); })()") % json.dumps(site["click"]))

    def new_row():
        rows = [r for r in cc.queue_rows(engine_home) if r["id"] not in before]
        return rows[-1] if rows else None

    row = cc.wait_for(new_row, 90)
    if not row:
        kept = [d for d in worker.evaluate(BROWSER_KEPT) or [] if d["url"] not in kept_before]
        out["browser"] = kept
        out["result"] = ("the browser kept it: dm was not handed it" if kept
                         else "nothing started (the page did not start a download)")
        return out
    out["dm_url"] = row.get("url", "")[:140]

    def moving():
        now = next((r for r in cc.queue_rows(engine_home) if r["id"] == row["id"]), None)
        if not now:
            return None
        kind = now["state"]["kind"] if isinstance(now["state"], dict) else str(now["state"])
        if kind == "failed" or now["progress"]["downloaded"] > 0 or kind == "completed":
            return now
        return None

    got = cc.wait_for(moving, 60) or next((r for r in cc.queue_rows(engine_home) if r["id"] == row["id"]), {})
    state = got.get("state") or {}
    kind = state.get("kind") if isinstance(state, dict) else str(state)
    out["dm_state"] = kind
    out["dm_bytes"] = (got.get("progress") or {}).get("downloaded", 0)
    out["dm_name"] = pathlib.Path(got.get("path") or got.get("filename") or "").name
    if kind == "failed":
        out["dm_reason"] = (state.get("reason") or "")[:200]
    time.sleep(2)
    kept = [d for d in worker.evaluate(BROWSER_KEPT) or []
            if d["url"] not in kept_before and d["state"] != "interrupted" and d["exists"]]
    out["browser"] = kept
    if kind == "failed":
        out["result"] = "dm took it and failed: " + out.get("dm_reason", "")
    elif kept:
        out["result"] = "dm took it, and the browser kept a copy too"
    elif out["dm_bytes"] > 0 or kind == "completed":
        out["result"] = "OK"
    else:
        out["result"] = "dm took it, and no bytes came in a minute"
    return out


def sites(base, origin, engine_dl, engine_home, worker):
    # A new install's catch rules, which chrome-check's own checks narrowed to .bin and .pdf.
    settings = json.loads((engine_home / "settings.json").read_text())
    settings["catch"] = DEFAULT_CATCH
    (engine_home / "settings.json").write_text(json.dumps(settings))
    worker.evaluate("chrome.storage.local.set({'dm.catch': %s})" % json.dumps(DEFAULT_CATCH))
    worker.evaluate("dmReady.then(m => m.forget_app())")
    results = []
    for name in WANT:
        try:
            r = one(base, name, SITES[name], engine_home, worker)
        except Exception as e:  # one site breaking must not stop the others
            r = {"site": name, "result": f"check failed: {e!r}"}
        results.append(r)
        print("SITE", json.dumps(r, default=str), flush=True)
        cc.check(f"{name}: the page's download is handed to dm and arrives", r.get("result") == "OK",
                 r.get("result", ""))
    print("\nSUMMARY")
    for r in results:
        print(f"  {r['site']:15} {r.get('result')}   {r.get('dm_name') or ''} {r.get('dm_bytes') or ''}")


cc.panel_checks = sites
cc.main()
