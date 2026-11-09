#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Inside Dockerfile.human-qa's image (see linux-human-qa.sh), in /work: dm
# installed from /in/dm.deb as a person installs it, or the checkout's build
# (/target/debug/dm-desktop) put where the .deb puts it; a release's extension
# (/in/chrome) in place of dist/chrome; then an X display, a window manager
# and a session bus with an unlocked keyring, as a desktop has, and
# human-qa.py with its arguments. The screen is pictured every ten seconds
# into /shots, beside human-qa.py's own words, since nobody watches it.
set -e
if [ -f /in/dm.deb ]; then
  apt-get update -qq >/dev/null
  apt-get install -y -qq /in/dm.deb >/dev/null 2>&1
  dpkg -s dm | grep -E '^(Version|Architecture)'
else
  install -m 755 /target/debug/dm-desktop /usr/bin/dm-desktop
  echo "a build of the checkout"
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
DM_BINARY=/usr/bin/dm-desktop DM_QA_EDGE=/usr/bin/chromium \
  dbus-run-session -- sh -c 'eval "$(printf "\n" | gnome-keyring-daemon --unlock --components=secrets 2>/dev/null)"
    python3 -u scripts/human-qa.py "$@"' qa "$@" 2>&1 | tee /shots/human-qa.txt || true
grep -q "rows hold$" /shots/human-qa.txt && ! grep -q "failing or not checked" /shots/human-qa.txt || status=1
# --save-baseline: the pictures come back with the screenshots.
if [ -d docs/behaviour-baseline/linux ]; then
  mkdir -p /shots/baseline && cp docs/behaviour-baseline/linux/*.png /shots/baseline/ 2>/dev/null || true
fi
exit $status
