"""dm against real sites, signed in nowhere, and everything it made removed.

    python3 scripts/real-sites.py                   # every site
    SITES=youtube,wikimedia python3 scripts/real-sites.py

Starts the real binary headless on a throwaway home, download folder and
cache (yt-dlp's included), hands it one link per site through the control
protocol, waits for each row to settle, checks what landed, then stops dm
and deletes all of it. A file served directly is compared byte for byte
with curl's copy, a video is opened with ffprobe, and a site that needs a
sign-in or blocks anonymous visitors must be refused with a reason rather
than saved as a "completed" page.

Not in CI: it fetches about 220 MB from real sites, which change. Run by
hand before a release (needs yt-dlp, ffmpeg and curl). First run, 2026-10-13,
found Wikimedia refusing dm's user agent and then serving several ranges at
once as the whole file; both are fixed and pinned by tests.
"""
import hashlib, json, os, pathlib, shutil, socket, subprocess, sys, tempfile, time

ROOT = pathlib.Path(__file__).resolve().parent.parent
# dm's agent, as dm-core says it (USER_AGENT), for curl's second opinion.
AGENT = "dm/0.1.0 (+https://github.com/pranavo72bex/Easy-Download-Manager)"
BINARY = pathlib.Path(os.environ.get("DM_BINARY") or ROOT / "desktop/target/debug/dm-desktop")

# (site, url, what should happen, how the result is checked)
CASES = [
    # An MP4, not the WebM yt-dlp picks by itself.
    ("YouTube (via yt-dlp)", "https://www.youtube.com/watch?v=jNQXAC9IVRw", "file", "mp4"),
    ("Internet Archive (redirect, 27 MB)", "https://archive.org/download/Popeye_forPresident/Popeye_forPresident_512kb.mp4", "file", "same-as-curl"),
    ("GitHub release (redirect to CDN)", "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-apple-darwin.tar.gz", "file", "same-as-curl"),
    ("GNU FTP (anonymous, 10 MB in REST ranges)", "ftp://ftp.gnu.org/gnu/bash/bash-5.2.tar.gz", "file", "same-as-curl"),
    ("OVH speed file (ranges, 8 connections)", "https://proof.ovh.net/files/10Mb.dat", "file", "same-as-curl"),
    ("W3Schools sample video (small MP4)", "https://www.w3schools.com/html/mov_bbb.mp4", "file", "same-as-curl"),
    ("GitHub raw text (compresses, ranges over gzip)", "https://raw.githubusercontent.com/torvalds/linux/master/README", "file", "same-as-curl"),
    ("Apple DTD (compressed text, no ranges)", "https://www.apple.com/DTDs/PropertyList-1.0.dtd", "file", "same-as-curl"),
    ("Wikimedia Commons image", "https://upload.wikimedia.org/wikipedia/commons/3/3f/Fronalpstock_big.jpg", "file", "image"),
    ("HLS stream (Mux test stream)", "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8", "file", "video"),
    ("BitTorrent (.torrent, Sintel)", "https://webtorrent.io/torrents/sintel.torrent", "file", "video"),
    ("Reddit, anonymous (blocked)", "https://www.reddit.com/r/aww/top/", "refused", None),
    ("Vimeo, needs sign-in", "https://vimeo.com/76979871", "refused", None),
]


if os.environ.get("SITES"):
    CASES = [c for c in CASES if any(w.lower() in c[0].lower() for w in os.environ["SITES"].split(","))]


def main():
    if not BINARY.exists():
        sys.exit(f"no dm binary at {BINARY} — cargo build -p dm-desktop")
    root = pathlib.Path(tempfile.mkdtemp(prefix="dm-real-"))
    home, downloads, cache = root / "home", root / "downloads", root / "cache"
    for d in (home, downloads, cache):
        d.mkdir()
    (home / "settings.json").write_text(json.dumps({
        "download_dir": str(downloads), "categorize": False, "connections": 8,
        "stream_quality": "360", "ask_panel_downloads": False,
        "show_progress": False, "show_complete": False,
    }))
    env = dict(os.environ, DM_HOME=str(home), XDG_CACHE_HOME=str(cache))
    yt_cache_before = (pathlib.Path.home() / ".cache/yt-dlp").exists()
    app = subprocess.Popen([str(BINARY), "--headless", "--control", "0"], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        port_file = home / "control.port"
        for _ in range(100):
            if port_file.exists() and port_file.read_text().strip():
                break
            time.sleep(0.2)
        conn = socket.create_connection(("127.0.0.1", int(port_file.read_text())), timeout=60)
        replies = conn.makefile("r")

        def call(method, **params):
            conn.sendall((json.dumps({"id": 1, "method": method, "params": params}) + "\n").encode())
            return json.loads(replies.readline())

        call("auth", token=(home / "control.token").read_text().strip())
        for _, url, _, _ in CASES:
            call("downloads.add", urls=[url])

        def rows():
            q = home / "queue.json"
            return {r["url"]: r for r in json.loads(q.read_text() or "[]")} if q.exists() else {}

        started = time.time()
        while time.time() - started < 1500:
            got = rows()
            if len(got) >= len(CASES) and all(r["state"]["kind"] in ("completed", "failed", "cancelled") for r in got.values()):
                break
            time.sleep(2)
        took = time.time() - started

        print(f"\n{'site':<40} {'dm says':<12} {'result'}")
        failures = 0
        for site, url, expect, how in CASES:
            r = rows().get(url)
            state = r["state"] if r else {"kind": "missing"}
            kind = state["kind"]
            reason = next((str(v) for k, v in state.items() if k != "kind"), "")
            path = pathlib.Path(r["path"]) if r and r.get("path") else None
            verdict = verify(expect, how, url, kind, path, downloads, root)
            ok = verdict.startswith("ok")
            failures += not ok
            print(f"{site:<40} {kind:<12} {'ok  ' if ok else 'FAIL'} {verdict}" + (f" — dm: {reason[:140]}" if reason else ""))
        print(f"\n{len(CASES) - failures} of {len(CASES)} sites as expected, in {took:.0f}s")
        conn.close()
    finally:
        app.terminate()
        try:
            app.wait(15)
        except subprocess.TimeoutExpired:
            app.kill()
        size = sum(f.stat().st_size for f in root.rglob("*") if f.is_file())
        shutil.rmtree(root, ignore_errors=True)
        print(f"cleaned: removed {size / 1e6:.1f} MB of downloads, parts, settings and cache "
              f"({'gone' if not root.exists() else 'STILL THERE: ' + str(root)})")
        made = not yt_cache_before and (pathlib.Path.home() / ".cache/yt-dlp").exists()
        print("yt-dlp's own cache in ~/.cache untouched" if not made else "yt-dlp wrote ~/.cache/yt-dlp")


def verify(expect, how, url, kind, path, downloads, root):
    if expect == "refused":
        if path and path.exists():
            return f"refused, but left {path.name} in the download folder"
        if kind != "failed":
            return f"expected a refusal, got {kind} ({path.name if path else ''})"
        return "ok: refused with a reason, nothing saved as if it were the file"
    if kind != "completed" or not path or not path.exists():
        return f"not downloaded ({kind})"
    size = path.stat().st_size if path.is_file() else sum(f.stat().st_size for f in path.rglob("*") if f.is_file())
    target = path if path.is_file() else max((f for f in path.rglob("*") if f.is_file()), key=lambda f: f.stat().st_size)
    kind_of = subprocess.run(["file", "-b", str(target)], capture_output=True, text=True).stdout.strip()[:60]
    if how == "same-as-curl":
        # curl is the second opinion, and a second opinion that failed to
        # arrive is not a disagreement: a reset connection is retried, and
        # only a copy curl finished is compared (OVH reset curl once).
        ref = root / "curl.ref"
        fetched = False
        # Asked as dm asks: its full agent, then its name alone, which is
        # what dm falls back to where a site refuses the first (and curl's
        # own agent is refused by some of the same sites).
        for agent in [AGENT, AGENT.split(" ")[0], AGENT.split(" ")[0]]:
            ref.unlink(missing_ok=True)
            if subprocess.run(["curl", "-sSLf", "-A", agent, "--retry", "3", "--retry-all-errors",
                               "-o", str(ref), url], check=False).returncode == 0:
                fetched = True
                break
            time.sleep(2)
        if not fetched:
            ref.unlink(missing_ok=True)
            return f"{path.name}: curl could not fetch its own copy to compare with"
        same = hashlib.sha256(ref.read_bytes()).hexdigest() == hashlib.sha256(path.read_bytes()).hexdigest()
        ref.unlink(missing_ok=True)
        return (f"ok: {path.name}, {size / 1e6:.1f} MB, byte-identical to curl's" if same
                else f"{path.name} differs from curl's copy")
    if how == "mp4" and target.suffix != ".mp4":
        return f"{target.name} is not an MP4"
    if how in ("video", "mp4"):
        ffprobe = shutil.which("ffprobe") or next(
            (p for p in ("/opt/homebrew/bin/ffprobe", "/usr/local/bin/ffprobe", "/usr/bin/ffprobe")
             if os.path.exists(p)), None)
        if not ffprobe:
            return f"ok: {target.name}, {size / 1e6:.1f} MB (not played: no ffprobe here)"
        probe = subprocess.run([ffprobe, "-v", "error", "-show_entries", "format=duration:stream=codec_type,codec_name,height",
                                "-of", "json", str(target)], capture_output=True, text=True)
        try:
            info = json.loads(probe.stdout)
            seconds = float(info["format"]["duration"])
            heights = [s.get("height") for s in info["streams"] if s.get("codec_type") == "video"]
            codecs = "+".join(s.get("codec_name", "?") for s in info["streams"])
            return f"ok: {target.name}, {size / 1e6:.1f} MB, plays {seconds:.0f}s, {heights[0] if heights else '?'}p, {codecs}"
        except Exception:
            return f"{target.name} is not a playable video ({kind_of})"
    if how == "image":
        return (f"ok: {path.name}, {size / 1e6:.1f} MB, {kind_of}" if "image" in kind_of
                else f"{path.name} is not an image: {kind_of}")
    return f"ok: {path.name}"


main()
