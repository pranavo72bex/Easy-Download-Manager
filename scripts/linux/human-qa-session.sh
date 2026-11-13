#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Inside Dockerfile.human-qa's image (see linux-human-qa.sh), in /work: dm
# installed from /in/dm.deb as a person installs it (its window: dm-gpui, or
# dm-desktop in a .deb from before the GPUI window), or the checkout's build
# (/target/debug/$DM_LINUX_BINARY) put where the .deb puts it; a release's extension
# (/in/chrome) in place of dist/chrome; then an X display, a window manager
# and a session bus with an unlocked keyring, as a desktop has, and
# human-qa.py with its arguments. The screen is pictured every ten seconds
# into /shots, beside human-qa.py's own words, since nobody watches it.
set -e
binary="${DM_LINUX_BINARY:-dm-gpui}"
if [ -f /in/dm.deb ]; then
  apt-get update -qq >/dev/null
  apt-get install -y -qq /in/dm.deb >/dev/null 2>&1
  dpkg -s dm | grep -E '^(Version|Architecture)'
  binary=dm-desktop
  [ -x /usr/bin/dm-gpui ] && binary=dm-gpui
  echo "its window: $binary"
else
  install -m 755 "/target/debug/$binary" "/usr/bin/$binary"
  echo "a build of the checkout: $binary"
fi
if [ -d /in/chrome ]; then rm -rf dist/chrome && mkdir -p dist && cp -r /in/chrome dist/chrome; fi
python3 -c "import json; print('extension', json.load(open('dist/chrome/manifest.json'))['version'])"
export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 >/dev/null 2>&1 &
sleep 2
openbox >/dev/null 2>&1 &
( n=0; while sleep 10; do n=$((n + 1))
    import -window root "/shots/$(printf %04d "$n")-screen.png" >/dev/null 2>&1 || true
  done ) &
status=0
# The keyring's words are for the inner shell to read, so they are quoted from this one.
# shellcheck disable=SC2016
DM_BINARY="/usr/bin/$binary" DM_QA_EDGE=/usr/bin/chromium \
  dbus-run-session -- sh -c 'eval "$(printf "\n" | gnome-keyring-daemon --unlock --components=secrets 2>/dev/null)"
    python3 -u scripts/human-qa.py "$@"' qa "$@" 2>&1 | tee /shots/human-qa.txt || true
# human-qa.py's full record (each browser check's own words, dm's stderr)
# comes back with the screenshots: a check that stops part-way says why there.
mkdir -p /shots/record && cp /tmp/dm-human-qa/*.txt /shots/record/ 2>/dev/null || true
# The window's pictures this run took, to set beside the baseline when B25 fails.
cp -r /tmp/dm-human-qa/pictures /shots/record/ 2>/dev/null || true
grep -q "rows hold$" /shots/human-qa.txt && ! grep -q "failing or not checked" /shots/human-qa.txt || status=1
# --save-baseline: the pictures come back with the screenshots, and which
# window's they are.
echo "$binary" > /shots/binary
pictures=docs/behaviour-baseline/gpui/linux
if [ -d "$pictures" ]; then
  mkdir -p /shots/baseline && cp "$pictures"/*.png /shots/baseline/ 2>/dev/null || true
fi
exit $status
