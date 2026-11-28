#!/bin/bash
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# EDM's APK on a booted Android emulator (gaps row N13; .github/workflows/android.yml's
# emulator job, docs/android.md):
#
#   bash scripts/android-emulator-check.sh app-debug.apk out/
#
# Installs the APK (its permissions granted), starts EDM's activity and checks that its process
# lives, that the activity is the one resumed and that its window drew: EDM's real window (N3),
# so the screen between the status bar and the navigation bar is not blank, one colour or
# nearly so; it holds text, bars and buttons of many colours. Then EDM is
# stopped and started again by a share, as another app's share sheet does, with the address of
# a file served on the computer (10.0.2.2 from inside the emulator), and the file must arrive
# whole in one of the places EDM downloads to. What it saw stays in `out/`: the screen
# (screen.png), logcat, the activity's and the window's dumps, the server's log.
#
# Needs adb on PATH with one emulator connected, and python3. The APK must be a debug one:
# `run-as` reads EDM's private folder.

set -euo pipefail

apk=${1:?the APK}
out=${2:?the folder for what was seen}
pkg=com.edm.android
activity=com.edm.android.EdmActivity
probe=edm-ci-probe.bin
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

# An emulator may refuse to clear the main log (Android 8.0's did); what is read later is
# filtered by EDM.
adb logcat -c || true
adb install -r -g "$apk"

# 1. EDM started as the launcher starts it.
adb shell am start -W -n "$pkg/$activity"
sleep 20
pid=$(sh_adb pidof "$pkg" || true)
[ -n "$pid" ] || fail "EDM's process is not running 20 s after start (see logcat.txt)"
echo "EDM runs as process $pid"

sh_adb dumpsys activity activities > "$out/activity.txt"
# dumpsys names it short (com.edm.android/.EdmActivity) where the class is in the package.
grep -E 'Resumed' "$out/activity.txt" | grep -qE "$pkg/($activity|\.${activity##*.})\b" \
    || fail "EDM's activity is not the resumed one (activity.txt)"
sh_adb dumpsys gfxinfo "$pkg" > "$out/gfxinfo.txt" || true
sh_adb dumpsys SurfaceFlinger --list > "$out/surfaces.txt" || true
grep -q "$pkg" "$out/surfaces.txt" || echo "::warning::no surface named $pkg in SurfaceFlinger's list"

adb exec-out screencap -p > "$out/screen.png"
adb exec-out screencap > "$out/screen.raw"
# The raw screen: width, height and format (and, from API 28, a colour space), then RGBA; the
# pixels are its last width x height x 4 bytes, whatever the header's size. The status bar and
# the navigation bar (the top 6 % and the bottom 8 %) are left out: the system draws them over
# a blank window too. What is left must not be blank: at least 2 % of its pixels unlike the
# commonest colour (the background) and at least 16 colours (4 bits a channel), as text and
# its anti-aliasing give.
# EDM_NO_GPU=1: the emulator offers no GPU EDM's renderer can use (Android 10's image: no guest
# Vulkan, and OpenGL ES 3.0 has no storage buffers), so the window is not looked at; EDM's start
# and the shared download below still are.
blank="EDM's window is blank: one colour, or nearly (screen.png)"
if [ "${EDM_NO_GPU:-0}" = 1 ]; then
    blank="::notice::not drawn here, as expected: this emulator has no GPU EDM can use"
    echo "::notice::the window is not checked on this emulator (EDM_NO_GPU)"
fi
python3 - "$out/screen.raw" <<'PY' || { [ "${EDM_NO_GPU:-0}" = 1 ] && echo "$blank"; } || fail "$blank"
import struct, sys
from collections import Counter
data = open(sys.argv[1], "rb").read()
width, height, form = struct.unpack_from("<III", data, 0)
pixels = data[len(data) - width * height * 4:]
top, bottom = int(height * 0.06), int(height * 0.92)
colours = Counter()
for y in range(top, bottom, 2):
    row = pixels[y * width * 4:(y + 1) * width * 4]
    for i in range(0, len(row), 8):
        colours[(row[i] >> 4, row[i + 1] >> 4, row[i + 2] >> 4)] += 1
seen = sum(colours.values())
background, most = colours.most_common(1)[0] if colours else ((0, 0, 0), 0)
other = seen - most
share = other / seen if seen else 0.0
print(
    f"screen {width}x{height} (format {form}): {len(colours)} colours, "
    f"{share:.1%} of the window unlike its background {background}"
)
sys.exit(0 if len(colours) >= 16 and share >= 0.02 else 1)
PY

# 2. A link shared to EDM. NativeActivity hands a running EDM no new intent (docs/android.md,
# N5), so EDM is stopped first and the share starts it, as Android does when EDM is not open.
srv=$(mktemp -d)
head -c 3145728 /dev/urandom > "$srv/$probe"
want=$(sha256sum "$srv/$probe" | cut -d' ' -f1)
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$srv" > "$out/server.log" 2>&1 &
server=$!
sleep 1
adb shell am force-stop "$pkg"
# An earlier run's probe is taken away first: left in place, EDM saves this run's beside it as
# "edm-ci-probe (1).bin" and the find below reads the old one (seen on a reused emulator).
sh_adb "rm -f /sdcard/Download/EDM/Other/edm-ci-probe*.bin" || true
adb shell am start -W -a android.intent.action.SEND -t text/plain \
    --es android.intent.extra.TEXT "http://10.0.2.2:$port/$probe" -n "$pkg/$activity"

# The places EDM downloads to (src/android.rs, `downloads`): Download/EDM in shared storage, the
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
grep -q "GET /$probe" "$out/server.log" || fail "EDM never asked the server for the shared address (server.log)"
[ -n "$found" ] || fail "the shared download is in none of EDM's download folders"
[ "$got" = "$want" ] || fail "$found is not the served file (sha256 $got, not $want)"
echo "the shared link downloaded whole: $found"
sh_adb dumpsys notification --noredact > "$out/notifications.txt" || true
grep -q "EDM added a download" "$out/notifications.txt" \
    || echo "::warning::no \"EDM added a download\" notification (notifications.txt)"

adb logcat -d -b crash > "$out/logcat-crash.txt" || true
if grep -q "$pkg" "$out/logcat-crash.txt"; then
    fail "EDM crashed (logcat-crash.txt)"
fi
pid=$(sh_adb pidof "$pkg" || true)
[ -n "$pid" ] || fail "EDM's process ended after the download (logcat.txt)"
echo "EDM on the emulator: started, drew, took a shared link and downloaded it"
