#!/usr/bin/env python3
"""The video panel's hard cases, in Chromium over the DevTools protocol.

What `chrome-check.py` does not draw: a right-to-left page, a transparent
video, a dialog over the player, keyboard use, fullscreen (including on a
component whose shadow root has no slot), a page whose CSP forbids
WebAssembly (Alt-click and the links responder still answer), a player that
keeps its <video> in a shadow root under a control layer, and a vertical
clip letterboxed in a wide player.

    python3 scripts/panel-check.py      (from the repository root;
                                         CHROME_PATH names Chromium)

It reuses chrome-check.py's browser, server and dm set-up, so it needs what
that needs: the extension built (sh scripts/package.sh) and dm built.
"""
import importlib.util, json, os, pathlib, shutil, signal, subprocess, sys, tempfile, threading, time

ROOT = pathlib.Path.cwd()
spec = importlib.util.spec_from_file_location("cc", ROOT / "scripts/chrome-check.py")
cc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cc)

V = '<video id="v" src="/files/clip.mp4" width="640" height="360" preload="none" {a}></video>'
PAGES = {
    "/rtl.html": ('<!doctype html><html dir="rtl"><title>rtl</title><div style="position:relative;margin:40px">'
                  + V.format(a="") + "</div>", None),
    "/ghost.html": ('<!doctype html><title>ghost</title><div style="position:relative;margin:40px">'
                    + V.format(a='style="opacity:0"') + "</div>", None),
    "/covered.html": ('<!doctype html><title>covered</title><div style="position:relative;margin:40px">'
                      + V.format(a="") + '</div><div style="position:absolute;left:0;top:0;width:900px;height:600px;'
                      'background:#fff;z-index:9">a dialog over the page</div>', None),
    "/keys.html": ('<!doctype html><title>keys</title><button id="before">before</button>'
                   '<div id="box" style="position:relative;margin:40px">'
                   + V.format(a='tabindex="0"') + '</div><button id="after">after</button>', None),
    "/full.html": ('<!doctype html><title>full</title><div id="box" style="position:relative;margin:40px;background:#000">'
                   + V.format(a="") + "</div>", None),
    "/nowasm.html": ('<!doctype html><title>no wasm</title><p><a id="l" href="/files/alt.bin">alt.bin</a>'
                     ' <a href="/files/two.bin">two</a></p><div style="position:relative;margin:40px">'
                     + V.format(a="") + "</div>",
                     "default-src 'none'; media-src 'self'; script-src 'none'; style-src 'none'"),
    "/mux.html": ('<!doctype html><title>mux-like</title><x-player id="xp" style="display:block;margin:40px;width:640px;height:360px"></x-player>'
                  '<script>customElements.define("x-video", class extends HTMLElement { connectedCallback() {'
                  ' this.attachShadow({mode: "open"}).innerHTML = \'<video src="/files/mux.mp4" width="640" height="360" preload="none" style="display:block"></video>\'; } });'
                  'customElements.define("x-player", class extends HTMLElement { connectedCallback() {'
                  ' this.attachShadow({mode: "open"}).innerHTML = \'<div style="position:relative;width:640px;height:360px">'
                  '<x-video style="display:block"></x-video><div style="position:absolute;inset:0;z-index:2"></div></div>\'; } });</script>', None),
    "/letterbox.html": ('<!doctype html><title>letterbox</title><div id="lb" style="position:relative;margin:20px;width:960px;height:540px;background:#000">'
                        '<video src="/files/tall.mp4" width="304" height="540" preload="none" style="position:absolute;left:328px;top:0"></video>'
                        '<div style="position:absolute;inset:0;z-index:2"></div></div>', None),
    "/shadowfull.html": ('<!doctype html><title>shadow fullscreen</title><x-full id="xf" style="display:block;margin:40px;width:640px;height:360px"></x-full>'
                         '<script>customElements.define("x-full", class extends HTMLElement { connectedCallback() {'
                         ' this.attachShadow({mode: "open"}).innerHTML = \'<div style="position:relative;width:640px;height:360px">'
                         '<video src="/files/sf.mp4" width="640" height="360" preload="none"></video></div>\'; } });</script>', None),
}
cc.PANEL_PAGES.update(PAGES)

STATE = """(() => { const xf = document.getElementById('xf');
  const h = document.querySelector('dm-panel') || (xf && xf.shadowRoot && xf.shadowRoot.querySelector('dm-panel'));
  if (!h) return null;
  const r = h.shadowRoot, go = r.querySelector('.go'), x = r.querySelector('.close');
  return { shown: getComputedStyle(h).display !== 'none', dir: getComputedStyle(h).direction,
    goLeft: go.getBoundingClientRect().left, closeLeft: x.getBoundingClientRect().left,
    parent: h.parentNode instanceof ShadowRoot ? 'shadow:' + h.parentNode.host.id : (h.parentElement && (h.parentElement.id || h.parentElement.tagName)),
    active: r.activeElement ? r.activeElement.className : '',
    label: go.getAttribute('aria-label'), name: r.querySelector('.title').textContent,
    live: r.querySelector('.live').textContent }; })()"""


def key(tab, name, code, vk, shift=False):
    mods = 8 if shift else 0
    for kind in ("keyDown", "keyUp"):
        extra = {"text": "\r"} if (name == "Enter" and kind == "keyDown") else {}
        tab.call("Input.dispatchKeyEvent", type=kind, key=name, code=code,
                 windowsVirtualKeyCode=vk, modifiers=mods, **extra)
    time.sleep(0.3)


def main():
    chrome = cc.find_chrome()
    work = pathlib.Path(tempfile.mkdtemp(prefix="dm-panel-extra-"))
    ext, profile = work / "ext", work / "profile"
    (profile / "Default").mkdir(parents=True)
    shutil.copytree(cc.EXT, ext)
    server = cc.Server(("127.0.0.1", 0), cc.Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_address[1]}"
    args = [str(chrome), f"--user-data-dir={profile}", f"--load-extension={ext}",
            f"--disable-extensions-except={ext}", "--remote-debugging-port=0", "--no-first-run",
            "--no-default-browser-check", "--disable-gpu",
            "--disable-features=DisableLoadExtensionCommandLineSwitch,HttpsUpgrades", "about:blank"]
    if os.geteuid() == 0:
        args.insert(1, "--no-sandbox")
    browser = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, preexec_fn=os.setpgrp)
    check = cc.check
    try:
        base = cc.devtools(profile)
        worker_t = cc.wait_for(lambda: [t for t in cc.targets(base) if t["type"] == "service_worker"
                                        and t["url"].endswith("/background.js")], 30)
        worker = cc.Target(worker_t[0]["webSocketDebuggerUrl"])
        cc.wait_for(lambda: worker.evaluate("typeof dmReady !== 'undefined' && dmReady.then(() => true)"), 20)

        def page(path):
            tab = cc.open_tab(base, origin + path)
            cc.loaded(tab, path)
            time.sleep(1.5)
            return tab

        def hover(tab, sel=None):
            rect = cc.VIDEO_RECT if sel is None else (
                f"(() => {{ const r = ({sel}).getBoundingClientRect();"
                " return { x: r.left, y: r.top, w: r.width, h: r.height }; })()")
            cc.hover(tab, tab.evaluate(rect))
            return cc.wait_for(lambda: (tab.evaluate(STATE) or {}).get("shown") and tab.evaluate(STATE), 3) \
                or tab.evaluate(STATE) or {}

        tab = page("/rtl.html")
        s = hover(tab)
        check("RTL page: the panel shows, left to right (× right of the bar)",
              s.get("shown") and s.get("dir") == "ltr" and s["closeLeft"] > s["goLeft"], str(s))

        tab = page("/ghost.html")
        s = hover(tab)
        check("a transparent video is offered no panel", not s.get("shown"), str(s))

        tab = page("/covered.html")
        s = hover(tab)
        check("a video under an unrelated page layer is offered no panel", not s.get("shown"), str(s))

        tab = page("/keys.html")
        tab.evaluate("document.getElementById('before').focus()")
        key(tab, "Tab", "Tab", 9)
        s = cc.wait_for(lambda: (tab.evaluate(STATE) or {}).get("shown") and tab.evaluate(STATE), 3) or tab.evaluate(STATE) or {}
        check("keyboard: focus on the video raises the panel", s.get("shown"), str(s))
        key(tab, "Tab", "Tab", 9)
        s = tab.evaluate(STATE) or {}
        check("keyboard: Tab from the video goes into the panel's bar", s.get("active") == "go", str(s))
        check("the bar is named by what it says (no fixed aria-label)",
              s.get("label") is None and s.get("name") == "Download video", str(s))
        time.sleep(1.0)
        s = tab.evaluate(STATE) or {}
        check("it stays up while the focus is in it", s.get("shown"), str(s))
        key(tab, "Tab", "Tab", 9); key(tab, "Tab", "Tab", 9)
        s = tab.evaluate(STATE) or {}
        check("Tab walks ? and ×", s.get("active") == "close", str(s))
        key(tab, "Enter", "Enter", 13)
        s = tab.evaluate(STATE) or {}
        items = tab.evaluate("[...document.querySelector('dm-panel').shadowRoot.querySelectorAll('.item')].map(i => i.textContent)")
        check("Enter on × opens its menu with the focus on its first line",
              s.get("active") == "item" and items and items[0] == "Hide for now", f"{s} {items}")
        key(tab, "ArrowDown", "ArrowDown", 40)
        focused = tab.evaluate("document.querySelector('dm-panel').shadowRoot.activeElement.textContent")
        check("ArrowDown moves down the menu", (focused or "").startswith("Never on"), focused)
        key(tab, "Escape", "Escape", 27)
        s = tab.evaluate(STATE) or {}
        open_ = tab.evaluate("!document.querySelector('dm-panel').shadowRoot.querySelector('.list').hidden")
        check("Escape closes the menu, back on the bar", s.get("active") == "go" and not open_, str(s))
        key(tab, "Escape", "Escape", 27)
        time.sleep(0.5)
        s = tab.evaluate(STATE) or {}
        on_video = tab.evaluate("document.activeElement && document.activeElement.id")
        check("Escape again hides the panel, the focus back on the video",
              not s.get("shown") and on_video == "v", f"{s} {on_video}")
        key(tab, "Tab", "Tab", 9)
        after = tab.evaluate("document.activeElement && document.activeElement.id")
        check("and Tab goes on through the page", after in ("after", "v"), after)

        tab = page("/full.html")
        tab.evaluate("document.getElementById('box').requestFullscreen()", gesture=True)
        time.sleep(1.0)
        s = hover(tab)
        check("fullscreen: the panel shows inside the fullscreen element",
              s.get("shown") and s.get("parent") == "box", str(s))
        tab.evaluate("document.exitFullscreen()", gesture=True)
        time.sleep(1.0)
        s = tab.evaluate(STATE) or {}
        check("and goes back to the page's body after", s.get("parent") == "BODY", str(s))

        tab = page("/mux.html")
        s = hover(tab, "document.querySelector('#xp')")
        check("a video in a shadow root under a control layer in another tree gets the panel",
              s.get("shown"), str(s))

        tab = page("/letterbox.html")
        s = hover(tab, "document.querySelector('#lb')")
        check("a vertical clip letterboxed in a wide player, under a player-wide layer, gets the panel",
              s.get("shown"), str(s))

        tab = page("/shadowfull.html")
        tab.evaluate("document.getElementById('xf').requestFullscreen()", gesture=True)
        time.sleep(1.0)
        s = hover(tab, "document.getElementById('xf').shadowRoot.querySelector('video')")
        check("fullscreen on a shadow host: the panel goes in its shadow root and shows",
              s.get("shown") and s.get("parent") == "shadow:xf", str(s))
        tab.evaluate("document.exitFullscreen()", gesture=True)
        time.sleep(1.0)

        tab = page("/nowasm.html")
        tab.call("Runtime.enable")
        has_panel = tab.evaluate("!!document.querySelector('dm-panel')")
        print(f"  (info) panel element on a script-src 'none' page: {has_panel}")
        tab_id = cc.wait_for(lambda: worker.evaluate(
            "chrome.tabs.query({active: true, lastFocusedWindow: true}).then(ts => ts[0] && ts[0].id)"), 5)
        links = worker.evaluate(f"chrome.tabs.sendMessage({tab_id}, {{type: 'dm.links', selected: false}}).then(r => r.links)")
        check("the page's links are answered by the plain page script",
              links and len(links) == 2 and links[0].endswith("/files/alt.bin"), str(links))
        # Alt-click: the content script's message reaches the worker, which
        # remembers the URL; read it back by Alt-clicking and asking the
        # worker's own listener through a page-message spy.
        worker.evaluate("""globalThis.__skips = []; chrome.runtime.onMessage.addListener((m) => {
            if (m && m.type === 'dm.skip') globalThis.__skips.push(m.src); return false; }); true""")
        r = tab.evaluate("(() => { const b = document.getElementById('l').getBoundingClientRect(); return [b.left + 5, b.top + 5]; })()")
        for kind in ("mouseMoved", "mousePressed", "mouseReleased"):
            p = {"type": kind, "x": r[0], "y": r[1], "button": "left", "modifiers": 1}
            if kind != "mouseMoved":
                p["clickCount"] = 1
            tab.call("Input.dispatchMouseEvent", **p)
        skips = cc.wait_for(lambda: worker.evaluate("globalThis.__skips.length && globalThis.__skips"), 5)
        check("Alt-click is said by the plain page script", bool(skips) and skips[0].endswith("/files/alt.bin"), str(skips))
    finally:
        os.killpg(os.getpgid(browser.pid), signal.SIGTERM)
        server.shutdown()
    failed = [r for r in cc.results if not r[0]]
    print(f"\n{len(cc.results) - len(failed)} of {len(cc.results)} checks passed")
    sys.exit(1 if failed else 0)


main()
