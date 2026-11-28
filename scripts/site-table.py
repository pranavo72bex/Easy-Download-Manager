#!/usr/bin/env python3
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""The daily site watch's results, as the README's site table (gaps row W6).

youtube-check.py and sites-check.py, given `--json <file>`, write what each site
did there (this file's `write_result`, which they load). This turns one run's
files into the table between the README's marker comments, the issue a failing
run opens, and the counts the workflow (.github/workflows/sites.yml) acts on:

    python3 scripts/site-table.py youtube.json sites.json                 # the table, printed
    python3 scripts/site-table.py youtube.json sites.json --readme README.md
    python3 scripts/site-table.py youtube.json sites.json --out table.md \\
        --issue issue.md --log youtube.log --log sites.log               # what the workflow makes
    python3 scripts/site-table.py --self-test

The README is not changed by CI: the table of the last scheduled run is that run's
`site-table.md` artifact, and a person puts it in with

    gh run download "$(gh run list --workflow sites.yml --limit 1 --json databaseId --jq '.[0].databaseId')"         --name site-watch --dir site-watch
    python3 scripts/site-table.py site-watch/*.json --readme README.md

A site's status is one of: `pass` (a whole video with sound landed in EDM's folder),
`fail` (it did not: the site or EDM changed), `blocked` (a login wall or bot check
answered the check's browser, or the file was still arriving when the wait ran
out: nothing of EDM was judged, as video-sites-check.py judges it) and `error` (the
check stopped before it reached the site). `fail` and `error` open the issue.
"""
import argparse
import datetime
import json
import os
import pathlib
import platform
import re
import sys
import tempfile

# UTF-8 for this script's own output, whatever the console's code page (gaps row X4).
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

START = "<!-- site-table:start -->"
END = "<!-- site-table:end -->"

# The names a reader knows each of sites-check.py's sites by.
NAMES = {
    "youtube": "YouTube",
    "vimeo": "Vimeo",
    "tiktok": "TikTok",
    "x": "X (a short silent clip)",
    "x2": "X",
    "instagram": "Instagram",
    "dailymotion": "Dailymotion",
    "rumble": "Rumble",
    "twitch": "Twitch (a highlight)",
    "twitch-clip": "Twitch (a clip)",
    "bilibili": "Bilibili",
    "peertube": "PeerTube",
    "facebook": "Facebook",
    "facebook-watch": "Facebook Watch",
    "reddit": "Reddit",
    "reddit-videos": "Reddit",
}

WORDS = {
    "pass": "works",
    "fail": "**broken**",
    "blocked": "not judged",
    "error": "**not checked**",
}

# sites-check.py's own words for a page that showed the check no video at all, or a sign-in
# dialog over its player, and YouTube's for an address it will not serve (its player's
# "Sign in to confirm you're not a bot", LOGIN_REQUIRED; or UNPLAYABLE, "Video unavailable", which
# a runner's address was told of "Me at the zoo" on 2026-11-23, issue #56): nothing of EDM was tried.
BLOCKED_RESULTS = ("no public video post found", "no video player appeared", "not signed in within",
                   "a login wall covers the player")
BOT_CHECK = re.compile(r"not a bot|unusual traffic|prove your humanity|just a moment|verify you are human|captcha"
                       r"|sign in to confirm|login_required|player says unplayable", re.IGNORECASE)


def name_of(site):
    return NAMES.get(site, site)


def site_status(r):
    """One sites-check.py result (its `SITE` line) as a status, judged as
    video-sites-check.py judges it."""
    result = r.get("result") or ""
    if result == "OK":
        return "pass"
    if result.startswith("check failed"):
        return "error"
    if any(b in result for b in BLOCKED_RESULTS):
        return "blocked"
    for row in r.get("edm_rows") or []:
        if ((row.get("state") or {}).get("kind") == "running"
                and ((row.get("progress") or {}).get("downloaded") or 0) > 0):
            return "blocked"
    return "fail"


def site_detail(r):
    """What a reader is told of one sites-check.py result."""
    result = r.get("result") or "no result"
    if result == "OK":
        m = r.get("media") or {}
        said = "a whole video with sound landed in EDM's folder"
        facts = [f"{m['height']} tall" if m.get("height") else "", f"{m['seconds']:.0f} s" if m.get("seconds") else ""]
        facts = [f for f in facts if f]
        return said + (f" ({', '.join(facts)})" if facts else "")
    page = r.get("title") or r.get("body") or ""
    return result + (f"; the page: {page}" if page and site_status(r) == "blocked" else "")


def youtube_row(checks):
    """youtube-check.py's checks of the real page as one row: every `YouTube:`
    check passed, or which one did not."""
    own = [c for c in checks if c["name"].startswith("YouTube:")]
    if not own:
        first = next((c for c in checks if not c["ok"]), None)
        why = f"{first['name']}: {first['detail']}" if first else "it ended before the page"
        return {"site": "youtube", "status": "error", "detail": "the check stopped before YouTube: " + why}
    failed = [c for c in own if not c["ok"]]
    if not failed:
        return {"site": "youtube", "status": "pass",
                "detail": "a whole video landed in EDM's folder, read by EDM's own reader"}
    first = failed[0]
    said = first["name"].removeprefix("YouTube: ") + (f" ({first['detail']})" if first["detail"] else "")
    bot = first["name"] == "YouTube: the watch page loads" or any(BOT_CHECK.search(str(c["detail"] or "")) for c in failed)
    return {"site": "youtube", "status": "blocked" if bot else "fail",
            "detail": ("YouTube's bot check answered: " if bot else "failed: ") + said}


def write_result(path, check, sites, checks, signed_in=False):
    """What one check script found, as `path` (JSON). `sites` are rows of
    {site, status, detail, url}; `checks` are chrome-check.py's (ok, name, detail)."""
    data = {
        "check": check,
        "finished": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "system": platform.system(),
        "signed_in": bool(signed_in),
        "run": run_url(),
        "sites": [{**s, "name": name_of(s["site"])} for s in sites],
        "checks": [{"ok": bool(ok), "name": str(name), "detail": str(detail)} for ok, name, detail in checks],
    }
    pathlib.Path(path).parent.mkdir(parents=True, exist_ok=True)
    pathlib.Path(path).write_text(json.dumps(data, indent=1, default=str), encoding="utf-8")


def run_url():
    if os.environ.get("GITHUB_RUN_ID"):
        return (f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/"
                f"{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/{os.environ['GITHUB_RUN_ID']}")
    return None


def load(paths):
    """Every site row of the run's files. A file that is missing or unreadable
    is a row of its own: the check it was to hold gave nothing."""
    runs, rows = [], []
    for path in paths:
        try:
            data = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
        except (OSError, ValueError) as e:
            rows.append({"site": pathlib.Path(path).stem, "name": f"every site of {pathlib.Path(path).name}", "status": "error",
                         "detail": f"the check wrote no result ({type(e).__name__})"})
            continue
        runs.append(data)
        rows.extend(data.get("sites") or [])
    return runs, rows


def cell(text, most=160):
    text = " ".join(str(text or "").split()).replace("|", "\\|")
    return text if len(text) <= most else text[: most - 1] + "…"


def table(runs, rows):
    """The README's table, the marker comments not included."""
    when = max((r.get("finished") or "" for r in runs), default="")
    run = next((r.get("run") for r in runs if r.get("run")), None)
    system = next((r.get("system") for r in runs if r.get("system")), "")
    signed = any(r.get("signed_in") for r in runs)
    head = (f"The daily site watch's last run, {when[:10] or 'never'}"
            + (f" ([the run]({run}))" if run else "")
            + f": {'signed in' if signed else 'signed out'}, a new browser profile"
            + (f" on {system}" if system else "")
            + ", a video played on each site and taken with the **Download video** panel.")
    lines = [head, "", "| Site | Result | What happened |", "|---|---|---|"]
    for r in rows:
        lines.append(f"| {cell(r.get('name') or name_of(r.get('site')), 40)} | {WORDS.get(r.get('status'), r.get('status'))} "
                     f"| {cell(r.get('detail'))} |")
    lines += ["", "*Not judged*: a login wall or bot check met the check's browser, or the file was still "
              "arriving when the wait ended; neither says anything of EDM. The table is made by "
              "`scripts/site-table.py` from `.github/workflows/sites.yml`."]
    return "\n".join(lines)


def into_readme(text, made):
    """`text` with what is between the markers replaced by `made`."""
    start, end = text.find(START), text.find(END)
    if start < 0 or end < start:
        raise ValueError(f"no {START} … {END} in the README")
    return text[: start + len(START)] + "\n" + made + "\n" + text[end:]


def counts(rows):
    by = {k: [r for r in rows if r.get("status") == k] for k in ("pass", "fail", "blocked", "error")}
    return {"passed": len(by["pass"]), "failed": len(by["fail"]) + len(by["error"]), "blocked": len(by["blocked"]),
            "failing": ", ".join(r.get("name") or r.get("site") for r in by["fail"] + by["error"])}


def tail(path, lines=60, width=300):
    try:
        said = pathlib.Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return "(no log)"
    # A run of backticks would end the issue's code block early.
    return "\n".join(re.sub(r"`{3,}", "``", line[:width]) for line in said[-lines:])


def issue(runs, rows, logs):
    """The issue's text: the sites that failed, then the end of each log."""
    run = next((r.get("run") for r in runs if r.get("run")), None)
    bad = [r for r in rows if r.get("status") in ("fail", "error")]
    parts = [f"The daily site watch found {len(bad)} site{'s' if len(bad) != 1 else ''} not working"
             + (f" ({run})" if run else "") + ":", ""]
    parts += [f"- **{cell(r.get('name') or r.get('site'), 40)}**: {cell(r.get('detail'), 400)}"
              + (f" ({r['url']})" if r.get("url") else "") for r in bad]
    parts += ["", "A site's own change shows as *broken* here before a person finds it. The run's artifact "
              "`site-watch` has the whole logs and the JSON results; `python3 scripts/sites-check.py <site>` "
              "or `python3 scripts/youtube-check.py` runs the same check by hand. This issue is added to "
              "while the watch keeps failing and closed by the first run that passes.", ""]
    for path in logs:
        parts += [f"<details><summary>The end of {pathlib.Path(path).name}</summary>", "", "```",
                  tail(path), "```", "", "</details>", ""]
    return "\n".join(parts)


def self_test():
    sample_checks = [(True, "the extension's service worker starts", ""),
                     (True, "YouTube: the page script loads", ""),
                     (False, "YouTube: its click lists the video's qualities with their sizes",
                      "{'sub': \"Sign in to confirm you're not a bot\"}")]
    yt = youtube_row([{"ok": o, "name": n, "detail": d} for o, n, d in sample_checks])
    assert yt["status"] == "blocked", yt
    assert youtube_row([{"ok": True, "name": "YouTube: x", "detail": ""}])["status"] == "pass"
    assert youtube_row([{"ok": False, "name": "YouTube: the video lands in EDM's folder", "detail": "[]"}])["status"] == "fail"
    assert youtube_row([{"ok": False, "name": "the extension's service worker starts", "detail": ""}])["status"] == "error"
    assert site_status({"result": "OK"}) == "pass"
    assert site_status({"result": "no video player appeared (login wall, bot check or the page needs a click)"}) == "blocked"
    assert site_status({"result": "the panel listed no qualities"}) == "fail"
    assert site_status({"result": "a login wall covers the player (login wall: Log in to TikTok), "
                                  "so EDM's panel stays off it"}) == "blocked"
    assert youtube_row([{"ok": False, "name": "YouTube: its player offers the video",
                         "detail": "YouTube's player says LOGIN_REQUIRED: Sign in to confirm you’re not a bot"}]
                       )["status"] == "blocked"
    assert youtube_row([{"ok": False, "name": "YouTube: its player offers the video",
                         "detail": "YouTube's player says UNPLAYABLE: Video unavailable"}])["status"] == "blocked"
    assert site_status({"result": "check failed: RuntimeError('x')"}) == "error"
    assert site_status({"result": "the download did not finish into EDM's folder",
                        "edm_rows": [{"state": {"kind": "running"}, "progress": {"downloaded": 5}}]}) == "blocked"
    assert "720 tall" in site_detail({"result": "OK", "media": {"height": 720, "seconds": 26.2}})
    rows = [{"site": "vimeo", "name": "Vimeo", "status": "fail", "detail": "a | b\nc"},
            {"site": "youtube", "name": "YouTube", "status": "pass", "detail": "ok"}]
    made = table([{"finished": "2026-11-18T05:00:00Z", "system": "Linux"}], rows)
    assert "| Vimeo | **broken** | a \\| b c |" in made, made
    assert "2026-11-18" in made and "signed out" in made
    readme = f"before\n{START}\nold\n{END}\nafter\n"
    new = into_readme(readme, made)
    assert new.startswith("before\n" + START + "\n") and new.endswith(END + "\nafter\n") and "old" not in new
    assert into_readme(new, made) == new
    c = counts(rows + [{"site": "x", "status": "error"}, {"site": "y", "status": "blocked"}])
    assert c == {"passed": 1, "failed": 2, "blocked": 1, "failing": "Vimeo, x"}, c
    _, missing = load([pathlib.Path(__file__).parent / "no-such-result.json"])
    assert missing[0]["status"] == "error"
    with tempfile.TemporaryDirectory() as d:
        log = pathlib.Path(d) / "x.log"
        log.write_text("one\n```\ntwo\n", encoding="utf-8")
        assert tail(log) == "one\n``\ntwo", tail(log)
    body = issue([{"run": "https://example.invalid/run"}], rows, [])
    assert "**Vimeo**" in body and "**YouTube**" not in body
    print("site-table.py: self-test passed")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("results", nargs="*", help="the JSON files the checks wrote (--json)")
    ap.add_argument("--readme", help="rewrite the table between the markers of this README")
    ap.add_argument("--out", help="write the table here as well")
    ap.add_argument("--issue", help="write the issue's text here")
    ap.add_argument("--log", action="append", default=[], help="a check's log, whose end goes in the issue")
    ap.add_argument("--self-test", action="store_true", help="check this script's own workings, and stop")
    a = ap.parse_args()
    if a.self_test:
        self_test()
        return
    if not a.results:
        ap.error("name the JSON files the checks wrote")
    runs, rows = load(a.results)
    made = table(runs, rows)
    print(made)
    if a.out:
        pathlib.Path(a.out).write_text(made + "\n", encoding="utf-8")
    if a.readme:
        path = pathlib.Path(a.readme)
        path.write_text(into_readme(path.read_text(encoding="utf-8"), made), encoding="utf-8", newline="\n")
    if a.issue:
        pathlib.Path(a.issue).write_text(issue(runs, rows, a.log), encoding="utf-8")
    c = counts(rows)
    print(f"\npassed {c['passed']}, failed {c['failed']}, not judged {c['blocked']}"
          + (f": {c['failing']}" if c["failing"] else ""))
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
            for k, v in c.items():
                out.write(f"{k}={v}\n")


if __name__ == "__main__":
    main()
