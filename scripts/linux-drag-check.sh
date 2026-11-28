#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the EDM contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Drag and drop with EDM's window on Linux (X11), with real input:
#
#   sh scripts/linux-drag-check.sh
#
# Under Xvfb with a window manager and xdotool, a link is dragged from a page
# in Firefox onto EDM's window, as a hand does it (scripts/linux/drag_check.py).
# Firefox is fetched into a Docker volume (edm-firefox-cache), as in
# linux-firefox-check.sh. Streamed in, not mounted, for the reason
# linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
# shellcheck source=scripts/linux/window.sh
. scripts/linux/window.sh
docker volume rm edm-drag-shots >/dev/null 2>&1 || true
docker build -t edm-linux-check scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -e EDM_BINARY="/target/debug/$binary" \
      -v edm-linux-target:/target \
      -v edm-firefox-cache:/root/.cache/edm-firefox \
      -v edm-drag-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      edm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool wmctrl dbus-x11 xz-utils bzip2 \
          libasound2 libdbus-glib-1-2 libx11-xcb1 libpci3 libgtk-3-0 imagemagick >/dev/null
        cargo build -q -p "$(basename "$EDM_BINARY")"
        export DISPLAY=:79
        Xvfb :79 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        dbus-launch --exit-with-session python3 scripts/linux/drag_check.py
      ' || status=$?
# Screenshots and EDM's log, out of the volume, into EDM_SHOTS.
SHOTS="${EDM_SHOTS:-$PWD/human-shots/drag}"
mkdir -p "$SHOTS"
docker rm -f edm-drag-shots-copy >/dev/null 2>&1 || true
docker create --name edm-drag-shots-copy -v edm-drag-shots:/shots edm-linux-check true >/dev/null
docker cp edm-drag-shots-copy:/shots/. "$SHOTS" >/dev/null 2>&1 || true
docker rm -f edm-drag-shots-copy >/dev/null 2>&1 || true
exit "${status:-0}"
