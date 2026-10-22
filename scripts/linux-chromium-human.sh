#!/bin/sh
# Chromium and dm's own window in a Linux container, used as a person uses
# them: a real pointer and real keys, and nothing else.
#
#   sh scripts/package.sh                       # builds dist/chrome
#   sh scripts/linux-chromium-human.sh          # screenshots into ./human-shots
#   DM_SHOTS=/some/folder sh scripts/linux-chromium-human.sh
#   DM_DISTRO=fedora sh scripts/linux-chromium-human.sh   # or arch
#   DM_DISTRO=edge sh scripts/linux-chromium-human.sh     # Microsoft Edge
#
# Runs scripts/chromium-human.py under Xvfb with a window manager (openbox),
# xdotool and the accessibility bus: dm opened with its window, Chromium
# with dist/chrome loaded and dm registered with `dm-desktop --register`,
# then links clicked and right-clicked, a video's panel clicked, an
# Alt-click, dm's Stop, Resume and Delete clicked, and the toolbar popup's
# "Catch downloads" switched off — every one an X11 event. DevTools and dm's
# control protocol only read what happened, and take the screenshots.
#
# Streamed in, not mounted, for the reason linux-check.sh gives; the built
# extension is ignored by git, so it is added to the stream. The screenshots
# come back out through a Docker volume (dm-human-shots), since Docker cannot
# mount a folder under ~/Downloads.
set -e
cd "$(dirname "$0")/.."
# DM_DISTRO runs the same check in Fedora 41's or Arch's own image with the
# distribution's own Chromium, dm built here in Debian and copied in
# (scripts/linux/human-distro.sh). Not Ubuntu: its Chromium is a snap, and
# snapd does not run in a container. DM_DISTRO=edge is Debian 12 with
# Microsoft Edge from Microsoft's repository instead of Chromium, x86_64
# only (under emulation on arm64), and runs chrome-check.py with Edge after
# the real-input check (DM_EDGE_CHROME_CHECK=0 skips that).
case "${DM_DISTRO:-debian}" in
  debian) ;;
  fedora|arch|edge) exec sh scripts/linux/human-distro.sh chromium "$DM_DISTRO" ;;
  ubuntu*) echo "Ubuntu's Chromium is a snap, which cannot run in a container: not checked there"; exit 2 ;;
  *) echo "DM_DISTRO is debian, fedora, arch or edge"; exit 2 ;;
esac
[ -f dist/chrome/pkg/dm_catcher_bg.wasm ] || { echo "no dist/chrome — run: sh scripts/package.sh"; exit 1; }
SHOTS="${DM_SHOTS:-$PWD/human-shots}"
docker build -q -t dm-linux-check scripts/linux >/dev/null
status=0
{ git ls-files -z --cached --others --exclude-standard; find dist/chrome -type f -print0; } \
  | tar --null -T - -cf - \
  | docker run --rm -i \
      -v dm-linux-target:/target \
      -v dm-human-shots:/shots \
      -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
      dm-linux-check sh -c '
        set -e
        mkdir -p /work && cd /work && tar -xf -
        rm -rf /shots/*
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends chromium openbox xdotool dbus-x11 \
          at-spi2-core python3-pyatspi libglib2.0-bin imagemagick >/dev/null 2>&1
        cd desktop && cargo build -q -p dm-desktop && cd ..
        export DISPLAY=:79 DM_BINARY=/target/debug/dm-desktop DM_SHOTS=/shots
        Xvfb :79 -screen 0 2400x1200x24 >/dev/null 2>&1 &
        sleep 1
        # openbox takes Alt+click for moving windows; a person Alt-clicking a
        # link in a browser means the browser, so that binding is left out.
        sed "/<mousebind button=\"A-Left\"/,/<\/mousebind>/d" /etc/xdg/openbox/rc.xml > /tmp/openbox-rc.xml
        openbox --config-file /tmp/openbox-rc.xml >/dev/null 2>&1 &
        dbus-launch --exit-with-session python3 scripts/chromium-human.py
      ' || status=$?
mkdir -p "$SHOTS"
docker run --rm -v dm-human-shots:/shots dm-linux-check tar -C /shots -cf - . | tar -xf - -C "$SHOTS"
echo "screenshots: $SHOTS"
exit $status
