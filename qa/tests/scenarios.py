"""The QA scenarios. Each takes no arguments, raises Fail or Skip, or returns
a short note. Names are unique: each scenario uses its own file names, so the
origin's per-name counters are its own."""
import json
import os
import pathlib
import shutil
import struct
import subprocess
import time
import urllib.parse

from harness import (BINARY, DM, DOWNLOADS, MOCKVIDEO, ORIGIN, ORIGIN_HOST, Fail, Skip, check, content, exact,
                     no_leftovers, proxied, reason, seed_of, stats, toxiproxy, url)

SCENARIOS = {}
MB = 1 << 20


def scenario(fn):
    SCENARIOS[fn.__name__] = fn
    return fn


def simple(name, size, settings=None, seconds=60, **opts):
    """One download from start to end; the file checked and nothing left over."""
    dm = DM(name, settings).start()
    try:
        dm.add(url(name, size, **opts))
        row = dm.wait(f"/f/{urllib.parse.quote(name)}?", seconds=seconds)
        exact(row, name, size)
        no_leftovers(dm, row)
        return row
    finally:
        dm.close()


# ------------------------------------------------------------ the basics

@scenario
def single_connection():
    simple("single.bin", 3 * MB, {"connections": 1})
    check(stats("single.bin")["max_open"] == 1, f"1 connection asked, {stats('single.bin')['max_open']} used")


@scenario
def connections_1_8_32():
    used = {}
    for n in (1, 8, 32):
        name = f"conn{n}.bin"
        simple(name, 24 * MB, {"connections": n}, drip=4 * MB)
        used[n] = stats(name)["max_open"]
        check(used[n] <= n, f"{n} connections allowed, {used[n]} opened")
    check(used[8] > 1 and used[32] > used[8] // 2, f"connections opened: {used}")
    return f"opened {used}"


@scenario
def sizes_and_boundaries():
    for size in (0, 1, 65535, 65536, 65537, MB - 1, MB, MB + 1, 4 * MB - 1, 4 * MB + 1):
        simple(f"size{size}.bin", size, {"connections": 8})


@scenario
def no_range_support():
    simple("noranges.bin", 6 * MB, {"connections": 8}, ranges=0)
    simple("arnone.bin", 6 * MB, {"connections": 8}, ar="none")
    check(stats("noranges.bin")["requests"] <= 2, f"no ranges, yet {stats('noranges.bin')['requests']} requests")


@scenario
def length_missing_or_chunked():
    simple("nolen.bin", 3 * MB + 7, {"connections": 8}, nolen=1)
    simple("chunked.bin", 3 * MB + 7, {"connections": 8}, chunked=1)


@scenario
def length_wrong():
    # Says more than it sends, then closes: must never be "completed" short.
    dm = DM("length_wrong", {"connections": 4}).start()
    try:
        dm.add(url("longclaim.bin", 1 * MB, len=2 * MB))
        row = dm.wait("longclaim.bin", seconds=90)
        if row["state"]["kind"] == "completed":
            size = pathlib.Path(row["path"]).stat().st_size
            check(size == row["progress"]["total"], f"completed with {size} bytes but reports {row['progress']['total']}")
            check(size != 2 * MB, "a file padded to the claimed length")
        else:
            check(reason(row), "failed without a reason")
        no_leftovers(dm)
    finally:
        dm.close()


# ------------------------------------------------------------ redirects, names

@scenario
def redirects():
    dm = DM("redirects").start()
    try:
        names = []
        for code in (301, 302, 307, 308):
            name = f"r{code}.bin"
            dm.add(f"{ORIGIN}/redir/{code}/4/f/{name}?size={MB + code}")
            names.append((name, MB + code))
        dm.add(f"{ORIGIN}/rel/3/f/rel.bin?size={MB}")
        names.append(("rel.bin", MB))
        for name, size in names:
            exact(dm.wait(name, seconds=60), name, size)
        dm.add(f"{ORIGIN}/loop/302/loop.bin")
        row = dm.wait("/loop/", seconds=60)
        check(row["state"]["kind"] == "failed", f"a redirect loop ended {row['state']}")
        check(reason(row), "a redirect loop failed with no reason")
        no_leftovers(dm)
        return f"loop said: {reason(row)}"
    finally:
        dm.close()


HOSTILE_NAMES = {
    "unicode": "attachment; filename*=UTF-8''%E6%96%87%E4%BB%B6%20r%C3%A9sum%C3%A9.pdf",
    "spaces": 'attachment; filename="my report  final.pdf"',
    "traversal": 'attachment; filename="../../evil.exe"',
    "traversal_star": "attachment; filename*=UTF-8''..%2F..%2F..%2Fetc%2Fevil.sh",
    "absolute": 'attachment; filename="/etc/passwd"',
    "backslash": 'attachment; filename="..\\\\..\\\\win.ini"',
    "con": 'attachment; filename="CON"',
    "nul_ext": 'attachment; filename="NUL.txt"',
    "long": 'attachment; filename="' + "a" * 400 + '.bin"',
    "dots": 'attachment; filename="..."',
    # Percent-encoded: a raw control byte in a header is not HTTP at all, and
    # the client refusing the response is right (BUGLOG.md, T-001).
    "control": "attachment; filename*=UTF-8''bad%01name%0A.bin",
}


@scenario
def hostile_filenames():
    dm = DM("hostile_filenames").start()
    try:
        out = dm.out.resolve()
        for key, cd in HOSTILE_NAMES.items():
            dm.add(url(f"cd_{key}", 1000, cd=cd))
        seen = {}
        for key in HOSTILE_NAMES:
            row = dm.wait(f"/f/cd_{key}?", seconds=60)
            p = exact(row, f"cd_{key}", 1000).resolve()
            check(out in p.parents, f"{key}: written outside the folder: {p}")
            check(len(p.name.encode()) <= 255, f"{key}: name of {len(p.name.encode())} bytes")
            check(not any(ord(c) < 32 for c in p.name), f"{key}: control character in {p.name!r}")
            check(p.name.upper().split(".")[0] not in ("CON", "NUL", "PRN", "AUX"), f"{key}: reserved Windows name {p.name}")
            check(p.name not in ("", ".", ".."), f"{key}: empty name")
            seen[key] = p.name
        check("文件" in seen["unicode"], f"unicode name lost: {seen['unicode']}")
        check(seen["spaces"].startswith("my report"), f"spaces: {seen['spaces']}")
        no_leftovers(dm)
        return json.dumps(seen, ensure_ascii=False)[:300]
    finally:
        dm.close()


@scenario
def no_overwrite():
    dm = DM("no_overwrite").start()
    try:
        keep = dm.out / "same.bin"
        keep.write_bytes(b"the person's own file")
        dm.add(url("same.bin", 50_000))
        row = dm.wait("same.bin")
        p = exact(row, "same.bin", 50_000)
        check(keep.read_bytes() == b"the person's own file", "an existing file was overwritten")
        check(p != keep, "downloaded over the existing file")
        return f"saved as {p.name}"
    finally:
        dm.close()


# ------------------------------------------------------------ resume

def mid(dm, needle, at, seconds=60):
    """Waits until `needle` has at least `at` bytes."""
    deadline = time.time() + seconds
    while time.time() < deadline:
        r = dm.row(needle)
        if r and r["progress"]["downloaded"] >= at:
            return r
        if r and r["state"]["kind"] in ("completed", "failed"):
            raise Fail(f"{needle} ended before {at} bytes: {r['state']}")
        time.sleep(0.1)
    r = dm.row(needle)
    raise Fail(f"{needle} never reached {at} bytes: {r and r['state']} {r and r['progress']} {r and r.get('stalled')}")


@scenario
def pause_resume():
    dm = DM("pause_resume", {"connections": 4}).start()
    try:
        dm.add(url("pause.bin", 8 * MB, drip=MB))
        row = mid(dm, "pause.bin", 2 * MB)
        check(dm.act("downloads.pause", row)["ok"], "pause refused")
        row = dm.wait("pause.bin", kinds=("paused",), seconds=10)
        held = row["progress"]["downloaded"]
        time.sleep(1.5)
        check(dm.row("pause.bin")["progress"]["downloaded"] == held, "bytes still arriving while paused")
        check(dm.act("downloads.resume", row)["ok"], "resume refused")
        exact(dm.wait("pause.bin", seconds=90), "pause.bin", 8 * MB)
        no_leftovers(dm, dm.row("pause.bin"))
    finally:
        dm.close()


def restart_resume(kill):
    tag = "kill" if kill else "restart"
    name = f"{tag}.bin"
    dm = DM(f"{tag}_resume", {"connections": 4}).start()
    try:
        # Slow enough that the stop comes seconds in: dm writes down how far
        # each range got once a second, and a stop before the first record
        # rightly starts again (BUGLOG.md, T-003).
        dm.add(url(name, 8 * MB, drip=256 * 1024, etag='"v1"'))
        mid(dm, name, 3 * MB, seconds=60)
        dm.stop(kill=kill)
        before = stats(name)["bytes"]
        dm.start()
        row = dm.row(name)
        check(row is not None, "the download was forgotten after the restart")
        if row["state"]["kind"] in ("paused", "failed"):
            dm.act("downloads.resume", row)
        exact(dm.wait(name, seconds=120), name, 8 * MB)
        sent = stats(name)["bytes"] - before
        check(sent < 8 * MB, f"resumed from scratch: {sent} more bytes sent for 8 MB")
        no_leftovers(dm, dm.row(name))
        return f"{sent // 1024} KB fetched after the {tag}"
    finally:
        dm.close()


@scenario
def resume_after_restart():
    return restart_resume(kill=False)


@scenario
def resume_after_kill9():
    return restart_resume(kill=True)


@scenario
def resume_without_ranges():
    dm = DM("resume_without_ranges", {"connections": 4}).start()
    try:
        dm.add(url("norange_resume.bin", 6 * MB, ranges=0, drip=2 * MB))
        row = mid(dm, "norange_resume.bin", 2 * MB)
        dm.act("downloads.pause", row)
        dm.wait("norange_resume.bin", kinds=("paused",), seconds=10)
        dm.act("downloads.resume", row)
        exact(dm.wait("norange_resume.bin", seconds=90), "norange_resume.bin", 6 * MB)
        no_leftovers(dm, dm.row("norange_resume.bin"))
    finally:
        dm.close()


@scenario
def file_replaced_mid_download():
    """The server's file changes (new ETag, new bytes) while dm fetches it:
    the result is one version whole, or a clear failure; never the two stitched."""
    notes = []
    for variant, extra in (("same size", {}), ("new size", {"newsize": 5 * MB})):
        name = f"changing_{variant[:3]}.bin"
        dm = DM(f"changing_{variant[:3]}", {"connections": 8}).start()
        try:
            # Slow, so ranges asked for later (splits, retries) meet the new file.
            dm.add(f"{ORIGIN}/changing/{name}?" + urllib.parse.urlencode({"size": 6 * MB, "flip": 2 * MB, "drip": 256 * 1024, **extra}))
            row = dm.wait(name, seconds=180)
            if row["state"]["kind"] == "completed":
                p = pathlib.Path(row["path"])
                got = content.file_sha256(p)
                size = p.stat().st_size
                v1 = content.sha256(seed_of(name), 6 * MB)
                v2 = content.sha256(seed_of(name) + 1, int(extra.get("newsize", 6 * MB)))
                check(got in (v1, v2), f"{variant}: the file is neither version ({size} bytes): two versions stitched")
                notes.append(f"{variant}: {'v2' if got == v2 else 'v1'} whole, {len(stats(name)['ranges'])} requests")
            else:
                check(reason(row), f"{variant}: failed with no reason")
                notes.append(f"{variant}: failed: {reason(row)}")
        finally:
            dm.close()
    return "; ".join(notes)


@scenario
def range_past_end_416():
    """The file shrinks while paused: the resume's range is past its end (416)."""
    name = "shrinks.bin"
    dm = DM("range_past_end_416", {"connections": 1}).start()
    try:
        dm.add(f"{ORIGIN}/changing/{name}?" + urllib.parse.urlencode({"size": 6 * MB, "flip": 3 * MB, "newsize": MB, "drip": MB}))
        row = mid(dm, name, 3 * MB + 1, seconds=60)
        dm.act("downloads.pause", row)
        dm.wait(name, kinds=("paused",), seconds=10)
        dm.act("downloads.resume", row)
        row = dm.wait(name, seconds=90)
        if row["state"]["kind"] == "completed":
            p = pathlib.Path(row["path"])
            check(content.file_sha256(p) == content.sha256(seed_of(name) + 1, MB),
                  f"completed with {p.stat().st_size} bytes that are not the new file")
            return "restarted and fetched the new file"
        check(reason(row), "failed with no reason")
        return f"failed: {reason(row)}"
    finally:
        dm.close()


# ------------------------------------------------------------ server errors

@scenario
def http_errors():
    # Three tries: the default ten, half a minute apart at the end, take over
    # two minutes to fail a lasting 500, by design (BUGLOG.md, T-002).
    dm = DM("http_errors", {"attempts": 3}).start()
    try:
        for code in (404, 403, 500):
            dm.add(url(f"err{code}.bin", 1000, status=code))
        said = {}
        for code in (404, 403, 500):
            row = dm.wait(f"err{code}.bin", seconds=120)
            check(row["state"]["kind"] == "failed", f"{code}: {row['state']}")
            check(reason(row), f"{code}: failed with no reason")
            said[code] = reason(row)
        no_leftovers(dm)
        return json.dumps(said)[:300]
    finally:
        dm.close()


@scenario
def retry_503_and_429():
    dm = DM("retry_503_and_429").start()
    try:
        t0 = time.time()
        dm.add(url("busy503.bin", MB, e503=2), url("slow429.bin", MB, e429=2, ra=2))
        exact(dm.wait("busy503.bin", seconds=120), "busy503.bin", MB)
        exact(dm.wait("slow429.bin", seconds=120), "slow429.bin", MB)
        took = time.time() - t0
        check(stats("slow429.bin")["requests"] >= 3, "429 was not retried")
        return f"{took:.1f}s"
    finally:
        dm.close()


@scenario
def stalls_and_cuts():
    simple("cut.bin", 6 * MB, {"connections": 4}, cut=300_000, cutn=6)
    simple("stall.bin", 3 * MB, {"connections": 2}, seconds=180, stall=500_000, stalln=2)
    simple("drip1k.bin", 6_000, {"connections": 1}, seconds=60, drip=1024)


# ------------------------------------------------------------ auth, cookies, TLS

@scenario
def basic_auth():
    dm = DM("basic_auth").start()
    try:
        host = urllib.parse.urlsplit(ORIGIN)
        good = f"{host.scheme}://qa:secret@{host.netloc}/basic/auth_ok.bin?size={MB}"
        dm.add(good, f"{ORIGIN}/basic/auth_none.bin?size={MB}")
        exact(dm.wait("auth_ok.bin"), "auth_ok.bin", MB)
        row = dm.wait("auth_none.bin", seconds=120)
        check(row["state"]["kind"] == "failed", f"no login: {row['state']}")
        check(reason(row), "no login: failed with no reason")
        return reason(row)
    finally:
        dm.close()


def native_host(dm, *messages):
    """Speaks to dm as the browser does: dm-desktop started as the native host."""
    frames = b"".join(struct.pack("<I", len(m)) + m for m in (json.dumps(x).encode() for x in messages))
    run = subprocess.run([str(BINARY), "chrome-extension://fojpndfjodldkfbamkofdmnfedgjjejo/"],
                         input=frames, capture_output=True, timeout=60, env={**os.environ, "DM_HOME": str(dm.home)})
    out, answers = run.stdout, []
    while len(out) >= 4:
        n = struct.unpack("<I", out[:4])[0]
        answers.append(json.loads(out[4:4 + n]))
        out = out[4 + n:]
    return answers


@scenario
def cookie_from_browser():
    dm = DM("cookie_from_browser").start()
    try:
        hello = {"protocolVersion": 1, "type": "engine.hello"}
        u = f"{ORIGIN}/cookie/cookie_ok.bin?size={MB}"
        said = native_host(dm, hello, {"protocolVersion": 1, "type": "download.start", "id": "c1", "url": u,
                                       "referrer": ORIGIN + "/", "cookies": "session=qa; other=1"})
        check(any(a.get("type") == "download.started" for a in said), f"the hand-off was refused: {said}")
        exact(dm.wait("cookie_ok.bin"), "cookie_ok.bin", MB)
        # Header names in any case: hyper sends them in lower case (BUGLOG.md, T-004).
        sent = [next((v for k, v in h.items() if k.lower() == "cookie"), "") for h in stats("cookie_ok.bin")["headers"]]
        check(all("session=qa" in c for c in sent), f"a request went without the cookie: {sent}")
    finally:
        dm.close()


@scenario
def bad_certificates():
    if not os.environ.get("ORIGIN"):
        raise Skip("the HTTPS ports are the origin container's; run in the compose stack")
    dm = DM("bad_certificates").start()
    try:
        cases = {8443: "private CA", 8444: "expired", 8445: "wrong host", 8446: "self-signed"}
        for port in cases:
            dm.add(f"https://{ORIGIN_HOST}:{port}/f/tls{port}.bin?size=1000")
        said = {}
        for port, what in cases.items():
            row = dm.wait(f"tls{port}.bin", seconds=120)
            check(row["state"]["kind"] == "failed", f"{what}: accepted ({row['state']})")
            check(reason(row), f"{what}: failed with no reason")
            said[what] = reason(row)
            check(stats(f"tls{port}.bin").get("requests", 0) == 0, f"{what}: a request got through")
        no_leftovers(dm)
        return json.dumps(said)[:400]
    finally:
        dm.close()


# ------------------------------------------------------------ the queue

@scenario
def queue_limit_20():
    dm = DM("queue_limit_20", {"concurrency": 3, "connections": 2}).start()
    try:
        names = [f"q{i:02}.bin" for i in range(20)]
        dm.add(*[url(n, 600_000, drip=600_000) for n in names])
        most = 0
        deadline = time.time() + 180
        while time.time() < deadline:
            rows = [r for r in dm.rows() if "/f/q" in r["url"]]
            running = sum(1 for r in rows if r["state"]["kind"] == "running")
            most = max(most, running)
            if len(rows) == 20 and all(r["state"]["kind"] in ("completed", "failed") for r in rows):
                break
            time.sleep(0.2)
        check(most <= 3, f"{most} running with a limit of 3")
        for n in names:
            exact(dm.row(f"/f/{n}?"), n, 600_000)
        return f"at most {most} at once"
    finally:
        dm.close()


@scenario
def duplicate_url():
    dm = DM("duplicate_url").start()
    try:
        u = url("dup.bin", MB)
        first = dm.add(u)
        exact(dm.wait("dup.bin"), "dup.bin", MB)
        second = dm.add(u)
        rows = [r for r in dm.rows() if "dup.bin" in r["url"]]
        check(len(rows) == 1, f"the same link listed {len(rows)} times ({first}, {second})")
        files = [p for p in dm.out.iterdir() if p.name.startswith("dup")]
        check(len(files) == 1, f"the same link saved {len(files)} times: {files}")
        return json.dumps(second)
    finally:
        dm.close()


@scenario
def cancel_each_phase():
    dm = DM("cancel_each_phase", {"connections": 4}).start()
    try:
        dm.add(url("cancel_connecting.bin", MB, delay=8))
        time.sleep(1)
        row = dm.row("cancel_connecting.bin")
        check(dm.act("downloads.cancel", row)["ok"], "cancel refused while connecting")
        dm.wait("cancel_connecting.bin", kinds=("cancelled",), seconds=15)
        dm.add(url("cancel_moving.bin", 8 * MB, drip=MB))
        row = mid(dm, "cancel_moving.bin", MB)
        check(dm.act("downloads.cancel", row)["ok"], "cancel refused while downloading")
        dm.wait("cancel_moving.bin", kinds=("cancelled",), seconds=15)
        time.sleep(1)
        for n in ("cancel_connecting.bin", "cancel_moving.bin"):
            no_leftovers(dm, dm.row(n))
            check(not (dm.out / n).exists(), f"{n}: a file left after cancel")
    finally:
        dm.close()


@scenario
def history_after_restart():
    dm = DM("history_after_restart").start()
    try:
        dm.add(url("hist.bin", 100_000), url("hist404.bin", 10, status=404))
        dm.wait("hist.bin")
        dm.wait("hist404.bin", seconds=120)
        before = {r["url"]: r["state"]["kind"] for r in dm.rows()}
        dm.stop()
        dm.start()
        after = {r["url"]: r["state"]["kind"] for r in dm.rows()}
        check(before == after, f"history changed across a restart: {before} -> {after}")
    finally:
        dm.close()


@scenario
def resources_over_100_downloads():
    dm = DM("resources_over_100_downloads", {"concurrency": 4, "connections": 4}).start()
    try:
        def wave(i):
            names = [f"leak{i}_{j}.bin" for j in range(20)]
            dm.add(*[url(n, 200_000) for n in names])
            for n in names:
                exact(dm.wait(f"/f/{n}?", seconds=120), n, 200_000)
        wave(0)
        time.sleep(2)
        base = dm.pid_stats()
        for i in range(1, 5):
            wave(i)
        time.sleep(3)
        end = dm.pid_stats()
        check(end[1] <= base[1] + 8, f"open files grew {base[1]} -> {end[1]} over 80 downloads")
        if base[2]:
            check(end[2] <= base[2] + 8, f"threads grew {base[2]} -> {end[2]}")
        check(end[0] <= base[0] * 1.5 + 20_000, f"memory grew {base[0]} -> {end[0]} KB")
        return f"rss {base[0]}->{end[0]} KB, fds {base[1]}->{end[1]}, threads {base[2]}->{end[2]}"
    finally:
        dm.close()


# ------------------------------------------------------------ the disk

def small_disk():
    total = shutil.disk_usage(DOWNLOADS).total
    if total > 1 << 30:
        raise Skip(f"the download folder is not a small disk ({total >> 20} MB); run in the compose stack")


def held_for_room(dm, needle):
    """dm pauses a download that will not fit and says why in the row's
    `stalled` (the window offers another folder, or Resume once there is
    room): a pause with that reason, not a failure (BUGLOG.md, T-005)."""
    row = dm.wait(needle, kinds=("paused", "failed", "completed"), seconds=120)
    check(row["state"]["kind"] != "completed", f"{needle}: bigger than the disk, yet completed")
    said = row.get("stalled") or reason(row)
    check(any(w in said.lower() for w in ("room", "space", "disk", "full")), f"{needle}: unclear: {row['state']} {said!r}")
    return row, said


@scenario
def disk_full():
    small_disk()
    dm = DM("disk_full", {"connections": 4}).start()
    try:
        free = shutil.disk_usage(DOWNLOADS).free
        dm.add(url("toobig.bin", free + 50 * MB))
        row, said = held_for_room(dm, "toobig.bin")
        check(not any(dm.out.iterdir()), f"files left for a download that did not fit: {list(dm.out.iterdir())}")
        dm.act("downloads.cancel", row)
        # The disk is usable again afterwards and a small download still works.
        dm.add(url("after_full.bin", MB))
        exact(dm.wait("after_full.bin", seconds=60), "after_full.bin", MB)
        return said
    finally:
        dm.close()
        shutil.rmtree(DOWNLOADS / "disk_full", ignore_errors=True)


@scenario
def disk_fills_mid_download():
    """No length stated, so nothing to refuse up front: the disk fills while
    the bytes arrive. Stopped with a reason, the disk not left full."""
    small_disk()
    dm = DM("disk_fills", {"connections": 1}).start()
    try:
        dm.add(url("fills.bin", 400 * MB, nolen=1, ranges=0))
        row = dm.wait("fills.bin", kinds=("paused", "failed", "completed"), seconds=180)
        check(row["state"]["kind"] != "completed", "400 MB on a 200 MB disk, yet completed")
        said = row.get("stalled") or reason(row)
        check(said, f"stopped with no reason: {row['state']}")
        dm.act("downloads.cancel", row)
        time.sleep(1)
        check(shutil.disk_usage(DOWNLOADS).free > 150 * MB, f"the disk left full: {shutil.disk_usage(DOWNLOADS).free >> 20} MB free")
        return said
    finally:
        dm.close()
        shutil.rmtree(DOWNLOADS / "disk_fills", ignore_errors=True)


@scenario
def five_gigabytes_on_small_disk():
    small_disk()
    dm = DM("five_gb", {"connections": 8}).start()
    try:
        dm.add(url("five.bin", 5 << 30))
        row, said = held_for_room(dm, "five.bin")
        check("5" in said, f"the 5 GB size is not said: {said}")
        dm.act("downloads.cancel", row)
        return said
    finally:
        dm.close()
        shutil.rmtree(DOWNLOADS / "five_gb", ignore_errors=True)


@scenario
def folder_removed_or_read_only():
    notes = []
    for how in ("removed", "read_only"):
        dm = DM(f"folder_{how}", {"connections": 2}).start()
        try:
            name = f"folder_{how}.bin"
            dm.add(url(name, 6 * MB, drip=MB))
            mid(dm, name, MB)
            if how == "removed":
                shutil.rmtree(dm.out)
            else:
                dm.out.chmod(0o555)
            row = dm.wait(name, kinds=("failed", "completed"), seconds=120)
            if row["state"]["kind"] == "completed":
                exact(row, name, 6 * MB)
                notes.append(f"{how}: completed in {pathlib.Path(row['path']).parent}")
            else:
                check(reason(row), f"{how}: failed with no reason")
                notes.append(f"{how}: {reason(row)}")
        finally:
            dm.out.mkdir(parents=True, exist_ok=True)
            dm.out.chmod(0o755)
            dm.close()
    return "; ".join(notes)


# ------------------------------------------------------------ the network (toxiproxy)

def through_proxy(name, size, settings, during, seconds=180, before=None):
    base = proxied()
    if before:
        before()
    dm = DM(name, settings).start()
    try:
        dm.add(f"{base}/f/{name}?size={size}&drip={2 * MB}")
        mid(dm, name, MB, seconds=seconds)
        during()
        row = dm.wait(name, kinds=("completed", "failed"), seconds=seconds)
        if row["state"]["kind"] == "failed":
            dm.act("downloads.resume", row)
            row = dm.wait(name, seconds=seconds)
        exact(row, name, size)
        no_leftovers(dm, row)
    finally:
        dm.close()
        toxiproxy("DELETE", "/proxies/origin")


def outage(seconds):
    def go():
        toxiproxy("POST", "/proxies/origin", {"enabled": False})
        time.sleep(seconds)
        toxiproxy("POST", "/proxies/origin", {"enabled": True})
    return go


@scenario
def network_loss_5s():
    through_proxy("loss5.bin", 8 * MB, {"connections": 4}, outage(5))


@scenario
def network_loss_60s():
    through_proxy("loss60.bin", 8 * MB, {"connections": 4}, outage(60), seconds=300)


@scenario
def bad_link():
    def toxics():
        for t in ({"name": "lat", "type": "latency", "attributes": {"latency": 300, "jitter": 200}},
                  {"name": "bw", "type": "bandwidth", "attributes": {"rate": 200}},
                  {"name": "reset", "type": "reset_peer", "toxicity": 0.05, "attributes": {"timeout": 2000}}):
            toxiproxy("POST", "/proxies/origin/toxics", t)
    # From the first byte: latency with jitter, 200 KB/s on each connection, and
    # one connection in twenty reset.
    through_proxy("badlink.bin", 6 * MB, {"connections": 8}, lambda: None, seconds=300, before=toxics)


# ------------------------------------------------------------ video pages (yt-dlp)

def mockvideo():
    if not MOCKVIDEO:
        raise Skip("no mock video site")
    return MOCKVIDEO


@scenario
def video_page():
    site = mockvideo()
    dm = DM("video_page").start()
    try:
        dm.add(f"{site}/watch?v=ok")
        row = dm.wait("watch?v=ok", seconds=180)
        check(row["state"]["kind"] == "completed", f"{row['state']}")
        p = pathlib.Path(row["path"])
        check(p.exists() and p.stat().st_size > 10_000, f"{p} missing or tiny")
        files = sorted(x.name for x in dm.out.iterdir())
        check(files == [p.name], f"one video asked for, the folder holds {files}")
        # The page's only video, named by the page: no "(1)" with nothing to
        # tell it from (docs/gaps.md row 31, now closed).
        check(" (1)" not in p.name and "-1]" not in p.name, f"numbered with nothing beside it: {p.name}")
        no_leftovers(dm, row)
        return p.name
    finally:
        dm.close()


@scenario
def video_page_without_media():
    site = mockvideo()
    dm = DM("video_page_without_media").start()
    try:
        dm.add(f"{site}/nomedia.html")
        row = dm.wait("nomedia.html", seconds=180)
        if row["state"]["kind"] == "completed":
            # Saved as the page itself is an acceptable answer for a plain page.
            check(pathlib.Path(row["path"]).stat().st_size < 10_000, "a page with no media saved as something large")
            return "saved the page"
        check(reason(row), "failed with no reason")
        return reason(row)
    finally:
        dm.close()


def fake_ytdlp(dm, body):
    script = dm.root / "yt-dlp"
    script.write_text("#!/bin/sh\n" + body)
    script.chmod(0o755)
    return script


@scenario
def ytdlp_crash_and_hang():
    site = mockvideo()
    notes = []
    for kind, body in (("crash", 'echo "Traceback (most recent call last):" >&2; echo "boom" >&2; exit 1\n'),
                       ("segfault", "kill -SEGV $$\n"),
                       ("hang", "exec sleep 100000\n")):
        dm = DM(f"ytdlp_{kind}")
        old = os.environ.get("DM_YTDLP")
        os.environ["DM_YTDLP"] = str(fake_ytdlp(dm, body))
        # A short silence limit for the hang (BUGLOG.md, BUG-004); its page
        # reading still has its own two minutes first.
        os.environ["DM_YTDLP_QUIET"] = "20"
        try:
            dm.start()
            # A watch address with no extension: yt-dlp's alone (BUGLOG.md, T-007).
            dm.add(f"{site}/watch?v={kind}")
            row = dm.wait(f"watch?v={kind}", kinds=("failed", "completed"), seconds=300)
            check(row["state"]["kind"] == "failed", f"{kind}: {row['state']} {row.get('path')}")
            check(reason(row), f"{kind}: failed with no reason")
            if kind == "hang":
                check("stopped answering" in reason(row), f"hang: {reason(row)}")
            notes.append(f"{kind}: {reason(row)}")
        finally:
            os.environ.pop("DM_YTDLP_QUIET", None)
            if old is None:
                os.environ.pop("DM_YTDLP", None)
            else:
                os.environ["DM_YTDLP"] = old
            dm.close()
    return "; ".join(notes)


@scenario
def hls_streams():
    site = mockvideo()
    dm = DM("hls_streams").start()
    try:
        dm.add(f"{site}/hls/index.m3u8", f"{site}/hls-missing/index.m3u8")
        good = dm.wait("/hls/index.m3u8", seconds=180)
        check(good["state"]["kind"] == "completed", f"HLS: {good['state']}")
        check(pathlib.Path(good["path"]).stat().st_size > 10_000, "HLS file tiny")
        bad = dm.wait("/hls-missing/index.m3u8", seconds=180)
        check(bad["state"]["kind"] == "failed", f"HLS with a segment missing: {bad['state']}")
        check(reason(bad), "missing segment: failed with no reason")
        return reason(bad)
    finally:
        dm.close()
