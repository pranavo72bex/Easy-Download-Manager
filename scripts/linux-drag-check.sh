#!/bin/sh
# Drag and drop with dm's window on Linux (X11), with real input:
#
#   sh scripts/linux-drag-check.sh
#
# Under Xvfb with a window manager and xdotool, a link is dragged from a page
# in Firefox onto dm's window, as a hand does it (scripts/linux/drag_check.py).
# Firefox is fetched into a Docker volume (dm-firefox-cache), as in
# linux-firefox-check.sh. Streamed in, not mounted, for the reason
# linux-check.sh gives.
set -e
cd "$(dirname "$0")/.."
docker volume rm dm-drag-shots >/dev/null 2>&1 || true
docker build -t dm-linux-check scripts/linux
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -v dm-linux-target:/target \
      -v dm-firefox-cache:/root/.cache/dm-firefox \
      -v dm-drag-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool wmctrl dbus-x11 xz-utils bzip2 \
          libasound2 libdbus-glib-1-2 libx11-xcb1 libpci3 libgtk-3-0 imagemagick >/dev/null
        cd desktop && cargo build -q -p dm-desktop && cd ..
        export DISPLAY=:79 DM_BINARY=/target/debug/dm-desktop
        Xvfb :79 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        dbus-launch --exit-with-session python3 scripts/linux/drag_check.py
      ' || status=$?
# Screenshots and dm's log, out of the volume, into DM_SHOTS.
SHOTS="${DM_SHOTS:-$PWD/human-shots/drag}"
mkdir -p "$SHOTS"
docker rm -f dm-drag-shots-copy >/dev/null 2>&1 || true
docker create --name dm-drag-shots-copy -v dm-drag-shots:/shots dm-linux-check true >/dev/null
docker cp dm-drag-shots-copy:/shots/. "$SHOTS" >/dev/null 2>&1 || true
docker rm -f dm-drag-shots-copy >/dev/null 2>&1 || true
exit "${status:-0}"
