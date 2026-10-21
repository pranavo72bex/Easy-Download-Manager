#!/bin/sh
# dm as someone new meets it, with real X11 input, in a Linux container:
#
#   sh scripts/linux-newcomer-check.sh          # screenshots into ./human-shots/newcomer
#   DM_SHOTS=/some/folder sh scripts/linux-newcomer-check.sh
#
# Runs scripts/linux/newcomer.py under Xvfb with a window manager (openbox),
# xdotool and the accessibility bus, on a fresh dm home: the browser bar, a
# file added by hand, a broken link, words that are no link, a real YouTube
# page with nothing installed (needs the network), Options › Browser and
# not now. Screenshots are copied out of a Docker volume, since Docker
# cannot mount a folder under ~/Downloads.
set -e
cd "$(dirname "$0")/.."
docker volume rm dm-newcomer-shots >/dev/null 2>&1 || true
docker build -q -t dm-linux-check scripts/linux >/dev/null
status=0
git ls-files -z --cached --others --exclude-standard \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -v dm-linux-target:/target \
      -v dm-newcomer-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends openbox xdotool x11-utils wmctrl dbus-x11 \
          at-spi2-core python3-pyatspi libglib2.0-bin imagemagick >/dev/null
        cd desktop && cargo build -q -p dm-desktop && cd ..
        export DISPLAY=:81 DM_BINARY=/target/debug/dm-desktop
        Xvfb :81 -screen 0 1280x800x24 >/dev/null 2>&1 &
        sleep 1
        openbox >/dev/null 2>&1 &
        dbus-launch --exit-with-session python3 scripts/linux/newcomer.py
      ' || status=$?
SHOTS="${DM_SHOTS:-$PWD/human-shots/newcomer}"
mkdir -p "$SHOTS"
docker rm -f dm-newcomer-shots-copy >/dev/null 2>&1 || true
docker create --name dm-newcomer-shots-copy -v dm-newcomer-shots:/shots dm-linux-check true >/dev/null
docker cp dm-newcomer-shots-copy:/shots/. "$SHOTS" >/dev/null 2>&1 || true
docker rm -f dm-newcomer-shots-copy >/dev/null 2>&1 || true
echo "screenshots: $SHOTS"
exit $status
