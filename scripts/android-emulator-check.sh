#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# dm's APK on a booted Android emulator (gaps row N13; .github/workflows/android.yml's
# emulator job, docs/android.md):
#
#   bash scripts/android-emulator-check.sh app-debug.apk out/
#
# Installs the APK (its permissions granted), starts dm's activity and checks that its process
# lives, that the activity is the one resumed and that its window drew: the screen holds dm's
# blue (0x1e88e5, the square of src/app.rs; N17's window changes what is looked for). Then dm is
# stopped and started again by a share, as another app's share sheet does, with the address of
# a file served on the computer (10.0.2.2 from inside the emulator), and the file must arrive
# whole in one of the places dm downloads to. What it saw stays in `out/`: the screen
# (screen.png), logcat, the activity's and the window's dumps, the server's log.
#
# Needs adb on PATH with one emulator connected, and python3. The APK must be a debug one:
# `run-as` reads dm's private folder.

set -euo pipefail

apk=${1:?the APK}
out=${2:?the folder for what was seen}
pkg=com.dm.android
activity=dev.gpui.mobile.GpuiInputActivity
probe=dm-ci-probe.bin
port=8765
mkdir -p "$out"

fail() {
    echo "::error::$*"
    exit 1
}

# Whatever happens, what the emulator saw is kept.
server=
finish() {
    adb exec-out screencap -p > "$out/screen-last.png" 2>/dev/null || true
    adb logcat -d > "$out/logcat.txt" 2>/dev/null || true
    adb logcat -d -b crash > "$out/logcat-crash.txt" 2>/dev/null || true
    if [ -n "$server" ]; then kill "$server" 2>/dev/null || true; fi
}
trap finish EXIT

# The emulator's own shell answers with CR LF.
sh_adb() {
    adb shell "$@" | tr -d '\r'
}

adb logcat -c
adb install -r -g "$apk"

# 1. dm started as the launcher starts it.
adb shell am start -W -n "$pkg/$activity"
sleep 20
pid=$(sh_adb pidof "$pkg" || true)
[ -n "$pid" ] || fail "dm's process is not running 20 s after start (see logcat.txt)"
echo "dm runs as process $pid"

sh_adb dumpsys activity activities > "$out/activity.txt"
grep -E 'Resumed' "$out/activity.txt" | grep -q "$pkg/$activity" \
    || fail "dm's activity is not the resumed one (activity.txt)"
sh_adb dumpsys gfxinfo "$pkg" > "$out/gfxinfo.txt" || true
sh_adb dumpsys SurfaceFlinger --list > "$out/surfaces.txt" || true
grep -q "$pkg" "$out/surfaces.txt" || echo "::warning::no surface named $pkg in SurfaceFlinger's list"

adb exec-out screencap -p > "$out/screen.png"
adb exec-out screencap > "$out/screen.raw"
# The raw screen: width, height and format (and, from API 28, a colour space), then RGBA.
python3 - "$out/screen.raw" <<'PY' || fail "dm's window drew no frame with dm's blue in it (screen.png)"
import struct, sys
data = open(sys.argv[1], "rb").read()
width, height, form = struct.unpack_from("<III", data, 0)
pixels = data[len(data) - width * height * 4:]
r, g, b = pixels[0::4], pixels[1::4], pixels[2::4]
near = sum(
    1
    for x, y, z in zip(r, g, b)
    if abs(x - 0x1E) <= 16 and abs(y - 0x88) <= 16 and abs(z - 0xE5) <= 16
)
print(f"screen {width}x{height} (format {form}): {near} pixels of dm's blue")
sys.exit(0 if near >= 200 else 1)
PY

# 2. A link shared to dm. NativeActivity hands a running dm no new intent (docs/android.md,
# N5), so dm is stopped first and the share starts it, as Android does when dm is not open.
srv=$(mktemp -d)
head -c 3145728 /dev/urandom > "$srv/$probe"
want=$(sha256sum "$srv/$probe" | cut -d' ' -f1)
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$srv" > "$out/server.log" 2>&1 &
server=$!
sleep 1
adb shell am force-stop "$pkg"
adb shell am start -W -a android.intent.action.SEND -t text/plain \
    --es android.intent.extra.TEXT "http://10.0.2.2:$port/$probe" -n "$pkg/$activity"

# The places dm downloads to (src/android.rs, `downloads`): Download/dm in shared storage, the
# app's own folder on it, else its home; sorted by kind into a folder below each (B16).
found=
for _ in $(seq 1 60); do
    found=$(sh_adb "find /sdcard/Download /sdcard/Android/data/$pkg -name $probe -type f 2>/dev/null" | head -1 || true)
    if [ -n "$found" ]; then
        got=$(adb exec-out cat "$found" | sha256sum | cut -d' ' -f1)
    else
        found=$(sh_adb "run-as $pkg find files -name $probe -type f 2>/dev/null" | head -1 || true)
        [ -n "$found" ] && got=$(adb exec-out run-as "$pkg" cat "$found" | sha256sum | cut -d' ' -f1)
    fi
    if [ -n "$found" ] && [ "$got" = "$want" ]; then
        break
    fi
    sleep 2
done
grep -q "GET /$probe" "$out/server.log" || fail "dm never asked the server for the shared address (server.log)"
[ -n "$found" ] || fail "the shared download is in none of dm's download folders"
[ "$got" = "$want" ] || fail "$found is not the served file (sha256 $got, not $want)"
echo "the shared link downloaded whole: $found"
sh_adb dumpsys notification --noredact > "$out/notifications.txt" || true
grep -q "dm added a download" "$out/notifications.txt" \
    || echo "::warning::no \"dm added a download\" notification (notifications.txt)"

adb logcat -d -b crash > "$out/logcat-crash.txt" || true
if grep -q "$pkg" "$out/logcat-crash.txt"; then
    fail "dm crashed (logcat-crash.txt)"
fi
pid=$(sh_adb pidof "$pkg" || true)
[ -n "$pid" ] || fail "dm's process ended after the download (logcat.txt)"
echo "dm on the emulator: started, drew, took a shared link and downloaded it"
