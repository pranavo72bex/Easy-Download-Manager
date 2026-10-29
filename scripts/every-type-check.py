"""Every type of file a person clicks goes to dm, with dm set as installed.

    sh scripts/package.sh                      # builds dist/chrome
    (cd desktop && cargo build -p dm-desktop)  # the app the host starts
    CHROME_PATH=/path/to/msedge python3 scripts/every-type-check.py

chrome-check.py sets the app to catch `.bin` and `.pdf` only, to check that a
list a person chose is kept; this runs all of its checks too, but replaces
its panel step with the app's own settings as a new install has them (no
list: every type) and a real click on a link to each kind of file below.
Each must land whole in dm's folder while the browser keeps nothing. In a
window, as a person sees it (docs/behaviour-contract.md, B37).

The extension keeps what the app said for a minute, so after the settings
change this waits that minute before the first click, as a person who has
just changed Options would.
"""
import importlib.util
import json
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("chrome_check", HERE / "chrome-check.py")
cc = importlib.util.module_from_spec(spec)
sys.modules["chrome_check"] = cc
spec.loader.exec_module(cc)

# One of each kind a person downloads, and two the browser cannot name.
KINDS = ["firmware.bin", "readme.txt", "archive.zip", "setup.exe", "disk.dmg", "system.iso",
         "data.json", "table.csv", "song.mp3", "clip.mp4", "photo.jpg", "sheet.xlsx",
         "unknown.xyz", "noextension"]


def every_type(base, origin, engine_dl, engine_home, worker):
    settings_file = engine_home / "settings.json"
    settings = json.loads(settings_file.read_text())
    settings.pop("catch", None)                     # the app's own default: no list
    settings_file.write_text(json.dumps(settings))
    print("  dm set as installed (no type list); waiting a minute for the extension to ask it again", flush=True)
    time.sleep(62)
    browser_dl = engine_home.parent / "browser-downloads"
    tab = cc.open_tab(base, f"{origin}/page.html")
    cc.loaded(tab, "/page.html", '[id="caught.bin"]')
    for name in KINDS:
        tab.evaluate(f"""(() => {{ const p = document.createElement('p'); const a = document.createElement('a');
            a.id = {json.dumps('every-' + name)}; a.href = '/files/{name}'; a.textContent = {json.dumps(name)};
            p.appendChild(a); document.body.appendChild(p); return 1; }})()""")
    for name in KINDS:
        x, y = tab.evaluate(f"""(() => {{ const a = document.getElementById({json.dumps('every-' + name)});
            a.scrollIntoView({{block: 'center'}}); const r = a.getBoundingClientRect();
            return [r.left + 5, r.top + r.height / 2]; }})()""")
        cc.click_at(tab, x, y)
        taken = cc.wait_for(lambda: cc.whole(engine_dl, name), 60)
        time.sleep(1)
        kept = sorted(p.name for p in browser_dl.iterdir() if p.name.startswith(name.split(".")[0]))
        cc.check(f"a click on {name} goes to dm and the browser keeps nothing",
                 bool(taken) and not kept, f"in dm: {bool(taken)}; in the browser's folder: {kept}")


cc.panel_checks = every_type
cc.main()
